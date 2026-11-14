//! Reusable original voxel wardrobe and presentation of the common damage rig.
use super::PlayerBodyVisual;
use crate::{ClientSystems, sync::ClientSession};
use bevy::{platform::collections::HashMap, prelude::*};
use common::{combat::*, humanoid::*, *};

pub struct Plugin;
impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PoseCache>()
            .add_systems(Startup, assets)
            .add_systems(PostUpdate, verify_native_replication)
            .add_systems(
                Update,
                (build_actors, interpolate, animate, sync_pieces)
                    .chain()
                    .after(super::feedback::advance)
                    .in_set(ClientSystems::Present),
            );
    }
}

#[derive(Component)]
pub(crate) struct RemoteTarget {
    pub position: Vec3,
    pub rotation: Quat,
    pub snap: bool,
}
#[derive(Component)]
struct BoneVisual {
    owner: u64,
    segment: Segment,
    inner: u8,
}
#[derive(Component)]
struct EquipmentVisual {
    owner: u64,
    helmet: bool,
}
#[derive(Component)]
struct PieceVisual(u64);
#[derive(Default, Resource)]
struct PoseCache {
    raid: u32,
    poses: HashMap<u64, PoseState>,
}
#[derive(Component)]
struct WorldWeapon {
    owner: u64,
}
#[derive(Component)]
struct WorldFlash(u64);
#[derive(Component)]
struct DetailVisual {
    owner: u64,
    segment: Segment,
    offset: Vec3,
    scale: Vec3,
    armor: bool,
}

#[derive(Resource)]
struct VoxelAssets {
    intact: Vec<Handle<Mesh>>,
    wounded: Vec<Handle<Mesh>>,
    inside: Vec<Handle<Mesh>>,
    cores: Vec<Handle<Mesh>>,
    organs: Vec<Handle<Mesh>>,
    organ_materials: Vec<Handle<StandardMaterial>>,
    cloth: Handle<StandardMaterial>,
    sentry: Handle<StandardMaterial>,
    skin: Handle<StandardMaterial>,
    leather: Handle<StandardMaterial>,
    flesh: Handle<StandardMaterial>,
    bone: Handle<StandardMaterial>,
    vest: Handle<StandardMaterial>,
    helmet: Handle<Mesh>,
    plate: Handle<Mesh>,
    weapons: Vec<Handle<Mesh>>,
    flash: Handle<StandardMaterial>,
    cube: Handle<Mesh>,
}

/// Stepped silhouettes and a carved entry cavity. Geometry is generated once per segment/state.
fn voxel_mesh(segment: Segment, wounded: bool, interior: bool) -> Mesh {
    let half = segment.half_size();
    let cell = 0.045;
    let n = (half * 2.0 / cell).ceil().as_ivec3().max(IVec3::ONE);
    let size = half * 2.0 / n.as_vec3();
    let mut merged: Option<Mesh> = None;
    for x in 0..n.x {
        for y in 0..n.y {
            for z in 0..n.z {
                let p = (Vec3::new(x as f32, y as f32, z as f32) + Vec3::splat(0.5)) * size - half;
                let q = p / half;
                let shaped = q.x * q.x + q.z * q.z < 1.45 && q.y.abs() < 1.0;
                let cavity = wounded && p.z < 0.01 && (p.x * p.x + p.y * p.y) < 0.007;
                let occupied = if interior {
                    q.x.abs() < 0.4 && q.z.abs() < 0.45 && (wounded || q.y.abs() < 0.5)
                } else {
                    shaped && !cavity
                };
                if !occupied {
                    continue;
                }
                let mesh = Mesh::from(Cuboid::from_size(
                    size * if interior { 0.92 } else { 0.985 },
                ))
                .translated_by(p);
                if let Some(m) = &mut merged {
                    m.merge(&mesh).expect("identical voxel attributes");
                } else {
                    merged = Some(mesh);
                }
            }
        }
    }
    merged.unwrap_or_else(|| Mesh::from(Cuboid::from_size(half)))
}

fn material(
    materials: &mut Assets<StandardMaterial>,
    color: Color,
    roughness: f32,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: color,
        perceptual_roughness: roughness,
        ..default()
    })
}
fn assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(VoxelAssets {
        intact: SEGMENTS
            .iter()
            .map(|s| meshes.add(voxel_mesh(*s, false, false)))
            .collect(),
        wounded: SEGMENTS
            .iter()
            .map(|s| meshes.add(voxel_mesh(*s, true, false)))
            .collect(),
        inside: SEGMENTS
            .iter()
            .map(|s| meshes.add(voxel_mesh(*s, true, true)))
            .collect(),
        cores: SEGMENTS
            .iter()
            .map(|s| {
                meshes.add(Cuboid::from_size(
                    s.half_size() * Vec3::new(0.30, 1.6, 0.30),
                ))
            })
            .collect(),
        organs: ORGANS.iter().map(|o| meshes.add(organ_mesh(*o))).collect(),
        organ_materials: [
            Color::srgb(0.74, 0.53, 0.49),
            Color::srgb(0.45, 0.025, 0.035),
            Color::srgb(0.23, 0.055, 0.035),
            Color::srgb(0.58, 0.30, 0.15),
        ]
        .into_iter()
        .map(|c| material(&mut materials, c, 0.6))
        .collect(),
        cloth: material(&mut materials, Color::srgb(0.12, 0.31, 0.32), 0.95),
        sentry: material(&mut materials, Color::srgb(0.48, 0.16, 0.09), 0.95),
        skin: material(&mut materials, Color::srgb(0.65, 0.43, 0.28), 0.8),
        leather: material(&mut materials, Color::srgb(0.055, 0.065, 0.06), 0.9),
        flesh: material(&mut materials, Color::srgb(0.43, 0.035, 0.045), 0.65),
        bone: material(&mut materials, Color::srgb(0.83, 0.72, 0.50), 0.8),
        vest: material(&mut materials, Color::srgb(0.21, 0.24, 0.14), 0.9),
        helmet: meshes.add(Cuboid::new(0.34, 0.16, 0.32)),
        plate: meshes.add(Cuboid::new(0.39, 0.32, 0.055)),
        weapons: [WeaponKind::Rifle, WeaponKind::Pistol, WeaponKind::Smg]
            .into_iter()
            .map(|w| meshes.add(world_gun(w)))
            .collect(),
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        flash: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.7, 0.1),
            emissive: LinearRgba::rgb(12.0, 5.0, 0.3),
            unlit: true,
            ..default()
        }),
    });
}

fn world_gun(weapon: WeaponKind) -> Mesh {
    let mut mesh = Mesh::from(Cuboid::new(
        0.085,
        0.10,
        if weapon == WeaponKind::Pistol {
            0.24
        } else {
            0.32
        },
    ));
    let parts = if weapon == WeaponKind::Pistol {
        vec![
            (Vec3::new(0.07, 0.16, 0.08), Vec3::new(0.0, -0.10, 0.05)),
            (Vec3::new(0.07, 0.06, 0.24), Vec3::new(0.0, 0.07, -0.02)),
        ]
    } else {
        let barrel = if weapon == WeaponKind::Rifle {
            0.37
        } else {
            0.23
        };
        vec![
            (
                Vec3::new(0.04, 0.04, barrel),
                Vec3::new(0.0, 0.02, -0.16 - barrel * 0.5),
            ),
            (
                Vec3::new(
                    0.075,
                    0.19,
                    if weapon == WeaponKind::Smg {
                        0.055
                    } else {
                        0.10
                    },
                ),
                Vec3::new(0.0, -0.14, -0.02),
            ),
            (Vec3::new(0.07, 0.16, 0.08), Vec3::new(0.0, -0.10, 0.12)),
            (Vec3::new(0.075, 0.11, 0.22), Vec3::new(0.0, 0.0, 0.24)),
            (Vec3::new(0.05, 0.04, 0.06), Vec3::new(0.0, 0.085, -0.08)),
        ]
    };
    for (size, offset) in parts {
        mesh.merge(&Mesh::from(Cuboid::from_size(size)).translated_by(offset))
            .expect("weapon attributes");
    }
    mesh
}

fn organ_mesh(organ: Organ) -> Mesh {
    let (_, _, half) = organ.volume();
    let n = (half * 2.0 / 0.025).ceil().as_ivec3();
    let size = half * 2.0 / n.as_vec3();
    let mut mesh: Option<Mesh> = None;
    for x in 0..n.x {
        for y in 0..n.y {
            for z in 0..n.z {
                let p = (Vec3::new(x as f32, y as f32, z as f32) + Vec3::splat(0.5)) * size - half;
                let q = p / half;
                let shape = match organ {
                    Organ::Brain => q.length_squared() < 1.25 && q.x.abs() > 0.055,
                    Organ::Heart => {
                        q.x * q.x + q.z * q.z + q.y * q.y * (if q.y < 0.0 { 1.6 } else { 0.8 })
                            < 1.1
                    }
                    Organ::Liver => (q.x + q.y * 0.35).powi(2) + q.y * q.y + q.z * q.z < 1.25,
                    _ => q.length_squared() < 1.3 && !(q.x > 0.2 && q.y > 0.2),
                };
                if shape {
                    let cube = Mesh::from(Cuboid::from_size(size * 0.97)).translated_by(p);
                    if let Some(m) = &mut mesh {
                        m.merge(&cube).expect("organ attributes");
                    } else {
                        mesh = Some(cube);
                    }
                }
            }
        }
    }
    mesh.unwrap_or_else(|| Mesh::from(Cuboid::from_size(half)))
}

fn surface(assets: &VoxelAssets, s: Segment, bot: bool) -> Handle<StandardMaterial> {
    use Segment::*;
    if matches!(s, Head | Neck) {
        assets.skin.clone()
    } else if matches!(s, LeftFoot | RightFoot | LeftHand | RightHand) {
        assets.leather.clone()
    } else if bot {
        assets.sentry.clone()
    } else {
        assets.cloth.clone()
    }
}

fn build_actors(
    mut commands: Commands,
    roots: Query<(Entity, &ChildOf), Added<PlayerBodyVisual>>,
    clients: Query<&Client>,
    session: Res<ClientSession>,
    assets: Res<VoxelAssets>,
) {
    for (entity, parent) in &roots {
        let Ok(client) = clients.get(parent.0) else {
            continue;
        };
        let bot = session.local(client.id).is_some_and(|p| p.bot);
        commands.entity(entity).with_children(|c| {
            for segment in SEGMENTS {
                c.spawn((
                    BoneVisual {
                        owner: client.id,
                        segment,
                        inner: 0,
                    },
                    Mesh3d(assets.intact[segment.index()].clone()),
                    MeshMaterial3d(surface(&assets, segment, bot)),
                    Transform::default(),
                ));
                c.spawn((
                    BoneVisual {
                        owner: client.id,
                        segment,
                        inner: 1,
                    },
                    Mesh3d(assets.inside[segment.index()].clone()),
                    MeshMaterial3d(assets.flesh.clone()),
                    Transform::default(),
                    Visibility::Hidden,
                ));
                c.spawn((
                    BoneVisual {
                        owner: client.id,
                        segment,
                        inner: 2,
                    },
                    Mesh3d(assets.cores[segment.index()].clone()),
                    MeshMaterial3d(assets.bone.clone()),
                    Transform::default(),
                    Visibility::Hidden,
                ));
            }
            c.spawn((
                EquipmentVisual {
                    owner: client.id,
                    helmet: true,
                },
                Mesh3d(assets.helmet.clone()),
                MeshMaterial3d(assets.vest.clone()),
                Transform::default(),
            ));
            c.spawn((
                EquipmentVisual {
                    owner: client.id,
                    helmet: false,
                },
                Mesh3d(assets.plate.clone()),
                MeshMaterial3d(assets.vest.clone()),
                Transform::default(),
            ));
            c.spawn((
                WorldWeapon { owner: client.id },
                Mesh3d(assets.weapons[0].clone()),
                MeshMaterial3d(assets.leather.clone()),
                Transform::default(),
            ));
            c.spawn((
                WorldFlash(client.id),
                Mesh3d(assets.helmet.clone()),
                MeshMaterial3d(assets.flash.clone()),
                Transform::from_scale(Vec3::splat(0.3)),
                Visibility::Hidden,
            ));
            for organ in ORGANS {
                let (segment, offset, _half) = organ.volume();
                // Own voxel organ geometry remains inside the matching articulated parent.
                c.spawn((
                    OrganVisual {
                        owner: client.id,
                        organ,
                        segment,
                        offset,
                    },
                    Mesh3d(assets.organs[organ as usize].clone()),
                    MeshMaterial3d(assets.organ_materials[organ as usize].clone()),
                    Transform::default(),
                    Visibility::Hidden,
                ));
            }
            for segment in SEGMENTS.into_iter().filter(|s| s.limb()) {
                c.spawn((
                    StumpVisual {
                        owner: client.id,
                        missing: segment,
                        parent: segment.parent().unwrap(),
                    },
                    Mesh3d(assets.cube.clone()),
                    MeshMaterial3d(assets.flesh.clone()),
                    Transform::default(),
                    Visibility::Hidden,
                ))
                .with_child((
                    Mesh3d(assets.cube.clone()),
                    MeshMaterial3d(assets.bone.clone()),
                    Transform::from_scale(Vec3::new(0.30, 1.08, 0.30)),
                ));
            }
            for segment in [
                Segment::LeftShin,
                Segment::RightShin,
                Segment::LeftForearm,
                Segment::RightForearm,
            ] {
                c.spawn((
                    DetailVisual {
                        owner: client.id,
                        segment,
                        offset: Vec3::new(0.0, 0.08, -segment.half_size().z),
                        scale: Vec3::new(0.14, 0.10, 0.045),
                        armor: false,
                    },
                    Mesh3d(assets.cube.clone()),
                    MeshMaterial3d(assets.leather.clone()),
                    Transform::default(),
                ));
            }
            for x in [-0.12, 0.0, 0.12] {
                c.spawn((
                    DetailVisual {
                        owner: client.id,
                        segment: Segment::Thorax,
                        offset: Vec3::new(x, -0.05, -0.20),
                        scale: Vec3::new(0.09, 0.15, 0.06),
                        armor: true,
                    },
                    Mesh3d(assets.cube.clone()),
                    MeshMaterial3d(assets.vest.clone()),
                    Transform::default(),
                ));
            }
            for x in [-0.062, 0.062] {
                c.spawn((
                    DetailVisual {
                        owner: client.id,
                        segment: Segment::Head,
                        offset: Vec3::new(x, 0.025, -0.142),
                        scale: Vec3::new(0.045, 0.035, 0.025),
                        armor: false,
                    },
                    Mesh3d(assets.cube.clone()),
                    MeshMaterial3d(assets.leather.clone()),
                    Transform::default(),
                ));
            }
            c.spawn((
                DetailVisual {
                    owner: client.id,
                    segment: Segment::Head,
                    offset: Vec3::new(0.0, -0.025, -0.16),
                    scale: Vec3::new(0.045, 0.07, 0.055),
                    armor: false,
                },
                Mesh3d(assets.cube.clone()),
                MeshMaterial3d(assets.skin.clone()),
                Transform::default(),
            ));
            for segment in [Segment::LeftFoot, Segment::RightFoot] {
                c.spawn((
                    DetailVisual {
                        owner: client.id,
                        segment,
                        offset: Vec3::new(0.0, -0.045, -0.015),
                        scale: Vec3::new(0.19, 0.03, 0.31),
                        armor: false,
                    },
                    Mesh3d(assets.cube.clone()),
                    MeshMaterial3d(assets.leather.clone()),
                    Transform::default(),
                ));
            }
            c.spawn((
                DetailVisual {
                    owner: client.id,
                    segment: Segment::Thorax,
                    offset: Vec3::new(0.0, 0.02, 0.20),
                    scale: if bot {
                        Vec3::new(0.36, 0.38, 0.19)
                    } else {
                        Vec3::new(0.28, 0.30, 0.10)
                    },
                    armor: false,
                },
                Mesh3d(assets.cube.clone()),
                MeshMaterial3d(assets.vest.clone()),
                Transform::default(),
            ));
        });
    }
}
#[derive(Component)]
struct OrganVisual {
    owner: u64,
    organ: Organ,
    segment: Segment,
    offset: Vec3,
}

#[derive(Component)]
struct StumpVisual {
    owner: u64,
    missing: Segment,
    parent: Segment,
}

fn interpolate(time: Res<Time>, mut actors: Query<(&mut Transform, &mut RemoteTarget)>) {
    let alpha = 1.0 - (-time.delta_secs() * 18.0).exp();
    for (mut t, mut target) in &mut actors {
        let snap = target.snap || t.translation.distance(target.position) > 4.0;
        t.translation = if snap {
            target.position
        } else {
            t.translation.lerp(target.position, alpha)
        };
        t.rotation = t
            .rotation
            .slerp(target.rotation, if snap { 1.0 } else { alpha });
        target.snap = false;
    }
}

fn animate(
    session: Res<ClientSession>,
    assets: Res<VoxelAssets>,
    feedback: Res<super::feedback::Feedback>,
    time: Res<Time<Real>>,
    mut cache: ResMut<PoseCache>,
    mut bones: Query<
        (&BoneVisual, &mut Transform, &mut Mesh3d, &mut Visibility),
        (
            Without<EquipmentVisual>,
            Without<OrganVisual>,
            Without<WorldWeapon>,
            Without<WorldFlash>,
            Without<DetailVisual>,
        ),
    >,
    mut gear: Query<
        (&EquipmentVisual, &mut Transform, &mut Visibility),
        (
            Without<BoneVisual>,
            Without<OrganVisual>,
            Without<WorldWeapon>,
            Without<WorldFlash>,
            Without<DetailVisual>,
        ),
    >,
    mut organs: Query<
        (&OrganVisual, &mut Transform, &mut Visibility),
        (
            Without<BoneVisual>,
            Without<EquipmentVisual>,
            Without<WorldWeapon>,
            Without<WorldFlash>,
            Without<DetailVisual>,
        ),
    >,
    mut weapons: Query<
        (&WorldWeapon, &mut Transform, &mut Mesh3d, &mut Visibility),
        (
            Without<BoneVisual>,
            Without<EquipmentVisual>,
            Without<OrganVisual>,
            Without<WorldFlash>,
            Without<DetailVisual>,
        ),
    >,
    mut flashes: Query<
        (&WorldFlash, &mut Transform, &mut Visibility),
        (
            Without<BoneVisual>,
            Without<EquipmentVisual>,
            Without<OrganVisual>,
            Without<WorldWeapon>,
            Without<DetailVisual>,
        ),
    >,
    mut details: Query<
        (&DetailVisual, &mut Transform, &mut Visibility),
        (
            Without<BoneVisual>,
            Without<EquipmentVisual>,
            Without<OrganVisual>,
            Without<WorldWeapon>,
            Without<WorldFlash>,
        ),
    >,
    mut stumps: Query<
        (&StumpVisual, &mut Transform, &mut Visibility),
        (
            Without<BoneVisual>,
            Without<EquipmentVisual>,
            Without<OrganVisual>,
            Without<WorldWeapon>,
            Without<WorldFlash>,
            Without<DetailVisual>,
        ),
    >,
) {
    if cache.raid != session.raid.raid_id {
        cache.poses.clear();
        cache.raid = session.raid.raid_id;
    }
    let age = (time.elapsed_secs_f64() - session.last_received).clamp(0.0, 0.065) as f32;
    let alpha = 1.0 - (-time.delta_secs() * 24.0).exp();
    let mut rigs = HashMap::new();
    for p in &session.players {
        let state = cache.poses.entry(p.id).or_insert_with(|| p.pose.clone());
        state.crouch += (p.pose.crouch - state.crouch) * alpha;
        state.velocity = Vec3::from(state.velocity)
            .lerp(p.pose.velocity.into(), alpha)
            .into();
        let speed = Vec2::new(p.pose.velocity[0], p.pose.velocity[2]).length();
        let phase = p.pose.phase
            + if p.pose.grounded {
                age * speed * std::f32::consts::PI / (2.0 * (0.28 - p.pose.crouch * 0.1))
            } else {
                0.0
            };
        let delta = phase - state.phase;
        state.phase += delta.sin().atan2(delta.cos()) * alpha;
        state.grounded = p.pose.grounded;
        state.mode = p.pose.mode;
        state.mode_time = p.pose.mode_time + age;
        state.aim_pitch += (p.pose.aim_pitch - state.aim_pitch) * alpha;
        for i in 0..2 {
            state.lean[i] += (p.pose.lean[i] - state.lean[i]) * alpha;
        }
        state.landing = p.pose.landing;
        state.weapon = p.weapon;
        state.reload_fraction = p.pose.reload_fraction;
        state.arm_weakness = p.pose.arm_weakness;
        state.root_drop = p.pose.root_drop;
        rigs.insert(
            p.id,
            evaluate_pose(
                state,
                p.body.anatomy.capacity(Region::LeftLeg),
                p.body.anatomy.capacity(Region::RightLeg),
            ),
        );
    }
    for (bone, mut t, mut mesh, mut visible) in &mut bones {
        let Some(p) = session.local(bone.owner) else {
            continue;
        };
        let a = &p.body.anatomy;
        let present = p.alive && a.missing & (1 << bone.segment.index()) == 0;
        let wound = a.wounds & (1 << bone.segment.index()) != 0;
        *visible = if present && (bone.inner == 0 || wound) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        *t = rigs[&p.id][bone.segment.index()].transform();
        if matches!(
            bone.segment,
            Segment::Thorax
                | Segment::LeftUpperArm
                | Segment::RightUpperArm
                | Segment::LeftForearm
                | Segment::RightForearm
                | Segment::LeftHand
                | Segment::RightHand
        ) {
            if let Some(r) = feedback.responses.get(&p.id) {
                t.translation.z += r.recoil;
                t.rotation *= Quat::from_rotation_x(r.recoil);
            }
        }
        mesh.0 = if bone.inner == 2 {
            let offset = t.rotation * Vec3::new(0.0, 0.0, -0.035);
            t.translation += offset;
            assets.cores[bone.segment.index()].clone()
        } else if bone.inner == 1 {
            assets.inside[bone.segment.index()].clone()
        } else if wound {
            assets.wounded[bone.segment.index()].clone()
        } else {
            assets.intact[bone.segment.index()].clone()
        };
    }
    for (gear, mut t, mut visible) in &mut gear {
        let Some(p) = session.local(gear.owner) else {
            continue;
        };
        let pose = rigs[&p.id];
        let equipped = if gear.helmet {
            p.armor.helmet > 0.0
        } else {
            p.armor.vest > 0.0
        };
        *visible = if p.alive && equipped {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        let s = if gear.helmet {
            Segment::Head
        } else {
            Segment::Thorax
        };
        *t = pose[s.index()].transform();
        let offset = t.rotation
            * if gear.helmet {
                Vec3::Y * 0.125
            } else {
                Vec3::new(0.0, 0.0, -0.17)
            };
        t.translation += offset;
        if !gear.helmet && p.armor.kind == ArmorKind::Heavy {
            t.scale = Vec3::new(1.15, 1.1, 1.2);
        }
    }
    for (organ, mut t, mut visible) in &mut organs {
        let Some(p) = session.local(organ.owner) else {
            continue;
        };
        let pose = rigs[&p.id][organ.segment.index()];
        let exposed = p.body.anatomy.wounds & (1 << organ.segment.index()) != 0;
        *visible =
            if p.alive && exposed && p.body.anatomy.missing & (1 << organ.segment.index()) == 0 {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        *t = pose.transform();
        t.scale = Vec3::splat(0.85 + 0.15 * p.body.anatomy.organs[organ.organ as usize] / 100.0);
        let offset = t.rotation * organ.offset;
        t.translation += offset;
    }
    for (weapon, mut t, mut mesh, mut visible) in &mut weapons {
        let Some(p) = session.local(weapon.owner) else {
            continue;
        };
        *visible = if p.alive && p.body.anatomy.capacity(Region::RightArm) > 0.0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        let pose = rigs[&p.id];
        *t = pose[Segment::RightHand.index()].transform();
        t.rotation = Quat::from_rotation_x(p.pose.aim_pitch);
        t.rotation *=
            Quat::from_rotation_z(-(p.pose.reload_fraction * std::f32::consts::PI).sin() * 0.30);
        let offset = t.rotation
            * Vec3::new(
                0.04,
                0.03,
                if p.weapon == WeaponKind::Pistol {
                    -0.26
                } else {
                    -0.07
                },
            );
        t.translation += offset;
        if let Some(r) = feedback.responses.get(&p.id) {
            t.translation.z += r.recoil;
        }
        mesh.0 = assets.weapons[p.weapon.index()].clone();
    }
    for (flash, mut t, mut visible) in &mut flashes {
        let Some(p) = session.local(flash.0) else {
            continue;
        };
        *visible = if p.alive && feedback.responses.get(&p.id).is_some_and(|r| r.flash > 0.0) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        t.translation = Vec3::Y
            * (PLAYER_VIEW_HEIGHT
                + if p.crouched {
                    PLAYER_CROUCH_VIEW_OFFSET
                } else {
                    0.0
                })
            + Quat::from_rotation_x(p.pose.aim_pitch) * Vec3::from(p.weapon.spec().barrel_offset);
    }
    for (detail, mut t, mut visible) in &mut details {
        let Some(p) = session.local(detail.owner) else {
            continue;
        };
        let present = p.alive
            && p.body.anatomy.missing & (1 << detail.segment.index()) == 0
            && (!detail.armor || p.armor.vest > 0.0);
        *visible = if present {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        *t = rigs[&p.id][detail.segment.index()].transform();
        let offset = t.rotation * detail.offset;
        t.translation += offset;
        t.scale = detail.scale;
    }
    for (stump, mut t, mut visible) in &mut stumps {
        let Some(p) = session.local(stump.owner) else {
            continue;
        };
        let missing = p.body.anatomy.missing;
        *visible = if p.alive
            && missing & (1 << stump.missing.index()) != 0
            && missing & (1 << stump.parent.index()) == 0
        {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        let pose = rigs[&p.id];
        let parent = pose[stump.parent.index()].transform();
        let child = pose[stump.missing.index()].transform();
        let local = (parent.rotation.inverse() * (child.translation - parent.translation))
            .normalize_or_zero();
        let half = stump.parent.half_size();
        let mut reach = f32::INFINITY;
        for i in 0..3 {
            if local[i].abs() > 0.001 {
                reach = reach.min(half[i] / local[i].abs());
            }
        }
        let normal = parent.rotation * local;
        *t = Transform::from_translation(parent.translation + normal * reach)
            .with_rotation(Quat::from_rotation_arc(Vec3::Y, normal))
            .with_scale(Vec3::new(
                stump.missing.half_size().x * 1.8,
                0.025,
                stump.missing.half_size().z * 1.8,
            ));
    }
}

fn sync_pieces(
    mut commands: Commands,
    time: Res<Time>,
    session: Res<ClientSession>,
    assets: Res<VoxelAssets>,
    mut visuals: Query<(Entity, &PieceVisual, &mut Transform)>,
) {
    for (entity, visual, mut t) in &mut visuals {
        if let Some(piece) = session.physics_pieces.iter().find(|p| p.id == visual.0) {
            let target = piece.pose.transform();
            let alpha = 1.0 - (-time.delta_secs() * 22.0).exp();
            t.translation = t.translation.lerp(target.translation, alpha);
            t.rotation = t.rotation.slerp(target.rotation, alpha);
        } else {
            commands.entity(entity).despawn();
        }
    }
    for p in &session.physics_pieces {
        if visuals.iter().any(|(_, v, _)| v.0 == p.id) {
            continue;
        }
        let owner = session.local(p.owner);
        let wounded = p.detached
            || owner.is_some_and(|o| o.body.anatomy.wounds & (1 << p.segment.index()) != 0);
        commands
            .spawn((
                PieceVisual(p.id),
                Mesh3d(if wounded {
                    assets.wounded[p.segment.index()].clone()
                } else {
                    assets.intact[p.segment.index()].clone()
                }),
                MeshMaterial3d(surface(&assets, p.segment, p.bot)),
                p.pose.transform(),
            ))
            .with_children(|c| {
                c.spawn((
                    Mesh3d(assets.inside[p.segment.index()].clone()),
                    MeshMaterial3d(assets.flesh.clone()),
                    Transform::default(),
                ));
                c.spawn((
                    Mesh3d(assets.cores[p.segment.index()].clone()),
                    MeshMaterial3d(assets.bone.clone()),
                    Transform::from_xyz(0.0, 0.0, -0.035),
                ));
                if p.segment == Segment::Head && owner.is_some_and(|o| o.armor.helmet > 0.0) {
                    c.spawn((
                        Mesh3d(assets.helmet.clone()),
                        MeshMaterial3d(assets.vest.clone()),
                        Transform::from_xyz(0.0, 0.125, 0.0),
                    ));
                }
                if p.segment == Segment::Thorax && owner.is_some_and(|o| o.armor.vest > 0.0) {
                    c.spawn((
                        Mesh3d(assets.plate.clone()),
                        MeshMaterial3d(assets.vest.clone()),
                        Transform::from_xyz(0.0, 0.0, -0.17),
                    ));
                }
                if let Some(owner) = owner {
                    for child in SEGMENTS.into_iter().filter(|s| {
                        s.parent() == Some(p.segment)
                            && owner.body.anatomy.missing & (1 << s.index()) != 0
                    }) {
                        let half = p.segment.half_size();
                        let offset = if p.segment == Segment::Thorax {
                            Vec3::new(
                                if child == Segment::LeftUpperArm {
                                    -half.x
                                } else {
                                    half.x
                                },
                                0.10,
                                0.0,
                            )
                        } else {
                            Vec3::new(0.0, -half.y, 0.0)
                        };
                        c.spawn((
                            Mesh3d(assets.cube.clone()),
                            MeshMaterial3d(assets.flesh.clone()),
                            Transform::from_translation(offset).with_scale(Vec3::new(
                                child.half_size().x * 1.8,
                                0.03,
                                child.half_size().z * 1.8,
                            )),
                        ));
                    }
                }
            });
    }
}

fn verify_native_replication(
    session: Res<ClientSession>,
    visuals: Query<&PieceVisual>,
    mut enabled: Local<Option<bool>>,
    mut previous: Local<Vec<u64>>,
) {
    if !*enabled.get_or_insert_with(|| std::env::var_os("TK_PLAYTEST_DIR").is_some()) {
        return;
    }
    let mut ids: Vec<_> = visuals.iter().map(|v| v.0).collect();
    ids.sort_unstable();
    let count = ids.len();
    ids.dedup();
    assert_eq!(count, ids.len(), "Duplicated physical render entity");
    let mut expected: Vec<_> = session.physics_pieces.iter().map(|p| p.id).collect();
    expected.sort_unstable();
    assert_eq!(
        ids, expected,
        "Physical presentation did not converge to the accepted snapshot"
    );
    if *previous != ids {
        info!(
            "PRESENTATION CONVERGED physical_entities={} unique_ids={:?}",
            count, ids
        );
        *previous = ids;
    }
}

pub(super) fn body() -> impl Bundle {
    (
        PlayerBodyVisual,
        Transform::default(),
        Visibility::Inherited,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn voxel_layers_are_real_detailed_meshes_and_wounds_remove_surface() {
        let healthy = voxel_mesh(Segment::Thorax, false, false);
        let wound = voxel_mesh(Segment::Thorax, true, false);
        assert!(healthy.count_vertices() > 1000);
        assert!(wound.count_vertices() < healthy.count_vertices());
        assert!(voxel_mesh(Segment::Head, true, true).count_vertices() > 24);
    }
}
