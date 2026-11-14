//! Confirmed shots only. No audio assets, players or sinks on this path.
use super::{LocalView, ViewModelCamera, WeaponMeshRoot, WeaponViewModel, WorldModelCamera};
use crate::{ClientSystems, sync::ClientSession};
use bevy::{platform::collections::HashMap, prelude::*};
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
}
impl Response {
    fn step(&mut self, dt: f32) {
        self.velocity += (-self.recoil * 180.0 - self.velocity * 23.0) * dt;
        self.recoil = (self.recoil + self.velocity * dt).clamp(0.0, 0.11);
        self.flash = (self.flash - dt).max(0.0);
        self.swap = (self.swap - dt * 4.0).max(0.0);
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

pub(super) fn advance(
    time: Res<Time>,
    session: Res<ClientSession>,
    mut feedback: ResMut<Feedback>,
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
            response.weapon = Some(player.weapon);
            response.flash = 0.0;
        }
        if !player.alive {
            response.flash = 0.0;
            response.recoil = 0.0;
            response.velocity = 0.0;
        }
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

fn animate_local(
    time: Res<Time>,
    id: Res<PlayerId>,
    session: Res<ClientSession>,
    feedback: Res<Feedback>,
    mut views: Query<(&WeaponViewModel, &mut Transform), With<LocalView>>,
    mut parts: Query<
        (
            &ChildOf,
            Option<&Flash>,
            Option<&Bolt>,
            Option<&Magazine>,
            Option<&SupportHand>,
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
    let speed = Vec3::from(p.pose.velocity).length();
    let phase = p.pose.phase;
    let reload = if p.reload_remaining > 0.0 {
        (1.0 - p.reload_remaining / p.weapon.spec().reload_seconds).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let arc = (reload * std::f32::consts::PI).sin();
    for (weapon, mut t) in &mut views {
        if weapon.weapon != p.weapon {
            continue;
        }
        t.translation += Vec3::new(phase.sin() * 0.008, (phase * 2.0).cos() * 0.006, 0.0)
            * speed.min(5.5)
            + Vec3::new(0.0, -arc * 0.11 - response.swap * 0.24, response.recoil);
        t.translation.y -= p.pose.landing * 0.05;
        t.rotation *= Quat::from_rotation_x((p.pose.velocity[1] * 0.012).clamp(-0.08, 0.08));
        let injury = (1.0 - p.body.anatomy.handling()) * (time.elapsed_secs() * 3.7).sin() * 0.035;
        t.rotation *= Quat::from_euler(
            EulerRot::XYZ,
            response.recoil * 1.8 + arc * 0.5,
            -arc * 0.25,
            injury - arc * 0.35,
        );
    }
    for (parent, flash, bolt, mag, hand, mut t, mut visibility) in &mut parts {
        if flash.is_none() && bolt.is_none() && mag.is_none() && hand.is_none() {
            continue;
        }
        let Ok(root_parent) = roots.get(parent.0) else {
            continue;
        };
        let Ok(weapon) = weapons.get(root_parent.0) else {
            continue;
        };
        if weapon.weapon != p.weapon {
            *visibility = Visibility::Hidden;
            continue;
        }
        if flash.is_some() {
            *visibility = if p.alive && response.flash > 0.0 {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
        if bolt.is_some() {
            t.translation.z = 0.01 + response.recoil * 0.7;
        }
        if mag.is_some() {
            t.translation.y = -0.14 - arc * 0.18;
            *visibility = if reload > 0.38 && reload < 0.55 {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            };
        }
        if hand.is_some() {
            *visibility = if p.body.anatomy.capacity(common::combat::Region::LeftArm) > 0.0 {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            t.translation = Vec3::new(-0.07, -0.06 - arc * 0.20, -0.22 + arc * 0.28);
            t.rotation = Quat::from_rotation_z(-arc * 0.9);
        }
    }
    for mut t in &mut cameras {
        let down = fall_blend(&p.pose);
        t.translation.y += camera_height_feedback(&p.pose);
        let (l, r) = p.body.anatomy.leg_support();
        t.rotation *= Quat::from_rotation_z(
            (r - l) * phase.sin() * 0.018 + p.pose.lean[0] * 0.025 + down * 0.18,
        );
    }
}

fn fall_blend(pose: &common::humanoid::PoseState) -> f32 {
    if pose.mode == PhysicalMode::Recovering {
        1.0 - pose.mode_time.clamp(0.0, 1.0)
    } else if matches!(pose.mode, PhysicalMode::Fallen | PhysicalMode::Dead) {
        (pose.mode_time / 0.35).clamp(0.0, 1.0)
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
