//! Confirmed shots only. No audio assets, players or sinks on this path.
use super::{LocalView, ViewModelCamera, WeaponMeshRoot, WeaponViewModel, WorldModelCamera};
use crate::{ClientSystems, sync::ClientSession};
use bevy::{platform::collections::HashMap, prelude::*};
use common::weapon_animation::{WeaponAction, WeaponRig, reload_action};
use common::{humanoid::PhysicalMode, *};

pub struct Plugin;
impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Feedback>().add_systems(
            Update,
            (advance, animate_local)
                .chain()
                .after(super::sync_local_view)
                .in_set(ClientSystems::Present),
        );
    }
}
#[derive(Default)]
pub(super) struct Response {
    pub recoil: f32,
    velocity: f32,
    pub flash: f32,
    last: Option<u64>,
    weapon: Option<WeaponKind>,
    swap: f32,
    pub(super) displayed: Option<WeaponKind>,
    pub(super) action: Option<WeaponAction>,
    sway: Vec2,
    previous_look: Option<(Vec2, f32)>,
    look_rate: Vec2,
    travel: Vec3,
    motion_phase: f32,
    landing: f32,
    pub(super) ads: f32,
    was_alive: bool,
}
impl Response {
    fn step(&mut self, dt: f32) {
        self.velocity += (-self.recoil * 180.0 - self.velocity * 23.0) * dt;
        self.recoil = (self.recoil + self.velocity * dt).clamp(0.0, 0.11);
        self.flash = (self.flash - dt).max(0.0);
        self.swap = (self.swap - dt * 4.0).max(0.0);
        if self.swap <= 0.5 {
            self.displayed = self.weapon;
        }
    }
    fn shot(&mut self, shot: &FiredProjectileData) {
        if self.last.is_some_and(|id| id >= shot.id) {
            return;
        }
        self.last = Some(shot.id);
        self.flash = 0.055;
        self.velocity = (self.velocity
            + match shot.weapon {
                WeaponKind::Rifle => 1.45,
                WeaponKind::Smg => 0.9,
                WeaponKind::Pistol => 1.9,
            })
        .min(2.5);
        self.recoil = (self.recoil + 0.012).min(0.11);
    }
}
#[derive(Default, Resource)]
pub(super) struct Feedback {
    pub responses: HashMap<u64, Response>,
    raid: u32,
}
#[derive(Component)]
pub(crate) struct Flash;
#[derive(Component)]
pub(super) struct Bolt;
#[derive(Component)]
pub(super) struct Magazine;
#[derive(Component)]
pub(super) struct SupportHand;
#[derive(Component)]
pub(super) struct Control;
#[derive(Component)]
pub(super) struct Forearm {
    pub support: bool,
}
#[derive(Component)]
pub(super) struct UpperArm {
    pub support: bool,
}

fn forearm_pose(wrist: Vec3, elbow: Vec3) -> Transform {
    let length = wrist.distance(elbow);
    let direction = if length > 0.000001 {
        (wrist - elbow) / length
    } else {
        Vec3::Y
    };
    Transform::from_translation((wrist + elbow) * 0.5)
        .with_rotation(Quat::from_rotation_arc(Vec3::Y, direction))
        .with_scale(Vec3::new(1.0, length / 0.28, 1.0))
}

fn body_side_elbow(wrist: Vec3, body_anchor: Vec3) -> Vec3 {
    // The upper arm is outside this view model. Do not stretch a 28cm forearm
    // all the way to the torso anchor when reaching up to a slide or HK handle.
    wrist + (body_anchor - wrist).clamp_length_max(0.29)
}

fn locomotion_offset(phase: f32, speed: f32, ads: f32) -> Vec3 {
    Vec3::new(phase.sin() * 0.0035, (phase * 2.0).cos() * 0.0015, 0.0)
        * speed.clamp(0.0, 5.5)
        * (1.0 - ads.clamp(0.0, 1.0) * 0.92)
}

pub(super) fn advance(
    time: Res<Time>,
    session: Res<ClientSession>,
    mut feedback: ResMut<Feedback>,
    local_input: Option<Res<LocalInput>>,
    local_id: Option<Res<PlayerId>>,
) {
    if feedback.raid != session.raid.raid_id {
        feedback.responses.clear();
        feedback.raid = session.raid.raid_id;
    }
    for response in feedback.responses.values_mut() {
        response.step(time.delta_secs().min(1.0 / 30.0));
    }
    for player in &session.players {
        let response = feedback.responses.entry(player.id).or_default();
        if response.weapon != Some(player.weapon) {
            response.swap = 1.0;
            if response.displayed.is_none() {
                response.displayed = Some(player.weapon);
            }
            response.weapon = Some(player.weapon);
            response.flash = 0.0;
        }
        if player.alive && !response.was_alive {
            response.swap = 0.5;
            response.displayed = Some(player.weapon);
        }
        response.was_alive = player.alive;
        if !player.alive {
            response.flash = 0.0;
            response.recoil = 0.0;
            response.velocity = 0.0;
        }
        let w = response.displayed.unwrap_or(player.weapon);
        let fraction = if player.reload_remaining > 0.0 && w == player.weapon {
            1.0 - player.reload_remaining / player.weapon.spec().reload_seconds
        } else {
            0.0
        };
        let target = reload_action(w, fraction, player.ammo_in_mag == 0);
        let alpha = 1.0 - (-time.delta_secs().min(0.05) * 32.0).exp();
        let action = response.action.get_or_insert(target);
        action.root.translation += ((target.root.translation - action.root.translation) * alpha)
            .clamp_length_max(1.2 * time.delta_secs().min(0.05));
        action.root.rotation = action.root.rotation.slerp(target.root.rotation, alpha);
        action.support.translation = action
            .support
            .translation
            .lerp(target.support.translation, alpha);
        action.support.rotation = action
            .support
            .rotation
            .slerp(target.support.rotation, alpha);
        action.magazine.translation = action
            .magazine
            .translation
            .lerp(target.magazine.translation, alpha);
        action.magazine.rotation = action
            .magazine
            .rotation
            .slerp(target.magazine.rotation, alpha);
        action.magazine_visible = target.magazine_visible;
        action.bolt += (target.bolt - action.bolt) * alpha;
        action.control += (target.control - action.control) * alpha;
        action.latch += (target.latch - action.latch) * alpha;
        let local = local_id.as_ref().is_some_and(|id| id.0 == player.id);
        let camera = if local {
            local_input
                .as_ref()
                .map_or(player.rot, |input| input.camera)
        } else {
            player.rot
        };
        let look = Vec2::new(camera.yaw, camera.pitch);
        let stamp = if local {
            time.elapsed_secs()
        } else {
            session.simulation_time
        };
        if let Some((old, previous_stamp)) = response.previous_look {
            if stamp > previous_stamp {
                let d = look - old;
                response.look_rate = Vec2::new(d.x.sin().atan2(d.x.cos()), d.y)
                    / (stamp - previous_stamp).max(0.001);
                response.previous_look = Some((look, stamp));
            }
        } else {
            response.previous_look = Some((look, stamp));
        }
        response.sway = response.sway.lerp(
            (response.look_rate * -0.006).clamp(Vec2::splat(-0.035), Vec2::splat(0.035)),
            1.0 - (-time.delta_secs() * 7.0).exp(),
        );
        response.travel = response.travel.lerp(
            player.pose.velocity.into(),
            1.0 - (-time.delta_secs() * 6.0).exp(),
        );
        let speed = Vec2::new(response.travel.x, response.travel.z).length();
        let rate = common::humanoid::stride_phase_rate(speed, player.pose.crouch);
        response.motion_phase =
            (response.motion_phase + rate * time.delta_secs()) % std::f32::consts::TAU;
        response.landing +=
            (player.pose.landing - response.landing) * (1.0 - (-time.delta_secs() * 8.0).exp());
        let requested = if local {
            if local_input
                .as_ref()
                .is_some_and(|i| i.aim && i.weapon == player.weapon)
            {
                1.0
            } else {
                0.0
            }
        } else {
            player.pose.ads
        };
        let usable = player.alive
            && player.pose.physical.is_none()
            && matches!(
                player.pose.mode,
                PhysicalMode::Standing | PhysicalMode::Stumbling
            )
            && player.reload_remaining <= 0.0
            && response.swap <= 0.05
            && player
                .body
                .anatomy
                .capacity(common::combat::Region::RightArm)
                > 0.0
            && !(local_input.as_ref().is_some_and(|i| local && i.run) && speed > 0.5);
        let target = if usable { requested } else { 0.0 };
        response.ads += (target - response.ads) * (1.0 - (-time.delta_secs() * 14.0).exp());
    }
    for shot in &session.shots {
        let response = feedback.responses.entry(shot.shooter).or_default();
        if session
            .local(shot.shooter)
            .is_some_and(|p| p.alive && p.weapon == shot.weapon)
        {
            response.shot(shot);
        } else if response.last.is_none_or(|id| shot.id > id) {
            response.last = Some(shot.id);
        }
    }
}

pub(super) fn animate_local(
    time: Res<Time>,
    id: Res<PlayerId>,
    session: Res<ClientSession>,
    feedback: Res<Feedback>,
    input: Res<LocalInput>,
    mut views: Query<(&WeaponViewModel, &mut Transform), With<LocalView>>,
    mut parts: Query<
        (
            &ChildOf,
            Option<&Flash>,
            Option<&Bolt>,
            Option<&Magazine>,
            Option<&SupportHand>,
            Option<&Control>,
            Option<&Forearm>,
            Option<&UpperArm>,
            &mut Transform,
            &mut Visibility,
        ),
        (
            Without<LocalView>,
            Without<WorldModelCamera>,
            Without<ViewModelCamera>,
        ),
    >,
    roots: Query<&ChildOf, With<WeaponMeshRoot>>,
    weapons: Query<&WeaponViewModel>,
    projection: Single<&Projection, With<ViewModelCamera>>,
    mut cameras: Query<
        &mut Transform,
        (
            Or<(With<WorldModelCamera>, With<ViewModelCamera>)>,
            Without<WeaponViewModel>,
        ),
    >,
) {
    let Some(p) = session.local(id.0) else {
        return;
    };
    let Some(response) = feedback.responses.get(&id.0) else {
        return;
    };
    let speed = Vec2::new(response.travel.x, response.travel.z).length();
    let phase = response.motion_phase;
    let motion_weight = 1.0 - response.ads * 0.92;
    let displayed = response.displayed.unwrap_or(p.weapon);
    let action = response
        .action
        .unwrap_or_else(|| reload_action(displayed, 0.0, false));
    let swap_drop = if response.swap > 0.5 {
        (1.0 - response.swap) * 2.0
    } else {
        response.swap * 2.0
    };
    let physical_drop = if matches!(p.pose.mode, PhysicalMode::Fallen | PhysicalMode::Recovering) {
        if p.pose.mode == PhysicalMode::Recovering {
            1.0 - common::weapon_animation::ease((p.pose.mode_time - 0.75) / 0.25)
        } else {
            common::weapon_animation::ease(p.pose.mode_time / 0.2)
        }
    } else {
        0.0
    };
    for (weapon, mut t) in &mut views {
        if weapon.weapon != displayed {
            continue;
        }
        t.translation += locomotion_offset(phase, speed, response.ads)
            + action.root.translation
            + Vec3::new(response.sway.x * 0.2, -swap_drop * 0.34, response.recoil);
        t.translation +=
            (Vec3::from(p.pose.velocity) - response.travel) * Vec3::new(-0.003, -0.002, -0.004);
        t.translation.y -= response.landing * 0.025 * motion_weight;
        t.rotation *=
            Quat::from_rotation_x((response.travel.y * 0.008).clamp(-0.05, 0.05) * motion_weight);
        let injury = (1.0 - p.body.anatomy.handling()) * (time.elapsed_secs() * 3.7).sin() * 0.035;
        t.rotation *= Quat::from_euler(
            EulerRot::XYZ,
            response.recoil * 1.8 + response.sway.y * motion_weight + swap_drop * 0.6,
            response.sway.x * motion_weight,
            (injury - response.sway.x * 0.5) * motion_weight,
        );
        t.rotation *= action.root.rotation;
        t.translation.y -= physical_drop * 0.34;
        t.rotation *= Quat::from_rotation_x(physical_drop * 0.6);
    }
    for (parent, flash, bolt, mag, hand, control, forearm, upper_arm, mut t, mut visibility) in
        &mut parts
    {
        if flash.is_none()
            && bolt.is_none()
            && mag.is_none()
            && hand.is_none()
            && control.is_none()
            && forearm.is_none()
            && upper_arm.is_none()
        {
            continue;
        }
        let Ok(root_parent) = roots.get(parent.0) else {
            continue;
        };
        let Ok(weapon) = weapons.get(root_parent.0) else {
            continue;
        };
        if weapon.weapon != displayed {
            *visibility = Visibility::Hidden;
            continue;
        }
        if flash.is_some() {
            *visibility = if p.alive && displayed == p.weapon && response.flash > 0.0 {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
        if bolt.is_some() {
            *t = common::weapon_animation::bolt_transform(
                displayed,
                &action,
                response.recoil,
                p.reload_remaining > 0.0,
            );
            if displayed == WeaponKind::Pistol && p.ammo_in_mag == 0 && p.reload_remaining == 0.0 {
                t.translation.z = WeaponRig::for_weapon(displayed).bolt_travel;
            }
            *visibility = Visibility::Inherited;
        }
        if control.is_some() {
            *t = common::weapon_animation::control_transform(displayed, &action);
            *visibility = Visibility::Inherited;
        }
        if mag.is_some() {
            *t = action.magazine;
            *visibility = if action.magazine_visible {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
        if hand.is_some() {
            *visibility = if p.body.anatomy.capacity(common::combat::Region::LeftArm) > 0.0 {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            *t = action.support;
        }
        if let Some(support) = forearm
            .map(|arm| arm.support)
            .or_else(|| upper_arm.map(|arm| arm.support))
        {
            let aspect = if let Projection::Perspective(p) = &**projection {
                p.aspect_ratio
            } else {
                1.0
            };
            let offset = super::weapons::framed_offset(displayed, aspect).lerp(
                common::weapon_animation::ads_offset(displayed),
                common::weapon_animation::ease(response.ads),
            );
            let grip = if support {
                action.support
            } else {
                Transform::from_translation(WeaponRig::for_weapon(displayed).trigger)
            };
            let wrist = grip.transform_point(Vec3::Z * 0.055);
            let elbow_camera = Vec3::new(if support { -0.26 } else { 0.27 }, -0.34, -0.16);
            let body_anchor =
                action.root.rotation.inverse() * (elbow_camera - action.root.translation) - offset;
            let elbow = body_side_elbow(wrist, body_anchor);
            *t = if forearm.is_some() {
                forearm_pose(wrist, elbow)
            } else {
                forearm_pose(elbow, body_anchor)
            };
            *visibility = if (!support
                || p.body.anatomy.capacity(common::combat::Region::LeftArm) > 0.0)
                && !(upper_arm.is_some() && elbow.distance_squared(body_anchor) < 0.000001)
            {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
    }
    for mut t in &mut cameras {
        let down = fall_blend(&p.pose);
        t.translation.y += camera_height_feedback(&p.pose);
        if p.alive
            && let Some(physical) = &p.pose.physical
        {
            let head = physical.unpack()[common::humanoid::Segment::Head.index()];
            let offset =
                Quat::from_rotation_y(p.rot.yaw - input.camera.yaw) * Vec3::from(head.position);
            t.translation = offset - Vec3::Y * 0.10;
            t.translation.y = t.translation.y.max(0.18 - p.pos[1]);
        }
        let (l, r) = p.body.anatomy.leg_support();
        t.rotation *= Quat::from_rotation_z(
            (r - l) * phase.sin() * 0.018 + p.pose.lean[0] * 0.025 + down * 0.18,
        );
    }
}

fn fall_blend(pose: &common::humanoid::PoseState) -> f32 {
    if pose.mode == PhysicalMode::Recovering {
        1.0 - common::weapon_animation::ease(pose.mode_time)
    } else if matches!(pose.mode, PhysicalMode::Fallen | PhysicalMode::Dead) {
        common::weapon_animation::ease(pose.mode_time / 0.35)
    } else {
        0.0
    }
}
fn camera_height_feedback(pose: &common::humanoid::PoseState) -> f32 {
    pose.root_drop - fall_blend(pose) * (1.15 - pose.crouch * 0.48) - pose.landing * 0.04
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn composed_view_forearms_keep_anatomical_length_and_actual_glove_contact_in_ads_and_reload() {
        for weapon in [WeaponKind::Pistol, WeaponKind::Smg, WeaponKind::Rifle] {
            for ads in [0.0, 0.5, 1.0] {
                for fraction in [0.0, 0.10, 0.55, 0.82, 0.94] {
                    let mut app = App::new();
                    let action = reload_action(weapon, fraction, true);
                    let mut session = ClientSession::default();
                    session.players.push(ClientData {
                        id: 7,
                        alive: true,
                        weapon,
                        reload_remaining: if fraction > 0.0 {
                            weapon.spec().reload_seconds * (1.0 - fraction)
                        } else {
                            0.0
                        },
                        ..default()
                    });
                    let mut feedback = Feedback::default();
                    feedback.responses.insert(
                        7,
                        Response {
                            displayed: Some(weapon),
                            action: Some(action),
                            ads,
                            recoil: 0.025,
                            ..default()
                        },
                    );
                    app.insert_resource(session)
                        .insert_resource(feedback)
                        .insert_resource(PlayerId(7))
                        .init_resource::<Time>()
                        .init_resource::<LocalInput>()
                        .add_systems(Update, animate_local);
                    app.world_mut().spawn((
                        ViewModelCamera,
                        Transform::default(),
                        Projection::from(PerspectiveProjection {
                            aspect_ratio: 1.6,
                            ..default()
                        }),
                    ));
                    app.world_mut()
                        .spawn((WorldModelCamera, Transform::default()));
                    let offset = super::super::weapons::view_offset(weapon).lerp(
                        common::weapon_animation::ads_offset(weapon),
                        common::weapon_animation::ease(ads),
                    );
                    let mut meshes = Assets::<Mesh>::default();
                    let mut materials = Assets::<StandardMaterial>::default();
                    let root = app
                        .world_mut()
                        .spawn((
                            WeaponMeshRoot,
                            super::super::weapons::assembly(
                                &mut meshes,
                                &mut materials,
                                weapon,
                                offset,
                            ),
                        ))
                        .id();
                    let view = app
                        .world_mut()
                        .spawn((LocalView, WeaponViewModel { weapon }, Transform::default()))
                        .add_child(root)
                        .id();
                    // DefaultPlugins registers visibility for rendered meshes;
                    // this GPU-free app installs that component explicitly.
                    let parts: Vec<_> = app
                        .world_mut()
                        .query_filtered::<Entity, With<Mesh3d>>()
                        .iter(app.world())
                        .collect();
                    for part in parts {
                        app.world_mut()
                            .entity_mut(part)
                            .insert(Visibility::Inherited);
                    }
                    app.update();
                    let parent = *app.world().get::<Transform>(view).unwrap();
                    let mesh_root = *app.world().get::<Transform>(root).unwrap();
                    let composed = parent.mul_transform(mesh_root);
                    // ANIM-14: a mechanically correct contact is not enough if
                    // the sleeves hide the manipulated slide from the eye.
                    if weapon == WeaponKind::Pistol && ads == 0.0 && fraction == 0.82 {
                        use common::humanoid::{SegmentPose, ray_box};
                        let slide = {
                            let mut slides =
                                app.world_mut().query_filtered::<&Transform, With<Bolt>>();
                            *slides.single(app.world()).unwrap()
                        };
                        let mut sleeves = app.world_mut().query_filtered::<(
                            &Transform,
                            &Mesh3d,
                            &Visibility,
                            Option<&Forearm>,
                            Option<&UpperArm>,
                        ), Or<(With<Forearm>, With<UpperArm>)>>(
                        );
                        let sleeves: Vec<_> = sleeves
                            .iter(app.world())
                            .filter(|(_, _, visible, _, _)| **visible != Visibility::Hidden)
                            .map(|(pose, mesh, _, forearm, upper)| {
                                let Some(bevy::mesh::VertexAttributeValues::Float32x3(vertices)) =
                                    meshes
                                        .get(&mesh.0)
                                        .unwrap()
                                        .attribute(Mesh::ATTRIBUTE_POSITION)
                                else {
                                    panic!("actual sleeve vertices required")
                                };
                                let min = vertices
                                    .iter()
                                    .map(|v| Vec3::from(*v))
                                    .fold(Vec3::splat(f32::INFINITY), Vec3::min);
                                let max = vertices
                                    .iter()
                                    .map(|v| Vec3::from(*v))
                                    .fold(Vec3::splat(f32::NEG_INFINITY), Vec3::max);
                                let world = composed.mul_transform(*pose);
                                (
                                    if forearm.is_some() {
                                        "forearm"
                                    } else {
                                        "upper"
                                    },
                                    forearm
                                        .map_or_else(|| upper.unwrap().support, |arm| arm.support),
                                    SegmentPose {
                                        position: world.translation.into(),
                                        rotation: world.rotation.to_array(),
                                    },
                                    (max - min) * 0.5 * world.scale,
                                )
                            })
                            .collect();
                        let facing_side = (composed.rotation.inverse() * -composed.translation)
                            .x
                            .signum()
                            * 0.035;
                        let occlusions: Vec<_> = [-0.16, -0.09, -0.02]
                            .into_iter()
                            .map(|z| {
                                let rail = composed.transform_point(
                                    slide.transform_point(Vec3::new(facing_side, 0.064, z)),
                                );
                                let hits: Vec<_> = sleeves
                                    .iter()
                                    .filter_map(|(kind, support, pose, half)| {
                                        ray_box(
                                            Vec3::ZERO,
                                            rail.normalize(),
                                            *pose,
                                            *half,
                                            rail.length() - 0.001,
                                        )
                                        .map(|hit| (*kind, *support, hit.0, *pose))
                                    })
                                    .collect();
                                (z, rail, hits)
                            })
                            .collect();
                        let clear = occlusions
                            .iter()
                            .filter(|(_, _, hits)| hits.is_empty())
                            .count();
                        assert!(
                            clear >= 2,
                            "sleeves obscure the actual manipulated slide face: {clear}/3 rail samples visible; {occlusions:?}"
                        );
                    }
                    let (glove_mesh, support_pose) = {
                        let mut hands = app
                            .world_mut()
                            .query_filtered::<(&Mesh3d, &Transform), With<SupportHand>>();
                        let (mesh, pose) = hands.single(app.world()).unwrap();
                        (mesh.0.clone(), *pose)
                    };
                    let trigger_pose = {
                        let mut hands = app
                            .world_mut()
                            .query_filtered::<(&Mesh3d, &Transform), Without<SupportHand>>();
                        *hands
                            .iter(app.world())
                            .find(|(mesh, _)| mesh.0 == glove_mesh)
                            .unwrap()
                            .1
                    };
                    let uppers: Vec<_> = app
                        .world_mut()
                        .query::<(&UpperArm, &Transform)>()
                        .iter(app.world())
                        .map(|(arm, pose)| (arm.support, *pose))
                        .collect();
                    let mut arms = app.world_mut().query::<(&Forearm, &Transform, &Mesh3d)>();
                    assert_eq!(arms.iter(app.world()).count(), 2);
                    for (forearm, pose, mesh) in arms.iter(app.world()) {
                        let grip = if forearm.support {
                            support_pose
                        } else {
                            trigger_pose
                        };
                        let Some(bevy::mesh::VertexAttributeValues::Float32x3(vertices)) = meshes
                            .get(&mesh.0)
                            .unwrap()
                            .attribute(Mesh::ATTRIBUTE_POSITION)
                        else {
                            panic!("actual sleeve mesh vertices required");
                        };
                        let top = vertices
                            .iter()
                            .map(|p| p[1])
                            .fold(f32::NEG_INFINITY, f32::max);
                        let bottom = vertices.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
                        let wrist = composed.transform_point(grip.transform_point(Vec3::Z * 0.055));
                        let cuff = composed.transform_point(pose.transform_point(Vec3::Y * top));
                        let elbow =
                            composed.transform_point(pose.transform_point(Vec3::Y * bottom));
                        assert!(
                            cuff.distance(wrist) < 0.0001,
                            "actual rendered cuff detached from glove: {weapon:?} ads={ads} reload={fraction} support={} cuff={cuff:?} wrist={wrist:?} local={pose:?}",
                            forearm.support
                        );
                        assert!(
                            cuff.distance(elbow) <= 0.291,
                            "view arm stretched across the mechanism"
                        );
                        assert!(pose.rotation.is_finite() && elbow.is_finite());
                        let upper = uppers
                            .iter()
                            .find(|(support, _)| *support == forearm.support)
                            .unwrap()
                            .1;
                        let upper_end =
                            composed.transform_point(upper.transform_point(Vec3::Y * 0.14));
                        assert!(
                            upper_end.distance(elbow) < 0.0001,
                            "visible elbow must join the upper arm, never float"
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn forearm_stays_on_the_wrist_and_body_side_instead_of_rotating_with_the_reload_glove() {
        for weapon in [WeaponKind::Pistol, WeaponKind::Smg, WeaponKind::Rifle] {
            for i in 0..=100 {
                let action = reload_action(weapon, i as f32 / 100.0, true);
                let offset = super::super::weapons::view_offset(weapon);
                let wrist = action.support.transform_point(Vec3::Z * 0.055);
                let elbow = action.root.rotation.inverse()
                    * (Vec3::new(-0.26, -0.34, -0.16) - action.root.translation)
                    - offset;
                let sleeve = forearm_pose(wrist, elbow);
                assert!(sleeve.transform_point(Vec3::Y * 0.14).distance(wrist) < 0.0001);
                assert!(sleeve.transform_point(-Vec3::Y * 0.14).distance(elbow) < 0.0001);
                let body_end = action.root.transform_point(offset + elbow);
                // action.root rotates the child offset as well as the sleeve.
                assert!(body_end.x < -0.20 && body_end.y < -0.25);
            }
        }
    }
    #[test]
    fn movement_is_centimetre_scale_continuous_and_quieter_on_the_sights() {
        let rate = common::humanoid::stride_phase_rate(5.5, 0.0);
        let mut previous = locomotion_offset(0.0, 5.5, 0.0);
        for i in 1..=240 {
            let phase = i as f32 / 120.0 * rate;
            let p = locomotion_offset(phase, 5.5, 0.0);
            assert!(p.length() < 0.022);
            assert!(p.distance(previous) < 0.003);
            assert!(locomotion_offset(phase, 5.5, 1.0).length() < 0.002);
            previous = p;
        }
    }
    #[test]
    fn snapshot_look_updates_hold_a_measured_turn_rate_without_sawtooth_weapon_sway() {
        let mut app = App::new();
        let mut session = ClientSession::default();
        session.players.push(ClientData {
            id: 7,
            alive: true,
            ..default()
        });
        app.insert_resource(session)
            .init_resource::<Time>()
            .init_resource::<Feedback>()
            .add_systems(Update, advance);
        let mut samples = Vec::new();
        for frame in 0..120 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(1.0 / 120.0));
            if frame % 4 == 0 {
                let mut session = app.world_mut().resource_mut::<ClientSession>();
                session.simulation_time = frame as f32 / 120.0;
                session.players[0].rot.yaw = session.simulation_time;
            }
            app.update();
            if frame > 90 {
                samples.push(app.world().resource::<Feedback>().responses[&7].sway.x);
            }
        }
        let min = samples.iter().copied().fold(f32::INFINITY, f32::min);
        let max = samples.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(min > -0.0075 && max < -0.0045);
        assert!(
            max - min < 0.001,
            "network sampling must not pulse the weapon every render frame"
        );
    }
    #[test]
    fn swap_holsters_before_revealing_new_model_and_reload_interrupt_blends_from_current_pose() {
        let mut app = App::new();
        let mut session = ClientSession::default();
        session.players.push(ClientData {
            id: 7,
            alive: true,
            weapon: WeaponKind::Pistol,
            ammo_in_mag: 12,
            reload_remaining: 0.75,
            ..default()
        });
        app.init_resource::<Time>()
            .init_resource::<Feedback>()
            .insert_resource(session)
            .add_systems(Update, advance);
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(1.0 / 120.0));
        app.update();
        let before = app.world().resource::<Feedback>().responses[&7]
            .action
            .unwrap();
        app.world_mut().resource_mut::<ClientSession>().players[0].reload_remaining = 0.0;
        app.update();
        let after = app.world().resource::<Feedback>().responses[&7]
            .action
            .unwrap();
        assert!(after.root.translation.distance(before.root.translation) < 0.02);
        assert!(
            after
                .support
                .translation
                .distance(before.support.translation)
                < 0.08
        );
        assert!(
            after.root.translation.length() > 0.01,
            "interruption must not snap straight to idle"
        );
        app.world_mut().resource_mut::<ClientSession>().players[0].weapon = WeaponKind::Rifle;
        app.update();
        assert_eq!(
            app.world().resource::<Feedback>().responses[&7].displayed,
            Some(WeaponKind::Pistol)
        );
        for _ in 0..16 {
            app.update();
        }
        assert_eq!(
            app.world().resource::<Feedback>().responses[&7].displayed,
            Some(WeaponKind::Rifle)
        );
        assert_eq!(
            app.world().resource::<ClientSession>().players[0].ammo_in_mag,
            12,
            "presentation cannot grant ammo"
        );
    }
    #[test]
    fn confirmed_shot_is_once_bounded_and_recovers() {
        let mut r = Response::default();
        let shot = FiredProjectileData {
            id: 1,
            weapon: WeaponKind::Rifle,
            shooter: 7,
            time: 0.0,
            muzzle: [0.0; 3],
            direction: [0.0, 0.0, -1.0],
        };
        r.shot(&shot);
        let v = r.velocity;
        r.shot(&shot);
        assert_eq!(r.velocity, v);
        assert!(r.flash > 0.0);
        for i in 2..100 {
            r.shot(&FiredProjectileData {
                id: i,
                ..shot.clone()
            });
            r.step(0.01);
            assert!(r.recoil <= 0.11);
        }
        for _ in 0..240 {
            r.step(1.0 / 120.0);
        }
        assert!(r.recoil < 0.001);
        assert_eq!(r.flash, 0.0);
    }
    #[test]
    fn standing_and_crouched_falls_keep_camera_above_floor_without_root_jump() {
        use common::humanoid::*;
        for (crouch, root, drop) in [(0.0, 1.0, 0.6), (1.0, 0.7, 0.3)] {
            let mut pose = PoseState {
                crouch,
                root_drop: drop,
                mode: PhysicalMode::Fallen,
                ..default()
            };
            let view = PLAYER_VIEW_HEIGHT + crouch * PLAYER_CROUCH_VIEW_OFFSET;
            assert!(
                ((root - drop) + view + camera_height_feedback(&pose) - (root + view)).abs()
                    < 0.001
            );
            pose.mode_time = 1.0;
            assert!(((root - drop) + view + camera_height_feedback(&pose) - 0.4).abs() < 0.001);
            pose.mode = PhysicalMode::Recovering;
            pose.mode_time = 0.0;
            pose.root_drop = 0.0;
            assert!((root + view + camera_height_feedback(&pose) - 0.4).abs() < 0.001);
        }
    }
    #[test]
    fn first_confirmed_shot_on_a_new_weapon_flashes_but_old_weapon_never_replays() {
        let mut app = App::new();
        let mut session = ClientSession::default();
        session.players.push(ClientData {
            id: 7,
            alive: true,
            weapon: WeaponKind::Pistol,
            ..default()
        });
        session.shots.push(FiredProjectileData {
            id: 1,
            shooter: 7,
            weapon: WeaponKind::Pistol,
            time: 0.0,
            muzzle: [0.0; 3],
            direction: [0.0, 0.0, -1.0],
        });
        app.init_resource::<Time>()
            .init_resource::<Feedback>()
            .insert_resource(session)
            .add_systems(Update, advance);
        app.update();
        assert!(app.world().resource::<Feedback>().responses[&7].flash > 0.0);
        app.world_mut().resource_mut::<ClientSession>().players[0].weapon = WeaponKind::Rifle;
        app.update();
        assert_eq!(app.world().resource::<Feedback>().responses[&7].flash, 0.0);
        app.world_mut().resource_mut::<ClientSession>().players[0].weapon = WeaponKind::Pistol;
        app.update();
        assert_eq!(app.world().resource::<Feedback>().responses[&7].flash, 0.0);
    }
}
