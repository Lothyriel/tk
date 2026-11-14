//! Optional native end-to-end driver. It sends exactly the same inputs as a player.
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
};
use common::{combat::*, map::*, raid::*, *};

use crate::{ClientSystems, input::InputCapture, sync::ClientSession};

pub struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Driver>().add_systems(
            Update,
            drive.in_set(ClientSystems::Input).after(InputCapture),
        );
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
enum Step {
    #[default]
    Connect,
    Equip,
    Deploy,
    Loot,
    Fight,
    Exit,
    Extract,
    Result,
    Return,
    Restart,
    Done,
}

#[derive(Default, Resource)]
struct Driver {
    step: Step,
    since: f64,
    extraction_capture: bool,
}

impl Driver {
    fn advance(&mut self, step: Step, now: f64) {
        info!("PLAYTEST {:?} -> {:?} at {:.2}s", self.step, step, now);
        self.step = step;
        self.since = now;
    }
}

fn capture(commands: &mut Commands, name: &str) {
    let root = std::env::var("TK_PLAYTEST_DIR").unwrap_or_else(|_| "target/playtest".into());
    std::fs::create_dir_all(&root).expect("Create playtest evidence directory");
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(
            std::path::Path::new(&root).join(format!("{name}.png")),
        ));
}

fn move_toward(input: &mut ClientInput, position: [f32; 3], target: [f32; 3]) -> f32 {
    input.camera = CameraInput::default();
    input.run = true;
    let delta = Vec2::new(target[0] - position[0], target[2] - position[2]);
    input.left = delta.x < -0.3;
    input.right = delta.x > 0.3;
    input.forward = delta.y < -0.3;
    input.backward = delta.y > 0.3;
    delta.length()
}

fn drive(
    mut commands: Commands,
    time: Res<Time<Real>>,
    session: Res<ClientSession>,
    id: Res<PlayerId>,
    mut input: ResMut<LocalInput>,
    mut driver: ResMut<Driver>,
    mut exit: MessageWriter<AppExit>,
) {
    let now = time.elapsed_secs_f64();
    assert!(now < 120.0, "PLAYTEST timed out at {:?}", driver.step);
    // Reset only held controls. Sequence counters remain monotonic across stages.
    input.forward = false;
    input.backward = false;
    input.left = false;
    input.right = false;
    input.run = false;
    input.crouch = false;
    input.jump = false;
    input.fire = false;
    input.interact = false;
    let Some(local) = session.local(id.0) else {
        return;
    };
    let p = &local.participant;
    assert!(
        !matches!(p.status, RaidStatus::Dead | RaidStatus::Missing),
        "PLAYTEST raid failed: {:?}, body={:?}",
        p.status,
        local.body.parts
    );
    let desired = Loadout {
        primary: WeaponKind::Rifle,
        ammo: AmmoKind::Ap,
        armor: ArmorKind::Heavy,
        helmet: true,
    };
    match driver.step {
        Step::Connect if now > 3.0 => {
            assert_eq!(session.raid.phase, RaidPhase::Lobby);
            capture(&mut commands, "lobby");
            input.loadout = desired;
            driver.advance(Step::Equip, now);
        }
        Step::Equip if now - driver.since > 0.6 && p.loadout == desired => {
            input.lobby_sequence = input.lobby_sequence.wrapping_add(1);
            driver.advance(Step::Deploy, now);
        }
        Step::Deploy if session.raid.phase == RaidPhase::Active => {
            assert_eq!(p.status, RaidStatus::Active);
            assert_eq!(p.loadout, desired);
            assert_eq!(local.armor.vest, 100.0);
            assert_eq!(local.armor.helmet, 40.0);
            assert_eq!(local.body.parts, [35.0, 85.0, 60.0, 70.0]);
            assert_eq!(local.ammo_in_mag, 30);
            assert_eq!(session.players.iter().filter(|p| p.bot).count(), 2);
            capture(&mut commands, "raid");
            driver.advance(Step::Loot, now);
        }
        Step::Loot => {
            if p.carried == 1 {
                assert!(!session.raid.loot_available[0]);
                info!("PLAYTEST loot confirmed at {:?}", local.pos);
                driver.advance(Step::Fight, now);
            } else if move_toward(&mut input, local.pos, LOOT_POSITIONS[0]) < 1.5 {
                input.forward = false;
                input.backward = false;
                input.left = false;
                input.right = false;
                input.interact = true;
            }
        }
        Step::Fight => {
            if session.players.iter().any(|p| p.bot && !p.body.alive()) {
                assert!(local.ammo_in_mag < 30);
                info!(
                    "PLAYTEST authoritative sentry kill; ammo={}",
                    local.ammo_in_mag
                );
                driver.advance(Step::Exit, now);
            } else {
                assert!(now - driver.since < 15.0, "PLAYTEST failed to hit a sentry");
                let target = session
                    .players
                    .iter()
                    .find(|p| p.bot && p.id == u64::MAX)
                    .expect("First sentry replicated");
                let origin = Vec3::from(local.pos) + Vec3::Y * PLAYER_VIEW_HEIGHT;
                let target = Vec3::from(target.pos) + Vec3::Y * 0.68;
                input.camera = Transform::from_translation(origin)
                    .looking_at(target, Vec3::Y)
                    .rotation
                    .into();
                input.weapon = WeaponKind::Rifle;
                input.fire = true;
                if local.ammo_in_mag == 0 {
                    input.reload_sequence = input.reload_sequence.wrapping_add(1);
                }
            }
        }
        Step::Exit => {
            if move_toward(&mut input, local.pos, EXTRACTION_CENTER) < 1.5 {
                input.forward = false;
                input.backward = false;
                input.left = false;
                input.right = false;
                driver.advance(Step::Extract, now);
            }
        }
        Step::Extract => {
            input.interact = true;
            if p.extraction_seconds >= 1.0 && !driver.extraction_capture {
                capture(&mut commands, "extracting");
                driver.extraction_capture = true;
            }
            if p.status == RaidStatus::Extracted && session.raid.phase == RaidPhase::Debrief {
                assert_eq!(p.secured, 1);
                assert_eq!(p.carried, 0);
                assert!(local.body.alive());
                capture(&mut commands, "debrief");
                driver.advance(Step::Result, now);
            }
        }
        Step::Result if now - driver.since > 2.0 => {
            input.lobby_sequence = input.lobby_sequence.wrapping_add(1);
            driver.advance(Step::Return, now);
        }
        Step::Return if session.raid.phase == RaidPhase::Lobby => {
            assert!(!p.ready);
            assert_eq!(p.status, RaidStatus::Waiting);
            assert_eq!(p.secured, 1);
            if now - driver.since < 1.0 {
                return;
            }
            capture(&mut commands, "return-lobby");
            input.lobby_sequence = input.lobby_sequence.wrapping_add(1);
            driver.advance(Step::Restart, now);
        }
        Step::Restart if session.raid.phase == RaidPhase::Active => {
            assert_eq!(session.raid.raid_id, 2);
            assert_eq!(p.secured, 1);
            assert_eq!(p.carried, 0);
            assert_eq!(session.raid.loot_available, [true; 3]);
            assert_eq!(local.ammo_in_mag, 30);
            assert_eq!(
                session
                    .players
                    .iter()
                    .filter(|p| p.bot && p.body.alive())
                    .count(),
                2
            );
            info!(
                "PLAYTEST PASS: lobby -> combat -> loot -> extraction -> debrief -> lobby -> raid 2; secured=1"
            );
            driver.advance(Step::Done, now);
        }
        Step::Done if now - driver.since > 2.0 => {
            exit.write(AppExit::Success);
        }
        _ => (),
    }
}
