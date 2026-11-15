//! Reusable original voxel wardrobe and presentation of the common damage rig.
use super::PlayerBodyVisual;
use super::voxel::configured::{self, ConfiguredPose, ConfiguredWeapon};
use crate::{ClientSystems, sync::ClientSession};
use bevy::{platform::collections::HashMap, prelude::*};
use common::{combat::*, humanoid::*, *};

pub struct Plugin;
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct AnimateActors;
impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PoseCache>()
            .add_systems(Startup, assets)
            .add_systems(PostUpdate, verify_native_replication)
            .add_systems(
                Update,
                (
                    build_actors,
                    interpolate,
                    animate.in_set(AnimateActors),
                    sync_pieces,
                )
                    .chain()
                    .after(super::feedback::advance)
                    .in_set(ClientSystems::Present),
            )
            .add_systems(
                Update,
                apply_configured_contacts
                    .after(configured::PoseConfigured)
                    .after(sync_pieces)
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
pub(crate) struct PoseCache {
    accepted_rigs: HashMap<u64, [SegmentPose; SEGMENT_COUNT]>,
    raid: u32,
    poses: HashMap<u64, PoseState>,
    roots: HashMap<u64, Transform>,
    world_foot_anchors: HashMap<u64, [Vec3; 2]>,
    last_live: HashMap<(u64, Segment), Transform>,
}

impl PoseCache {
    pub(crate) fn presented_roll(&self, id: u64) -> Option<f32> {
        self.poses.get(&id).map(|pose| pose.aim_roll)
    }

    pub(crate) fn presented_support(&self, id: u64) -> Option<(f32, [f32; 2])> {
        self.poses
            .get(&id)
            .map(|pose| (pose.phase, pose.foot_weight))
    }

    pub(crate) fn presented_bone(&self, id: u64, segment: Segment) -> Option<Transform> {
        self.last_live.get(&(id, segment)).copied()
    }

    pub(crate) fn presented_foot_anchors(&self, id: u64) -> Option<[Vec3; 2]> {
        let root = self.roots.get(&id)?;
        let pose = self.poses.get(&id)?;
        Some(pose.feet.map(|foot| root.transform_point(foot.into())))
    }
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
    grasp: Handle<Mesh>,
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
    flash: Handle<StandardMaterial>,
    cube: Handle<Mesh>,
}

// Overextended gait targets keep their planted ankle; close the resulting
// knee seam on the live meshes only. Rig positions, contacts and colliders
// remain unchanged, and articulated bodies retain their original meshes.
fn gait_knee_scale(rig: &[SegmentPose; SEGMENT_COUNT], segment: Segment) -> f32 {
    use Segment::*;
    let (thigh, shin) = match segment {
        LeftThigh | LeftShin => (LeftThigh, LeftShin),
        RightThigh | RightShin => (RightThigh, RightShin),
        _ => return 1.0,
    };
    let a = Vec3::from(rig[thigh.index()].position);
    let b = Vec3::from(rig[shin.index()].position);
    let span = thigh.half_size().y + shin.half_size().y;
    ((a.distance(b) + 0.004) / span).clamp(1.0, 1.35)
}

fn interpolate_foot_anchor(
    anchor: &mut Vec3,
    confirmed: Vec3,
    root: &Transform,
    alpha: f32,
) -> Vec3 {
    *anchor = anchor.lerp(confirmed, alpha);
    root.rotation.inverse() * (*anchor - root.translation)
}

fn retain_configured_upper_pose(
    current: &mut [SegmentPose; SEGMENT_COUNT],
    previous: &[SegmentPose; SEGMENT_COUNT],
) {
    use Segment::*;
    // Configuration solving owns the upper-body posture and hand contacts,
    // not the locomotion of the pelvis or feet.
    current[Thorax.index()] = previous[Thorax.index()];
    for segment in [
        LeftUpperArm,
        LeftForearm,
        LeftHand,
        RightUpperArm,
        RightForearm,
        RightHand,
    ] {
        current[segment.index()] = previous[segment.index()];
    }
}

#[cfg(test)]
#[test]
fn rejected_upper_pose_keeps_the_current_gait() {
    use Segment::*;
    let prior = evaluate_pose(&PoseState::default(), 1.0, 1.0);
    let mut current = evaluate_pose(
        &PoseState {
            phase: 1.0,
            grounded: true,
            velocity: [0.0, 0.0, -5.5],
            ..default()
        },
        1.0,
        1.0,
    );
    let feet = [LeftFoot, RightFoot, Pelvis].map(|part| current[part.index()].position);
    assert_ne!(feet[0], prior[LeftFoot.index()].position);
    retain_configured_upper_pose(&mut current, &prior);
    assert_eq!(
        [LeftFoot, RightFoot, Pelvis].map(|part| current[part.index()].position),
        feet
    );
    assert_eq!(
        current[LeftHand.index()].position,
        prior[LeftHand.index()].position
    );
}

#[cfg(test)]
#[test]
fn planted_anchor_stays_fixed_as_the_remote_root_turns_and_translates() {
    let planted = Vec3::new(1.0, 0.15, 2.0);
    let mut anchor = planted;
    for tick in 0..32 {
        let t = tick as f32 / 31.0;
        let root = Transform::from_xyz(1.0 + t * 0.3, 1.0, 2.0 - t * 0.6)
            .with_rotation(Quat::from_rotation_y(t * 0.7));
        let local = interpolate_foot_anchor(&mut anchor, planted, &root, 0.22);
        assert!(root.transform_point(local).distance(planted) < 0.00001);
    }
}

#[cfg(test)]
#[test]
fn gait_knee_scale_closes_overextended_meshes_without_moving_contacts() {
    let state = PoseState {
        grounded: true,
        velocity: [0.0, 0.0, -5.5],
        feet: [[-0.13, -0.845, 0.49], [0.13, -0.845, -0.06]],
        foot_weight: [0.98, 0.02],
        phase: 2.61,
        ..default()
    };
    let rig = evaluate_pose(&state, 1.0, 1.0);
    let (thigh, shin) = (Segment::LeftThigh, Segment::LeftShin);
    let end = |segment: Segment, sign: f32, scale: f32| {
        rig[segment.index()]
            .transform()
            .transform_point(Vec3::Y * sign * segment.half_size().y * scale)
    };
    assert!(end(thigh, -1.0, 1.0).distance(end(shin, 1.0, 1.0)) > 0.045);
    let a = end(thigh, -1.0, gait_knee_scale(&rig, thigh));
    let b = end(shin, 1.0, gait_knee_scale(&rig, shin));
    assert!(a.distance(b) < 0.005);
    assert_eq!(gait_knee_scale(&rig, Segment::LeftFoot), 1.0);
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
        grasp: meshes.add(super::weapons::grasp::mesh()),
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
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        flash: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.7, 0.1),
            emissive: LinearRgba::rgb(12.0, 5.0, 0.3),
            unlit: true,
            ..default()
        }),
    });
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
                let mut surface = c.spawn((
                    BoneVisual {
                        owner: client.id,
                        segment,
                        inner: 0,
                    },
                    Mesh3d(assets.intact[segment.index()].clone()),
                    MeshMaterial3d(surface(&assets, segment, bot)),
                    Transform::default(),
                ));
                if segment == Segment::LeftHand {
                    surface.insert((
                        super::weapons::grasp::component([0.0; 2]),
                        super::weapons::grasp::bounds(),
                    ));
                }
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
                Transform::default(),
                Visibility::Hidden,
            ));
            c.spawn((
                WorldFlash(client.id),
                Mesh3d(assets.cube.clone()),
                MeshMaterial3d(assets.flash.clone()),
                Transform::from_scale(Vec3::new(0.075, 0.075, 0.12)),
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

fn interpolate(
    time: Res<Time>,
    mut cache: ResMut<PoseCache>,
    mut actors: Query<(&mut Transform, &mut RemoteTarget, &Client)>,
) {
    let alpha = 1.0 - (-time.delta_secs() * 18.0).exp();
    for (mut t, mut target, client) in &mut actors {
        let snap = target.snap || t.translation.distance(target.position) > 4.0;
        if snap {
            cache.world_foot_anchors.remove(&client.id);
        }
        t.translation = if snap {
            target.position
        } else {
            t.translation.lerp(target.position, alpha)
        };
        t.rotation = t
            .rotation
            .slerp(target.rotation, if snap { 1.0 } else { alpha });
        target.snap = false;
        cache.roots.insert(client.id, *t);
    }
}

#[derive(bevy::ecs::system::SystemParam)]
struct ActorVisuals<'w, 's> {
    bones: Query<
        'w,
        's,
        (
            &'static BoneVisual,
            &'static mut Transform,
            &'static mut Mesh3d,
            &'static mut Visibility,
            Option<&'static mut bevy::mesh::morph::MeshMorphWeights>,
        ),
        (
            Without<EquipmentVisual>,
            Without<OrganVisual>,
            Without<WorldWeapon>,
            Without<WorldFlash>,
            Without<DetailVisual>,
        ),
    >,
    gear: Query<
        'w,
        's,
        (
            &'static EquipmentVisual,
            &'static mut Transform,
            &'static mut Visibility,
        ),
        (
            Without<BoneVisual>,
            Without<OrganVisual>,
            Without<WorldWeapon>,
            Without<WorldFlash>,
            Without<DetailVisual>,
        ),
    >,
    organs: Query<
        'w,
        's,
        (
            &'static OrganVisual,
            &'static mut Transform,
            &'static mut Visibility,
        ),
        (
            Without<BoneVisual>,
            Without<EquipmentVisual>,
            Without<WorldWeapon>,
            Without<WorldFlash>,
            Without<DetailVisual>,
        ),
    >,
    weapons: Query<
        'w,
        's,
        (
            Entity,
            &'static WorldWeapon,
            &'static mut Visibility,
            Option<&'static ConfiguredWeapon>,
        ),
        (
            Without<BoneVisual>,
            Without<EquipmentVisual>,
            Without<OrganVisual>,
            Without<WorldFlash>,
            Without<DetailVisual>,
        ),
    >,
    flashes: Query<
        'w,
        's,
        (
            &'static WorldFlash,
            &'static mut Transform,
            &'static mut Visibility,
        ),
        (
            Without<BoneVisual>,
            Without<EquipmentVisual>,
            Without<OrganVisual>,
            Without<WorldWeapon>,
            Without<DetailVisual>,
        ),
    >,
    details: Query<
        'w,
        's,
        (
            &'static DetailVisual,
            &'static mut Transform,
            &'static mut Visibility,
        ),
        (
            Without<BoneVisual>,
            Without<EquipmentVisual>,
            Without<OrganVisual>,
            Without<WorldWeapon>,
            Without<WorldFlash>,
        ),
    >,
    stumps: Query<
        'w,
        's,
        (
            &'static StumpVisual,
            &'static mut Transform,
            &'static mut Visibility,
        ),
        (
            Without<BoneVisual>,
            Without<EquipmentVisual>,
            Without<OrganVisual>,
            Without<WorldWeapon>,
            Without<WorldFlash>,
            Without<DetailVisual>,
        ),
    >,
}

fn apply_configured_contacts(
    session: Res<ClientSession>,
    mut cache: ResMut<PoseCache>,
    weapons: Query<(&WorldWeapon, &ConfiguredPose, &Visibility)>,
    mut bones: Query<(&BoneVisual, &mut Transform), Without<WorldFlash>>,
    mut flashes: Query<
        (&WorldFlash, &mut Transform, &mut Visibility),
        (Without<BoneVisual>, Without<WorldWeapon>),
    >,
) {
    let mut arms = HashMap::new();
    let mut roots = HashMap::new();
    for (weapon, pose, visible) in &weapons {
        if *visible == Visibility::Hidden {
            continue;
        }
        let Some(player) = session.local(weapon.owner) else {
            continue;
        };
        let root = cache.roots.get(&player.id).copied().unwrap_or_else(|| {
            Transform::from_translation(player.pos.into())
                .with_rotation(Quat::from_rotation_y(player.rot.yaw))
        });
        roots.insert(player.id, pose.muzzle);
        for (arm, shoulder, segments) in [
            (
                pose.solution.support,
                pose.shoulders[0],
                [
                    Segment::LeftUpperArm,
                    Segment::LeftForearm,
                    Segment::LeftHand,
                ],
            ),
            (
                pose.solution.trigger,
                pose.shoulders[1],
                [
                    Segment::RightUpperArm,
                    Segment::RightForearm,
                    Segment::RightHand,
                ],
            ),
        ] {
            for (segment, pose) in segments.into_iter().zip(arm.segments(shoulder)) {
                if player.body.anatomy.missing & (1 << segment.index()) == 0 {
                    let transform = pose.transform();
                    arms.insert((player.id, segment), transform);
                    cache
                        .last_live
                        .insert((player.id, segment), root.mul_transform(transform));
                }
            }
        }
    }
    for (bone, mut transform) in &mut bones {
        if let Some(pose) = arms.get(&(bone.owner, bone.segment)) {
            *transform = *pose;
            if bone.inner == 2 {
                let offset = transform.rotation * Vec3::new(0.0, 0.0, -0.035);
                transform.translation += offset;
            }
        }
    }
    for (flash, mut transform, mut visible) in &mut flashes {
        if let Some(muzzle) = roots.get(&flash.0) {
            *transform = muzzle.mul_transform(Transform::from_translation(Vec3::NEG_Z * 0.05));
        } else {
            *visible = Visibility::Hidden;
        }
    }
}

fn animate(
    mut commands: Commands,
    session: Res<ClientSession>,
    assets: Res<VoxelAssets>,
    feedback: Res<super::feedback::Feedback>,
    time: Res<Time<Real>>,
    mut cache: ResMut<PoseCache>,
    visuals: ActorVisuals,
) {
    let ActorVisuals {
        mut bones,
        mut gear,
        mut organs,
        mut weapons,
        mut flashes,
        mut details,
        mut stumps,
    } = visuals;
    if cache.raid != session.raid.raid_id {
        cache.poses.clear();
        cache.roots.clear();
        cache.world_foot_anchors.clear();
        cache.last_live.clear();
        cache.accepted_rigs.clear();
        cache.raid = session.raid.raid_id;
    }
    let age = (time.elapsed_secs_f64() - session.last_received).clamp(0.0, 0.065) as f32;
    let alpha = 1.0 - (-time.delta_secs() * 24.0).exp();
    let mut rigs = HashMap::new();
    let mut postures = HashMap::new();
    let mut posture_errors = HashMap::new();
    let cache = &mut *cache;
    for p in &session.players {
        let confirmed_root = Transform::from_translation(p.pos.into())
            .with_rotation(Quat::from_rotation_y(p.rot.yaw));
        let root = cache.roots.get(&p.id).copied().unwrap_or(confirmed_root);
        let state = cache.poses.entry(p.id).or_insert_with(|| p.pose.clone());
        state.crouch += (p.pose.crouch - state.crouch) * alpha;
        state.velocity = Vec3::from(state.velocity)
            .lerp(p.pose.velocity.into(), alpha)
            .into();
        let speed = Vec2::new(p.pose.velocity[0], p.pose.velocity[2]).length();
        let phase = p.pose.phase
            + if p.pose.grounded {
                age * stride_phase_rate(speed, p.pose.crouch)
            } else {
                0.0
            };
        let delta = phase - state.phase;
        state.phase += delta.sin().atan2(delta.cos()) * alpha;
        state.grounded = p.pose.grounded;
        state.mode = p.pose.mode;
        state.mode_time = p.pose.mode_time + age;
        state.aim_pitch += (p.pose.aim_pitch - state.aim_pitch) * alpha;
        state.aim_roll += (p.pose.aim_roll - state.aim_roll) * alpha;
        state.ads += (p.pose.ads - state.ads) * alpha;
        for i in 0..2 {
            state.lean[i] += (p.pose.lean[i] - state.lean[i]) * alpha;
        }
        state.landing = p.pose.landing;
        state.weapon = feedback
            .responses
            .get(&p.id)
            .and_then(|r| r.displayed)
            .unwrap_or(p.weapon);
        state.reload_fraction = p.pose.reload_fraction;
        state.reload_empty = p.pose.reload_empty;
        state.arm_weakness = p.pose.arm_weakness;
        state.root_drop = p.pose.root_drop;
        state.fall_direction = p.pose.fall_direction;
        state.fall_phase = p.pose.fall_phase;
        state.fall_velocity = p.pose.fall_velocity;
        state.physical = p.pose.physical.clone();
        state.acceleration = p.pose.acceleration;
        state.turn_rate += (p.pose.turn_rate - state.turn_rate) * alpha;
        let foot_alpha = 1.0 - (-time.delta_secs() * 18.0).exp();
        let confirmed = p
            .pose
            .feet
            .map(|foot| confirmed_root.transform_point(foot.into()));
        let anchors = cache.world_foot_anchors.entry(p.id).or_insert(confirmed);
        if p.pose.physical.is_some() || !p.pose.grounded || !p.alive {
            *anchors = confirmed;
        }
        for i in 0..2 {
            // Filter the contact independently of root motion and yaw.
            state.feet[i] =
                interpolate_foot_anchor(&mut anchors[i], confirmed[i], &root, foot_alpha).into();
            state.foot_weight[i] += (p.pose.foot_weight[i] - state.foot_weight[i]) * foot_alpha;
            state.foot_normal[i] = p.pose.foot_normal[i];
            let delta = p.pose.foot_yaw[i] - state.foot_yaw[i];
            state.foot_yaw[i] += delta.sin().atan2(delta.cos()) * foot_alpha;
        }
        let mut rig = evaluate_pose_with_action(
            state,
            p.body.anatomy.capacity(Region::LeftLeg),
            p.body.anatomy.capacity(Region::RightLeg),
            feedback.responses.get(&p.id).and_then(|r| r.action),
        );
        if state.weapon == p.weapon
            && let Some(entry) = session
                .configurations
                .cache
                .get(session.configurations.cache.epoch(), p.configuration)
        {
            match common::humanoid::configured::evaluate(
                state,
                [
                    p.body.anatomy.capacity(Region::LeftLeg),
                    p.body.anatomy.capacity(Region::RightLeg),
                ],
                entry
                    .visual(entry.presented_magazine(&p.mechanics))
                    .unwrap_or(&entry.resolved),
                &p.mechanics,
                feedback.responses.get(&p.id).and_then(|r| r.action),
                Some(common::humanoid::configured::shoulder_cover(&p.armor)),
            ) {
                Ok(configured) => {
                    rig = configured.segments;
                    if let Some(posture) = configured.posture {
                        postures.insert(p.id, posture);
                        cache.accepted_rigs.insert(p.id, rig);
                    }
                }
                Err(error) => {
                    posture_errors.insert(p.id, format!("configured posture: {error:?}"));
                    if let Some(previous) = cache.accepted_rigs.get(&p.id) {
                        retain_configured_upper_pose(&mut rig, previous);
                    }
                }
            }
        }
        if p.alive && p.pose.physical.is_none() {
            for segment in SEGMENTS {
                if p.body.anatomy.missing & (1 << segment.index()) == 0 {
                    cache.last_live.insert(
                        (p.id, segment),
                        root.mul_transform(rig[segment.index()].transform()),
                    );
                }
            }
        }
        rigs.insert(p.id, rig);
    }
    for (bone, mut t, mut mesh, mut visible, fingers) in &mut bones {
        let Some(p) = session.local(bone.owner) else {
            continue;
        };
        let a = &p.body.anatomy;
        let present =
            p.alive && p.pose.physical.is_none() && a.missing & (1 << bone.segment.index()) == 0;
        let wound = a.wounds & (1 << bone.segment.index()) != 0;
        *visible = if present && (bone.inner == 0 || wound) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        *t = rigs[&p.id][bone.segment.index()].transform();
        if present
            && p.pose.grounded
            && matches!(
                p.pose.mode,
                PhysicalMode::Standing | PhysicalMode::Stumbling
            )
        {
            t.scale.y = gait_knee_scale(&rigs[&p.id], bone.segment);
        }
        if matches!(
            bone.segment,
            Segment::Thorax
                | Segment::LeftUpperArm
                | Segment::RightUpperArm
                | Segment::LeftForearm
                | Segment::RightForearm
                | Segment::LeftHand
                | Segment::RightHand
        ) && let Some(r) = feedback.responses.get(&p.id)
        {
            t.translation.z += r.recoil;
            if bone.segment != Segment::Thorax {
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
        if let Some(mut fingers) = fingers {
            let action = feedback
                .responses
                .get(&p.id)
                .filter(|r| r.displayed == Some(WeaponKind::Rifle) && !wound && present)
                .and_then(|r| r.action);
            let weights = if let Some(action) = action {
                mesh.0 = assets.grasp.clone();
                super::weapons::grasp::weights(&action)
            } else {
                [0.0; 2]
            };
            *fingers = super::weapons::grasp::component(weights);
        }
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
        *visible = if p.alive && p.pose.physical.is_none() && equipped {
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
        if !gear.helmet {
            t.translation.z += feedback.responses.get(&p.id).map_or(0.0, |r| r.recoil);
        }
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
        *visible = if p.alive
            && p.pose.physical.is_none()
            && exposed
            && p.body.anatomy.missing & (1 << organ.segment.index()) == 0
        {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        *t = pose.transform();
        t.scale = Vec3::splat(0.85 + 0.15 * p.body.anatomy.organs[organ.organ as usize] / 100.0);
        let offset = t.rotation * organ.offset;
        t.translation += offset;
    }
    for (entity, weapon, mut visible, previous) in &mut weapons {
        let Some(p) = session.local(weapon.owner) else {
            continue;
        };
        let usable = p.alive
            && p.pose.physical.is_none()
            && matches!(
                p.pose.mode,
                PhysicalMode::Standing | PhysicalMode::Stumbling
            )
            && p.body.anatomy.capacity(Region::RightArm) > 0.0;
        *visible = if usable {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if !usable {
            continue;
        }
        let pose = rigs[&p.id];
        let mut preferred = posed_weapon(&cache.poses[&p.id], &pose);
        let displayed = cache.poses[&p.id].weapon;
        let action = feedback
            .responses
            .get(&p.id)
            .and_then(|r| r.action)
            .unwrap_or_else(|| {
                common::weapon_animation::manipulation::action(displayed, &p.mechanics)
            });
        let mut request = if displayed == p.weapon {
            ConfiguredWeapon {
                epoch: session.configurations.cache.epoch(),
                handle: p.configuration,
                view_model: false,
                magazine: None,
            }
        } else if let Some(previous) = previous {
            *previous
        } else {
            continue;
        };
        let Some(entry) = session
            .configurations
            .cache
            .get(request.epoch, request.handle)
        else {
            *visible = Visibility::Hidden;
            continue;
        };
        let mechanics = feedback
            .responses
            .get(&p.id)
            .map_or(p.mechanics, |r| r.displayed_mechanics);
        request.magazine = entry.presented_magazine(&mechanics);
        let Some(visual) = entry.visual(request.magazine) else {
            continue;
        };
        if let Ok(contacts) = handling::WeaponContacts::from_assembly(&visual.assembly) {
            preferred = common::humanoid::configured::preferred_root(
                &cache.poses[&p.id],
                pose[Segment::Thorax.index()].position.into(),
                contacts.trigger,
                action,
            );
        }
        let recoil = feedback.responses.get(&p.id).map_or(0.0, |r| r.recoil);
        preferred.translation.z += recoil;
        let mut input = configured::native::action(visual, action, recoil, &mechanics);
        input.preflight_error = posture_errors.get(&p.id).cloned();
        input.preferred_root = preferred;
        input.shoulders = [Segment::LeftUpperArm, Segment::RightUpperArm].map(|segment| {
            pose[segment.index()]
                .transform()
                .transform_point(Vec3::Y * 0.15)
                + Vec3::Z * recoil
        });
        if let Some(posture) = postures.get(&p.id) {
            input.preferred_root = posture.preferred_root;
            input.preferred_root.translation.z += recoil;
            input.shoulders = posture.shoulders.map(|p| p + Vec3::Z * recoil);
            input.poles = posture.poles;
            input.stock_target = posture.stock_target.map(|p| p + Vec3::Z * recoil);
            input.max_adjustment = posture.max_adjustment;
        }
        // Existing anatomical meshes (including wounds/cores) consume the same
        // accepted solution below, rather than drawing a second pair of arms.
        input.visible_hands = [false; 6];
        commands.entity(entity).insert((request, input));
    }
    for (flash, mut t, mut visible) in &mut flashes {
        let Some(p) = session.local(flash.0) else {
            continue;
        };
        *visible = if p.alive
            && cache.poses[&p.id].weapon == p.weapon
            && feedback.responses.get(&p.id).is_some_and(|r| r.flash > 0.0)
        {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        let gun = posed_weapon(&cache.poses[&p.id], &rigs[&p.id]);
        t.translation = gun.transform_point(
            common::weapon_animation::WeaponRig::for_weapon(cache.poses[&p.id].weapon).muzzle
                - Vec3::Z * 0.05,
        );
        t.rotation = gun.rotation;
        if let Some(r) = feedback.responses.get(&p.id) {
            t.translation.z += r.recoil;
        }
    }
    for (detail, mut t, mut visible) in &mut details {
        let Some(p) = session.local(detail.owner) else {
            continue;
        };
        let present = p.alive
            && p.pose.physical.is_none()
            && p.body.anatomy.missing & (1 << detail.segment.index()) == 0
            && (!detail.armor || p.armor.vest > 0.0);
        *visible = if present {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        *t = rigs[&p.id][detail.segment.index()].transform();
        if detail.segment == Segment::Thorax {
            t.translation.z += feedback.responses.get(&p.id).map_or(0.0, |r| r.recoil);
        }
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
    cache: Res<PoseCache>,
    id: Res<PlayerId>,
    mut visuals: Query<(Entity, &PieceVisual, &mut Transform, &mut Visibility)>,
) {
    let piece_visibility = |piece: &PhysicsPieceData| {
        physical_visibility(piece, id.0, session.local(id.0).is_some_and(|p| p.alive))
    };
    for (entity, visual, mut t, mut visible) in &mut visuals {
        if let Some(piece) = session.physics_pieces.iter().find(|p| p.id == visual.0) {
            let target = piece.pose.transform();
            let alpha = 1.0 - (-time.delta_secs() * 22.0).exp();
            t.translation = t.translation.lerp(target.translation, alpha);
            t.rotation = t.rotation.slerp(target.rotation, alpha);
            *visible = piece_visibility(piece);
        } else {
            commands.entity(entity).despawn();
        }
    }
    for p in &session.physics_pieces {
        if visuals.iter().any(|(_, v, _, _)| v.0 == p.id) {
            continue;
        }
        let owner = session.local(p.owner);
        let wounded = p.detached
            || owner.is_some_and(|o| o.body.anatomy.wounds & (1 << p.segment.index()) != 0);
        commands
            .spawn((
                PieceVisual(p.id),
                piece_visibility(p),
                Mesh3d(if wounded {
                    assets.wounded[p.segment.index()].clone()
                } else {
                    assets.intact[p.segment.index()].clone()
                }),
                MeshMaterial3d(surface(&assets, p.segment, p.bot)),
                cache
                    .last_live
                    .get(&(p.owner, p.segment))
                    .copied()
                    .unwrap_or_else(|| p.pose.transform()),
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

fn physical_visibility(piece: &PhysicsPieceData, local: u64, alive: bool) -> Visibility {
    // The living physical camera follows the head while the world meshes lag
    // snapshots for interpolation. Do not let the player's own torso/arms sweep
    // through that camera during get-up. Detached fragments and remote bodies
    // remain visible; death can still be observed from the normal death view.
    if piece.owner == local
        && (matches!(piece.segment, Segment::Head | Segment::Neck) || alive && !piece.detached)
    {
        Visibility::Hidden
    } else {
        Visibility::Inherited
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
    fn live_local_physical_meshes_do_not_cross_the_head_camera_but_remote_and_detached_parts_remain()
     {
        let mut piece = PhysicsPieceData {
            id: 1,
            owner: 7,
            segment: Segment::Thorax,
            pose: SegmentPose::default(),
            detached: false,
            bot: false,
        };
        assert_eq!(physical_visibility(&piece, 7, true), Visibility::Hidden);
        assert_eq!(physical_visibility(&piece, 8, true), Visibility::Inherited);
        assert_eq!(physical_visibility(&piece, 7, false), Visibility::Inherited);
        piece.detached = true;
        assert_eq!(physical_visibility(&piece, 7, true), Visibility::Inherited);
        piece.segment = Segment::Head;
        assert_eq!(physical_visibility(&piece, 7, true), Visibility::Hidden);
        assert_eq!(physical_visibility(&piece, 8, true), Visibility::Inherited);
    }
    #[test]
    fn voxel_layers_are_real_detailed_meshes_and_wounds_remove_surface() {
        let healthy = voxel_mesh(Segment::Thorax, false, false);
        let wound = voxel_mesh(Segment::Thorax, true, false);
        assert!(healthy.count_vertices() > 1000);
        assert!(wound.count_vertices() < healthy.count_vertices());
        assert!(voxel_mesh(Segment::Head, true, true).count_vertices() > 24);
    }
}

#[cfg(test)]
mod configured_tests;
