//! Optional native end-to-end driver. It sends exactly the same inputs as a player.
use bevy::prelude::*;
use common::{combat::*, map::*, raid::*, *};

use crate::{ClientSystems, input::InputCapture, sync::ClientSession};
mod animation;
mod asset_review;
mod lobby;
mod mechanics;
mod physical;
mod pov;
mod video;

pub struct Plugin;

pub(crate) fn requested_viewport() -> Option<(u32, u32)> {
    let value = std::env::var("TK_PLAYTEST_VIEWPORT").ok()?;
    let (width, height) = value.split_once('x').expect("viewport WIDTHxHEIGHT");
    Some((
        width.parse().expect("viewport width"),
        height.parse().expect("viewport height"),
    ))
}

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Driver>()
            .add_systems(
                Update,
                lobby::drive
                    .before(drive)
                    .after(crate::loadout::EditorInput)
                    .in_set(ClientSystems::Input),
            )
            .add_systems(
                Update,
                drive.in_set(ClientSystems::Input).after(InputCapture),
            )
            .init_resource::<video::CaptureQueue>()
            .add_systems(Update, review_hud.after(drive))
            .add_systems(
                Update,
                asset_review::body_visibility.after(ClientSystems::Present),
            )
            .add_systems(
                PostUpdate,
                asset_review::camera.after(bevy::transform::TransformSystems::Propagate),
            )
            .add_systems(PostUpdate, video::record);
        if std::env::var("TK_PLAYTEST_POV").as_deref() == Ok("stop-lean")
            && let Ok(value) = std::env::var("TK_PLAYTEST_POV_HZ")
        {
            let hz = value.parse::<u32>().expect("POV cadence in Hz");
            assert!((15..=240).contains(&hz));
            app.insert_resource(pov::Cadence::new(hz))
                .add_systems(Last, pov::pace)
                .add_systems(
                    PostUpdate,
                    pov::trace.after(bevy::transform::TransformSystems::Propagate),
                );
        }
    }
}

fn review_hud(driver: Res<Driver>, mut hud: Query<&mut Node, With<crate::hud::RaidHud>>) {
    for mut node in &mut hud {
        // The verbose raid panel covers the hands in portrait review windows.
        // State evidence stays in the route logs; restore ordinary HUD afterward.
        node.display = if matches!(driver.step, Step::Animation | Step::AssetReview) {
            Display::None
        } else {
            Display::Flex
        };
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
enum Step {
    #[default]
    Connect,
    Equip,
    Deploy,
    Showcase,
    AnimationSetup,
    PhysicalSetup,
    PhysicalAction,
    PhysicalRest,
    Pov,
    Animation,
    AssetReview,
    Weapons,
    Loot,
    Fight,
    Exit,
    Extract,
    Result,
    Return,
    Restart,
    Injury,
    Done,
}

#[derive(Default, Resource)]
struct Driver {
    step: Step,
    since: f64,
    extraction_capture: bool,
    frames: Vec<f32>,
    captures: u32,
    actions: u32,
    flash_captures: u8,
    animation: animation::Sequence,
    weapons: mechanics::Weapons,
    injury: mechanics::Injury,
    physical: physical::Review,
    pov_ads: pov::Ads,
    pov_inspect: pov::Inspect,
    pov_stop_lean: pov::StopLean,
    pov_stop_lean_roll: f32,
    pov_visual_since: Option<f64>,
    fall_ids: Vec<u64>,
    recovery_seen: bool,
    recovery_recorded: bool,
    next_fight_report: f64,
    asset_captures: u8,
    panel_configured: bool,
}

impl Driver {
    fn advance(&mut self, step: Step, now: f64) {
        info!("PLAYTEST {:?} -> {:?} at {:.2}s", self.step, step, now);
        self.step = step;
        self.since = now;
    }
}

fn capture(commands: &mut Commands, name: &str) {
    let name = name.to_owned();
    commands
        .queue(move |world: &mut World| world.resource_mut::<video::CaptureQueue>().0.push(name));
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

fn aim_barrel(input: &mut ClientInput, local: &ClientData, target: Vec3) {
    let eye = Vec3::from(local.pos)
        + Vec3::Y
            * (PLAYER_VIEW_HEIGHT
                + if local.crouched {
                    PLAYER_CROUCH_VIEW_OFFSET
                } else {
                    0.0
                });
    input.camera = barrel_aim(eye, target, input.weapon_id());
    // The driver acts as a player holding an anatomical aim point. Recoil is
    // authority-owned; counter only its received estimate through normal input.
    let recoil = local.recoil.unpack();
    input.camera.yaw -= recoil.angles[0];
    input.camera.pitch -= recoil.angles[1];
}

/// Exact zero-roll offset-ray solution. Fixed-point look-at iteration oscillates near the muzzle.
fn barrel_aim(eye: Vec3, target: Vec3, weapon: WeaponId) -> CameraInput {
    let d = target - eye;
    let b = Vec3::from(weapon.spec().barrel_offset);
    let h = (d.length_squared() - b.x * b.x - b.y * b.y)
        .max(0.0001)
        .sqrt();
    let pitch = (d.y / (h * h + b.y * b.y).sqrt()).clamp(-1.0, 1.0).asin() - b.y.atan2(h);
    let local_z = b.y * pitch.sin() - h * pitch.cos();
    CameraInput {
        pitch,
        yaw: (-d.x).atan2(-d.z) + b.x.atan2(-local_z),
        roll: 0.0,
    }
}

fn head_target(target: &ClientData) -> Vec3 {
    let rig = common::humanoid::evaluate_pose(&target.pose, 1.0, 1.0);
    Vec3::from(target.pos)
        + Quat::from_rotation_y(target.rot.yaw)
            * Vec3::from(rig[common::humanoid::Segment::Head.index()].position)
}

#[derive(bevy::ecs::system::SystemParam)]
struct PlaytestState<'w> {
    time: Res<'w, Time<Real>>,
    session: Res<'w, ClientSession>,
    id: Res<'w, PlayerId>,
    input: ResMut<'w, LocalInput>,
    driver: ResMut<'w, Driver>,
}

type AcceptedViewModels<'w, 's> = Query<
    'w,
    's,
    (
        &'static crate::render::voxel::configured::ConfiguredWeapon,
        &'static InheritedVisibility,
    ),
    (
        With<crate::render::voxel::configured::ConfiguredPose>,
        Without<crate::render::voxel::configured::ConfigurationVisualError>,
    ),
>;

#[derive(bevy::ecs::system::SystemParam)]
struct PlaytestLenses<'w, 's> {
    world: Single<'w, 's, &'static Projection, With<crate::render::WorldModelCamera>>,
    view: Single<'w, 's, &'static Projection, With<crate::render::ViewModelCamera>>,
    configured: AcceptedViewModels<'w, 's>,
}

fn drive(
    mut commands: Commands,
    state: PlaytestState,
    mut exit: MessageWriter<AppExit>,
    audio: Query<(), With<AudioPlayer>>,
    mut view_camera: Single<&mut Camera, With<crate::render::ViewModelCamera>>,
    flashes: Query<&Visibility, With<crate::render::feedback::Flash>>,
    projections: PlaytestLenses,
) {
    let PlaytestState {
        time,
        session,
        id,
        mut input,
        mut driver,
    } = state;
    let now = time.elapsed_secs_f64();
    let lens = |view: bool| {
        let projection = if view {
            &*projections.view
        } else {
            &*projections.world
        };
        match projection {
            Projection::Perspective(p) => p.fov.to_degrees(),
            _ => 0.0,
        }
    };
    let world_fov = lens(false);
    assert!(now < 240.0, "PLAYTEST timed out at {:?}", driver.step);
    assert_eq!(
        audio.iter().count(),
        0,
        "Temporary shot mute must not create audio entities"
    );
    driver.frames.push(time.delta_secs() * 1000.0);
    if driver.frames.len() == 300 {
        driver.frames.sort_by(f32::total_cmp);
        info!(
            "TK PERF frame_ms mean={:.3} p95={:.3} max={:.3}",
            driver.frames.iter().sum::<f32>() / 300.0,
            driver.frames[285],
            driver.frames[299]
        );
        driver.frames.clear();
    }
    // Reset only held controls. Sequence counters remain monotonic across stages.
    input.forward = false;
    input.backward = false;
    input.left = false;
    input.right = false;
    input.run = false;
    input.crouch = false;
    input.jump = false;
    input.fire = false;
    // ADS_PROOF_BEGIN
    input.aim = false;
    // ADS_PROOF_END
    input.interact = false;
    let Some(local) = session.local(id.0) else {
        return;
    };
    let p = &local.participant;
    let cache = &session.configurations.cache;
    // A new configuration epoch can arrive before its replacement snapshot.
    // Held input is already cleared above; wait for the exact accepted reference.
    if !cache.matches(cache.epoch(), local) {
        return;
    }
    let spec = &cache
        .get(cache.epoch(), local.configuration)
        .unwrap()
        .resolved;
    let peer = std::env::var("TK_PLAYTEST_ROLE").is_ok_and(|role| role == "1");
    view_camera.is_active = !(driver.step == Step::Injury || peer && driver.step == Step::Showcase);
    let loot_index = if peer { 1 } else { 0 };
    let bot_id = if peer { u64::MAX - 1 } else { u64::MAX };
    if driver.step == Step::Weapons
        && driver.flash_captures & (1 << local.weapon.index()) == 0
        && flashes.iter().any(|v| *v == Visibility::Inherited)
    {
        driver.flash_captures |= 1 << local.weapon.index();
        let name = format!("confirmed-{}-flash", local.weapon_id.spec().catalog_key);
        capture(&mut commands, &name);
        info!(
            "CONFIRMED FLASH EVIDENCE {:?} ammo={} visible_flash_entities={}",
            local.weapon,
            local.ammo_in_mag,
            flashes
                .iter()
                .filter(|v| **v == Visibility::Inherited)
                .count()
        );
    }
    if !matches!(
        driver.step,
        Step::Injury | Step::Done | Step::PhysicalAction | Step::PhysicalRest
    ) {
        assert!(
            !matches!(p.status, RaidStatus::Dead | RaidStatus::Missing),
            "PLAYTEST raid failed: {:?}, body={:?}",
            p.status,
            local.body.parts
        );
    }
    let desired = Loadout {
        primary: if peer && !lobby::enabled() {
            WeaponId::MP5
        } else {
            WeaponId::M4A1
        },
        ammo: AmmoKind::Ap,
        armor: ArmorKind::Heavy,
        helmet: true,
    };
    match driver.step {
        Step::Connect if now > 3.0 => {
            let root = std::path::PathBuf::from(std::env::var("TK_PLAYTEST_DIR").unwrap());
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(
                root.join("camera-settings.json"),
                format!(
                    "{{\"world_fov_degrees\":{world_fov},\"weapon_fov_degrees\":{}}}",
                    lens(true)
                ),
            )
            .unwrap();
            assert_eq!(session.raid.phase, RaidPhase::Lobby);
            capture(&mut commands, "lobby");
            input.loadout = desired;
            driver.advance(Step::Equip, now);
        }
        Step::Equip
            if now - driver.since > 0.6
                && p.loadout == desired
                && (!lobby::enabled() || driver.panel_configured) =>
        {
            input.lobby_sequence = input.lobby_sequence.wrapping_add(1);
            driver.advance(Step::Deploy, now);
        }
        Step::Deploy if session.raid.phase == RaidPhase::Active => {
            assert_eq!(p.status, RaidStatus::Active);
            assert_eq!(p.loadout, desired);
            assert_eq!(local.armor.vest, 100.0);
            assert_eq!(local.armor.helmet, 40.0);
            assert_eq!(local.body.parts, [35.0, 85.0, 60.0, 70.0]);
            assert_eq!(local.ammo_in_mag, spec.magazine_size);
            info!(
                "DEPLOY KIT capacity={} definitions={:?}",
                spec.magazine_size,
                spec.assembly
                    .parts
                    .iter()
                    .map(|part| part.definition.0)
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                session.players.iter().filter(|p| p.bot).count(),
                if pov_fixture() { 0 } else { 2 }
            );
            capture(&mut commands, "raid");
            driver.advance(
                if std::env::var_os("TK_PLAYTEST_POV").is_some() {
                    Step::Pov
                } else if physical::scenario().is_some() {
                    Step::PhysicalSetup
                } else if std::env::var("TK_PLAYTEST_CLIENTS").is_ok_and(|v| v == "2") {
                    if std::env::var_os("TK_ANIMATION_REVIEW").is_some() {
                        Step::AnimationSetup
                    } else {
                        Step::Showcase
                    }
                } else {
                    Step::Loot
                },
                now,
            );
        }
        Step::Pov => {
            let preset = std::env::var("TK_PLAYTEST_POV").expect("POV preset");
            let previous_lean = driver.pov_stop_lean_roll;
            input.camera = CameraInput::default();
            input.weapon = EquipmentSlot::Primary;
            let elapsed = now - driver.since;
            let ads = local.pose.ads;
            let visual_ready = projections.configured.iter().any(|(weapon, visible)| {
                weapon.view_model
                    && weapon.epoch == cache.epoch()
                    && weapon.handle == local.configuration
                    && visible.get()
            });
            if !visual_ready {
                driver.pov_visual_since = None;
            }
            let visual_settled = visual_ready
                && now - *driver.pov_visual_since.get_or_insert(now) >= pov::VIEW_WARMUP_SECONDS;
            let ready = local.pose.grounded && visual_settled;
            let name = match preset.as_str() {
                "ads" => driver.pov_ads.advance(elapsed, ready, ads, world_fov),
                "inspect" => driver.pov_inspect.advance(elapsed, ready, local.mechanics),
                "stop-lean" | "side-cover" => {
                    // The capture runs after Present; allow the interpolation
                    // buffer to catch up before taking a static stance/roll cue.
                    let settled = session.local_motion.sample(now).is_some_and(|pose| {
                        let roll_settled = if driver.pov_stop_lean == pov::StopLean::Neutral {
                            pose.roll.abs() < 0.001
                        } else {
                            (pose.roll - local.pose.aim_roll).abs() < 0.01
                        };
                        roll_settled
                            && (pose.crouch - f32::from(u8::from(local.crouched))).abs() < 0.01
                    });
                    let side_cover = preset == "side-cover";
                    let at_limit = !side_cover
                        || !matches!(driver.pov_stop_lean, pov::StopLean::Left | pov::StopLean::Right)
                        || previous_lean.abs() >= 0.299;
                    driver.pov_stop_lean.advance(elapsed, ready && settled && at_limit, local, side_cover)
                }
                _ => panic!("Unknown POV preset: {preset}"),
            }
            .unwrap_or_else(|error| {
                panic!(
                    "{error}: preset={preset} movement={:?} pos={:?} velocity={:?} roll={} ads={ads} fov={world_fov} visual_ready={visual_ready} mechanics={:?}",
                    driver.pov_stop_lean, local.pos, local.pose.velocity, local.pose.aim_roll, local.mechanics
                )
            });
            if let Some(name) = name {
                info!(
                    "POV EVIDENCE {name} t={now:.3} pos={:?} velocity={:?} crouch={:.3} roll={:.3} ads={ads:.3} fov={world_fov:.2} config={} action={:?} fraction={:.3}",
                    local.pos,
                    local.pose.velocity,
                    local.pose.crouch,
                    local.pose.aim_roll,
                    local.configuration,
                    local.mechanics.kind,
                    local.mechanics.fraction
                );
                capture(&mut commands, name);
            }
            input.aim = driver.pov_ads == pov::Ads::Aim;
            if preset == "inspect" && name == Some("pov-rest") {
                input.inspect_sequence = input.inspect_sequence.wrapping_add(1);
            }
            if matches!(preset.as_str(), "stop-lean" | "side-cover") {
                driver
                    .pov_stop_lean
                    .input(&mut input, previous_lean, time.delta_secs());
                driver.pov_stop_lean_roll = input.camera.roll;
            }
            if (driver.pov_ads == pov::Ads::Finish
                || driver.pov_inspect == pov::Inspect::Finish
                || driver.pov_stop_lean == pov::StopLean::Finish)
                && std::path::Path::new(&std::env::var("TK_PLAYTEST_DIR").unwrap())
                    .join("pov-return.png")
                    .is_file()
            {
                info!("POV PASS {preset} t={now:.3}");
                exit.write(AppExit::Success);
            }
        }
        Step::PhysicalSetup => {
            let name = physical::scenario().unwrap();
            let distance = physical::setup(&mut input, local, peer, &name);
            if now - driver.since > 4.0 {
                assert!(
                    distance < 0.7,
                    "physical review setup must use ordinary movement"
                );
                driver.advance(Step::PhysicalAction, now);
            }
        }
        Step::PhysicalAction => {
            let name = physical::scenario().unwrap();
            view_camera.is_active = peer;
            let other = session
                .players
                .iter()
                .find(|p| !p.bot && p.id != id.0)
                .unwrap();
            let elapsed = now - driver.since;
            if physical::action(
                &mut commands,
                &mut driver.physical,
                &mut input,
                local,
                other,
                physical::ReviewStep {
                    peer,
                    name: &name,
                    elapsed,
                },
            ) {
                driver.advance(Step::PhysicalRest, now);
            }
        }
        Step::PhysicalRest => {
            let name = physical::scenario().unwrap();
            view_camera.is_active = peer;
            let target = if peer {
                local
            } else {
                session
                    .players
                    .iter()
                    .find(|p| !p.bot && p.id != id.0)
                    .unwrap()
            };
            let elapsed = now - driver.since;
            if physical::rest(
                &mut commands,
                &mut driver.physical,
                &mut input,
                local,
                target,
                &session.physics_pieces,
                physical::ReviewStep {
                    peer,
                    name: &name,
                    elapsed,
                },
            ) {
                driver.advance(Step::Done, now);
            }
        }
        Step::AnimationSetup => {
            let target = if peer {
                [-7.0, 1.0, 18.0]
            } else {
                [-5.0, 1.0, 16.0]
            };
            if move_toward(&mut input, local.pos, target) < 0.4 {
                input.forward = false;
                input.backward = false;
                input.left = false;
                input.right = false;
            }
            if now - driver.since > 3.5 {
                let distance =
                    Vec2::new(local.pos[0] - target[0], local.pos[2] - target[2]).length();
                assert!(
                    distance < 0.7,
                    "Animation review must reach safe staging through ordinary movement"
                );
                driver.advance(Step::Animation, now);
            }
        }
        Step::Animation => {
            let elapsed = now - driver.since;
            let observer = animation::drive(
                &mut commands,
                &mut driver.animation,
                &mut input,
                local,
                session.players.iter().find(|p| !p.bot && p.id != id.0),
                animation::ReviewFrame {
                    elapsed,
                    peer,
                    world_fov,
                    magazine_size: spec.magazine_size,
                },
            );
            view_camera.is_active = !observer;
            if driver.animation.complete {
                info!(
                    "CONTINUOUS ANIMATION PASS reciprocal M1911 + {:?}; empty/tactical/interrupted reloads through normal UDP inputs",
                    desired.primary
                );
                let next = if std::env::var_os("TK_WEAPON_DEBUG").is_some() {
                    Step::AssetReview
                } else {
                    Step::Showcase
                };
                driver.advance(next, now);
            }
        }
        Step::AssetReview => {
            view_camera.is_active = false;
            input.camera = CameraInput::default();
            let elapsed = now - driver.since;
            for (index, label) in asset_review::VIEWS.into_iter().enumerate() {
                if elapsed >= index as f64 * 2.0 + 1.0 && driver.asset_captures & (1 << index) == 0
                {
                    driver.asset_captures |= 1 << index;
                    capture(&mut commands, &format!("asset-{label}"));
                    info!("ASSET VIEW {label} elapsed={elapsed:.2} world_fov={world_fov}");
                }
            }
            if elapsed >= 8.0 {
                assert_eq!(driver.asset_captures, 0b1111);
                driver.advance(Step::Showcase, now);
            }
        }
        Step::Showcase => {
            let elapsed = now - driver.since;
            if peer {
                if let Some(actor) = session.players.iter().find(|p| !p.bot && p.id != id.0) {
                    input.camera = Transform::from_translation(
                        Vec3::from(local.pos) + Vec3::Y * PLAYER_VIEW_HEIGHT,
                    )
                    .looking_at(Vec3::from(actor.pos) + Vec3::Y * 0.15, Vec3::Y)
                    .rotation
                    .into();
                    // Follow from the side at readable full-body distance,
                    // outside the green exit pillars. All movement remains UDP input.
                    let goal = Vec3::from(actor.pos) + Vec3::new(-3.0, 0.0, 0.5);
                    let delta =
                        Quat::from_rotation_y(-input.camera.yaw) * (goal - Vec3::from(local.pos));
                    if Vec2::new(delta.x, delta.z).length() > 0.45 {
                        input.left = delta.x < -0.25;
                        input.right = delta.x > 0.25;
                        input.forward = delta.z < -0.25;
                        input.backward = delta.z > 0.25;
                        input.run = delta.length() > 4.5;
                    }
                }
            } else {
                input.camera = default();
                if (1.5..2.5).contains(&elapsed) {
                    input.forward = true;
                }
                if (2.5..3.2).contains(&elapsed) {
                    input.backward = true;
                    input.run = true;
                }
                if (3.2..3.8).contains(&elapsed) {
                    input.left = true;
                }
                if (3.8..4.6).contains(&elapsed) {
                    input.right = true;
                    input.crouch = true;
                }
                if (4.6..5.3).contains(&elapsed) {
                    input.jump = true;
                }
                if (5.8..6.3).contains(&elapsed) {
                    input.backward = true;
                }
            }
            for (index, (at, name)) in [
                (1.4, "pose-idle"),
                (2.2, "pose-walk"),
                (2.9, "pose-run"),
                (3.6, "pose-strafe"),
                (4.4, "pose-crouch"),
                (4.9, "pose-jump"),
                (5.6, "pose-land"),
                (6.4, "pose-stop"),
            ]
            .into_iter()
            .enumerate()
            {
                if elapsed >= at && driver.captures & (1 << index) == 0 {
                    capture(&mut commands, name);
                    driver.captures |= 1 << index;
                    let actor = if peer {
                        session
                            .players
                            .iter()
                            .find(|p| !p.bot && p.id != id.0)
                            .unwrap_or(local)
                    } else {
                        local
                    };
                    info!(
                        "POSE EVIDENCE {} actor={} position={:?} velocity={:?} grounded={} crouch={} phase={:.3}",
                        name,
                        actor.id,
                        actor.pos,
                        actor.pose.velocity,
                        actor.pose.grounded,
                        actor.pose.crouch,
                        actor.pose.phase
                    );
                }
            }
            if elapsed > 6.8 {
                driver.advance(Step::Weapons, now);
            }
        }
        Step::Weapons => {
            let since = driver.since;
            let elapsed = driver.weapons.control(&mut input, local, now - since);
            input.camera = CameraInput {
                pitch: 0.8,
                ..default()
            };
            for (index, (at, name)) in [
                (0.8, "weapon-burst"),
                (3.3, "weapon-empty"),
                (3.9, "weapon-reload"),
                (4.4, "weapon-pistol"),
                (5.3, "weapon-pistol-reload"),
                (7.0, "weapon-pistol-ready"),
                (8.3, "weapon-primary-reload"),
            ]
            .into_iter()
            .enumerate()
            {
                let bit = 1 << (index + 8);
                if elapsed >= at && driver.captures & bit == 0 {
                    capture(&mut commands, name);
                    driver.captures |= bit;
                    info!(
                        "WEAPON EVIDENCE {} {:?} mag={} reserve={} reload={:.3} audio_entities={}",
                        name,
                        local.weapon,
                        local.ammo_in_mag,
                        local.reserve_ammo,
                        local.reload_remaining(),
                        audio.iter().count()
                    );
                    if name == "weapon-empty" {
                        assert_eq!(local.ammo_in_mag, 0);
                    }
                    if name == "weapon-pistol-ready" {
                        assert_eq!(local.weapon, WeaponKind::Pistol);
                        assert!(local.mechanics.chambered && local.mechanics.magazine_inserted);
                        assert_eq!(local.ammo_in_mag, spec.magazine_size + 1);
                        assert_eq!(local.reload_remaining(), 0.0);
                    }
                }
            }
            if elapsed > 10.0 {
                assert_eq!(local.weapon_id, desired.primary);
                assert_eq!(local.ammo_in_mag, spec.magazine_size);
                assert_eq!(local.reload_remaining(), 0.0);
                info!(
                    "WEAPON EXERCISE PASS primary empty, blocked trigger, interrupted reload, pistol shot/full reload, primary full reload; audio_entities=0"
                );
                driver.advance(Step::Loot, now);
            }
        }
        Step::Loot => {
            if p.carried == 1 {
                assert!(!session.raid.loot_available[loot_index]);
                info!("PLAYTEST loot confirmed at {:?}", local.pos);
                driver.advance(Step::Fight, now);
            } else if move_toward(&mut input, local.pos, LOOT_POSITIONS[loot_index]) < 1.5 {
                input.forward = false;
                input.backward = false;
                input.left = false;
                input.right = false;
                input.interact = true;
            }
        }
        Step::Fight => {
            if session
                .players
                .iter()
                .any(|p| p.id == bot_id && !p.body.alive())
            {
                assert!(local.ammo_in_mag < spec.magazine_size);
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
                    .find(|p| p.bot && p.id == bot_id)
                    .expect("First sentry replicated");
                if now >= driver.next_fight_report {
                    driver.next_fight_report = now + 1.0;
                    info!(
                        "PLAYTEST FIGHT local={:?} ammo={}/{} mechanics={:?} body={:?} target={:?} head={:?} target_roll={} target_body={:?}",
                        local.pos,
                        local.ammo_in_mag,
                        local.reserve_ammo,
                        local.mechanics,
                        local.body.parts,
                        target.pos,
                        head_target(target),
                        target.pose.aim_roll,
                        target.body.parts
                    );
                }
                input.weapon = EquipmentSlot::Primary;
                // Roll/lean displaces the head away from the actor root. A fixed
                // root + height can miss every round while the sentry is leaning.
                aim_barrel(&mut input, local, head_target(target));
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
            if !peer {
                input.lobby_sequence = input.lobby_sequence.wrapping_add(1);
            }
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
            assert_eq!(local.ammo_in_mag, spec.magazine_size);
            if lobby::enabled() {
                let definitions: Vec<_> = spec
                    .assembly
                    .parts
                    .iter()
                    .map(|part| part.definition.0)
                    .collect();
                for expected in if peer {
                    &[122, 128, 129, 132, 133, 134][..]
                } else {
                    &[120, 122, 124, 126][..]
                } {
                    assert!(
                        definitions.contains(expected),
                        "redeploy lost configured part {expected}"
                    );
                }
                assert_eq!(spec.magazine_size, if peer { 30 } else { 20 });
                info!(
                    "REDEPLOY CONFIGURATOR PASS capacity={} definitions={definitions:?}",
                    spec.magazine_size
                );
            }
            assert_eq!(local.body.anatomy.missing, 0);
            assert_eq!(local.body.anatomy.organs, [100.0; 4]);
            assert_eq!(local.body.anatomy.blood, 100.0);
            assert!(
                session.physics_pieces.is_empty(),
                "Previous raid physics leaked"
            );
            info!(
                "ANATOMY RESET raid={} missing={} wounds={} blood={} organs={:?} physics_pieces=0",
                session.raid.raid_id,
                local.body.anatomy.missing,
                local.body.anatomy.wounds,
                local.body.anatomy.blood,
                local.body.anatomy.organs
            );
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
            if std::env::var("TK_PLAYTEST_CLIENTS").is_ok_and(|v| v == "2") {
                driver.captures = 0;
                driver.actions = 0;
                driver.advance(Step::Injury, now);
            } else {
                driver.advance(Step::Done, now);
            }
        }
        Step::Injury => {
            use common::humanoid::*;
            let Some(other) = session.players.iter().find(|p| !p.bot && p.id != id.0) else {
                return;
            };
            let since = driver.since;
            let elapsed = driver
                .injury
                .control(&mut input, local, other, now - since, peer);
            // LIVE_RAGDOLL_PROOF_BEGIN
            let victim = if peer { local } else { other };
            let mut body_ids: Vec<_> = session
                .physics_pieces
                .iter()
                .filter(|p| p.owner == victim.id && !p.detached)
                .map(|p| p.id)
                .collect();
            body_ids.sort_unstable();
            if victim.pose.mode == PhysicalMode::Fallen && driver.fall_ids.is_empty() {
                assert!(
                    victim.pose.physical.is_some(),
                    "native fall must exercise articulated physics, not the budget fallback"
                );
                assert_eq!(
                    body_ids.len(),
                    SEGMENT_COUNT - victim.body.anatomy.missing.count_ones() as usize
                );
                driver.fall_ids = body_ids.clone();
                capture(&mut commands, "injury-physical-begin");
                info!(
                    "LIVE RECOVERY EVIDENCE fall victim={} ids={body_ids:?}",
                    victim.id
                );
            }
            if victim.pose.mode == PhysicalMode::Recovering {
                assert!(victim.pose.physical.is_some());
                assert_eq!(
                    body_ids, driver.fall_ids,
                    "get-up replaced physical IDs under snapshot loss"
                );
                if !driver.recovery_seen {
                    capture(&mut commands, "injury-getup");
                    info!(
                        "LIVE RECOVERY EVIDENCE getup victim={} ids={body_ids:?}",
                        victim.id
                    );
                    driver.recovery_seen = true;
                }
            }
            if victim.pose.mode == PhysicalMode::Standing
                && driver.recovery_seen
                && !driver.recovery_recorded
            {
                assert!(
                    victim.pose.physical.is_none() && body_ids.is_empty(),
                    "standing must remove the old physical visual in the same snapshot"
                );
                let path = std::path::PathBuf::from(std::env::var("TK_PLAYTEST_DIR").unwrap())
                    .join("live-recovery.json");
                std::fs::write(path, format!("{{\"victim\":{},\"fall_ids\":{:?},\"getup_ids\":{:?},\"control_restored\":true}}", victim.id, driver.fall_ids, driver.fall_ids)).unwrap();
                capture(&mut commands, "injury-restored");
                info!(
                    "LIVE RECOVERY PASS victim={} stable_ids={} control_restored=true",
                    victim.id,
                    driver.fall_ids.len()
                );
                driver.recovery_recorded = true;
            }
            if elapsed > 21.9 {
                assert!(
                    driver.recovery_recorded,
                    "missing articulated recovery evidence"
                );
            }
            if elapsed > 18.2 {
                assert!(
                    driver.recovery_recorded,
                    "supported articulated recovery must complete by the approved 18.2s checkpoint"
                );
            }
            // LIVE_RAGDOLL_PROOF_END
            if elapsed < 2.5 {
                move_toward(
                    &mut input,
                    local.pos,
                    if peer {
                        [0.0, 1.0, 17.0]
                    } else {
                        [-5.0, 1.0, 14.0]
                    },
                );
            } else {
                let mut segment = if elapsed < 7.0 {
                    if peer {
                        Segment::RightShin
                    } else {
                        Segment::LeftShin
                    }
                } else if elapsed < 10.0 {
                    Segment::RightUpperArm
                } else if elapsed < 18.0 {
                    Segment::LeftShin
                } else {
                    Segment::Head
                };
                // A separated segment has no live pose. Keep the ordinary
                // observer camera on the body instead of aiming at its absent
                // limb's placeholder; no further shin shot is fired once absent.
                if other.body.anatomy.missing & (1 << segment.index()) != 0 {
                    segment = Segment::Thorax;
                }
                let entry = session
                    .configurations
                    .cache
                    .get(session.configurations.cache.epoch(), other.configuration)
                    .expect("injury target configuration");
                let rig = configured::evaluate(
                    &other.pose,
                    [
                        other.body.anatomy.capacity(Region::LeftLeg),
                        other.body.anatomy.capacity(Region::RightLeg),
                    ],
                    &entry.resolved,
                    &other.mechanics,
                    None,
                    Some(configured::shoulder_cover(&other.armor)),
                )
                .expect("injury target anatomy")
                .segments;
                let target = Vec3::from(other.pos)
                    + Quat::from_rotation_y(other.rot.yaw)
                        * Vec3::from(rig[segment.index()].position);
                aim_barrel(&mut input, local, target);
                for (index, at) in [3.0, 7.4, 11.0, 11.5, 12.0, 12.5, 13.0]
                    .into_iter()
                    .enumerate()
                {
                    if elapsed >= at && driver.actions & (1 << index) == 0 {
                        driver.actions |= 1 << index;
                        if index == 0
                            || !peer
                                && (index == 1
                                    || other.body.anatomy.missing
                                        & (1 << Segment::LeftShin.index())
                                        == 0)
                        {
                            input.fire_pressed_sequence =
                                input.fire_pressed_sequence.wrapping_add(1);
                            info!(
                                "INJURY SHOT cue={at} target={segment:?} weapon={:?} mechanics={:?}",
                                local.weapon, local.mechanics
                            );
                        }
                    }
                }
                if (4.0..5.0).contains(&elapsed) {
                    input.backward = true;
                }
                // Approach through ordinary movement so wound/stump evidence is readable, not a distant speck.
                let distance =
                    Vec2::new(local.pos[0] - other.pos[0], local.pos[2] - other.pos[2]).length();
                if !peer && (5.1..7.0).contains(&elapsed) && distance > 3.2 {
                    input.forward = true;
                    input.run = true;
                }
                if !peer && (14.6..16.2).contains(&elapsed) && distance > 1.8 {
                    input.forward = true;
                }
                if peer && (13.5..14.1).contains(&elapsed) {
                    input.forward = true;
                }
                if !peer && elapsed > 18.5 && other.body.alive() {
                    input.fire = true;
                }
                if !peer
                    && elapsed > 20.0
                    && !other.body.alive()
                    && let Some(piece) = session.physics_pieces.iter().find(|p| {
                        p.owner == other.id && p.segment == Segment::Thorax && !p.detached
                    })
                {
                    aim_barrel(&mut input, local, piece.pose.position.into());
                }
            }
            for (index, (at, name)) in [
                (2.8, "injury-healthy"),
                (3.2, "injury-correction"),
                (4.7, "injury-lateral-limp"),
                (7.7, "injury-arm"),
                (13.3, "injury-separation"),
                (14.2, "injury-fall"),
                (18.2, "injury-recovery"),
                (20.5, "injury-ragdoll"),
            ]
            .into_iter()
            .enumerate()
            {
                let mut capture_due = elapsed >= at;
                if name == "injury-fall" {
                    // Capture the confirmed state, not a local clock estimate
                    // of the peer's movement. The route gates on this same fall.
                    capture_due = victim.pose.mode == PhysicalMode::Fallen;
                }
                // LIVE_RAGDOLL_PROOF_BEGIN
                if name == "injury-recovery" {
                    // Capture the actual supported completion, not an assumed
                    // fixed settling trajectory. The 18.2s bound above remains.
                    capture_due = driver.recovery_recorded;
                }
                // LIVE_RAGDOLL_PROOF_END
                if capture_due && driver.captures & (1 << index) == 0 {
                    driver.captures |= 1 << index;
                    capture(&mut commands, name);
                    info!(
                        "INJURY EVIDENCE {} local={} anatomy={:?} mode={:?} other={} anatomy={:?} mode={:?} pieces={:?}",
                        name,
                        id.0,
                        local.body.anatomy,
                        local.pose.mode,
                        other.id,
                        other.body.anatomy,
                        other.pose.mode,
                        session
                            .physics_pieces
                            .iter()
                            .map(|p| (p.id, p.owner, p.segment, p.detached))
                            .collect::<Vec<_>>()
                    );
                    if name == "injury-lateral-limp" {
                        let region = if peer {
                            Region::LeftLeg
                        } else {
                            Region::RightLeg
                        };
                        assert!(
                            local.body.anatomy.capacity(region) < 1.0,
                            "Normal peer shot did not injure expected leg"
                        );
                    }
                    if name == "injury-separation" {
                        let target = if peer { local } else { other };
                        assert!(
                            target.body.anatomy.missing & (1 << Segment::LeftShin.index()) != 0
                        );
                    }
                    let target = if peer { local } else { other };
                    if name == "injury-fall" {
                        assert_eq!(target.pose.mode, PhysicalMode::Fallen);
                    }
                    if name == "injury-recovery" {
                        assert_eq!(target.pose.mode, PhysicalMode::Standing);
                    }
                    if name == "injury-arm" {
                        assert!(target.body.anatomy.capacity(Region::RightArm) < 1.0);
                    }
                }
            }
            if elapsed > 22.0 {
                let target = if peer { local } else { other };
                assert!(
                    !target.body.alive(),
                    "Normal brain/vital shots must end control"
                );
                assert!(
                    session
                        .physics_pieces
                        .iter()
                        .any(|p| p.detached && p.owner == target.id)
                );
                assert!(
                    session
                        .physics_pieces
                        .iter()
                        .any(|p| !p.detached && p.owner == target.id)
                );
                let mut ids: Vec<_> = session.physics_pieces.iter().map(|p| p.id).collect();
                ids.sort_unstable();
                let count = ids.len();
                ids.dedup();
                assert_eq!(count, ids.len());
                info!(
                    "CONVERGENCE victim={} missing={} wounds={} organs={:?} pieces={:?}",
                    target.id,
                    target.body.anatomy.missing,
                    target.body.anatomy.wounds,
                    target.body.anatomy.organs,
                    ids
                );
                info!(
                    "PHYSICAL PLAYTEST PASS: normal peer limb shots, lateral impairment, separation and ragdoll converge; unique_pieces={} audio_entities=0",
                    count
                );
                driver.advance(Step::Done, now);
            }
        }
        Step::Done if now - driver.since > 2.0 => {
            exit.write(AppExit::Success);
        }
        _ => (),
    }
}

#[cfg(test)]
mod aiming_tests {
    use super::*;
    use common::humanoid::{Segment, SegmentPose, ray_box};

    #[test]
    fn native_sentry_aim_hits_leaning_head_instead_of_fixed_root_height() {
        let eye = Vec3::new(-4.12, 1.018 + PLAYER_VIEW_HEIGHT, 0.918);
        for roll in [-0.4456, -0.2569, 0.2569, 0.4456] {
            for yaw in [-0.65, 0.0, 0.65] {
                let target = ClientData {
                    pos: [0.0, 1.018, -10.0],
                    rot: CameraInput { yaw, ..default() },
                    pose: common::humanoid::PoseState {
                        aim_roll: roll,
                        aim_pitch: -0.0471,
                        ..default()
                    },
                    ..default()
                };
                // Independent anatomical oracle: standing head is 0.65 m above
                // root, shifted 0.7 m/rad by roll, then rotated by actor yaw.
                let root_rotation = Quat::from_rotation_y(yaw);
                let center =
                    Vec3::from(target.pos) + root_rotation * Vec3::new(-roll * 0.7, 0.65, 0.0);
                assert!(head_target(&target).distance(center) < 1e-6);
                let head = SegmentPose {
                    position: center.to_array(),
                    rotation: (root_rotation
                        * Quat::from_rotation_x(-0.0471)
                        * Quat::from_rotation_z(roll))
                    .to_array(),
                };
                let aim = Quat::from(&barrel_aim(eye, head_target(&target), WeaponId::M4A1));
                let muzzle = eye + aim * Vec3::from(WeaponId::M4A1.spec().barrel_offset);
                let direction = aim * Vec3::NEG_Z;
                assert!((center - muzzle).cross(direction).length() < 0.001);
                assert!(
                    ray_box(muzzle, direction, head, Segment::Head.half_size(), 20.0).is_some()
                );
                if roll.abs() > 0.4 && yaw == 0.0 {
                    let old = Quat::from(&barrel_aim(
                        eye,
                        Vec3::from(target.pos) + Vec3::Y * 0.68,
                        WeaponId::M4A1,
                    ));
                    assert!(
                        ray_box(
                            eye + old * Vec3::from(WeaponId::M4A1.spec().barrel_offset),
                            old * Vec3::NEG_Z,
                            head,
                            Segment::Head.half_size(),
                            20.0,
                        )
                        .is_none(),
                        "fixture must reproduce the old fixed-height miss"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn close_and_far_offset_barrels_reach_the_intended_point_without_changing_ballistics() {
        let eye = Vec3::new(1.0, 1.55, 17.0);
        for weapon in [WeaponKind::Rifle, WeaponKind::Smg, WeaponKind::Pistol] {
            for offset in [
                Vec3::new(1.3, 0.3, 0.8),
                Vec3::new(-1.6, -0.9, -0.7),
                Vec3::new(4.0, 0.1, -10.0),
            ] {
                let target = eye + offset;
                let input = barrel_aim(eye, target, weapon.definition());
                let rotation = Quat::from(&input);
                let muzzle = eye + rotation * Vec3::from(weapon.spec().barrel_offset);
                let direction = rotation * (-Vec3::Z);
                let distance = (target - muzzle).dot(direction);
                assert!(distance > 0.0);
                assert!(
                    (muzzle + direction * distance).distance(target) < 0.001,
                    "{weapon:?} {offset:?}"
                );
            }
        }
    }
}
