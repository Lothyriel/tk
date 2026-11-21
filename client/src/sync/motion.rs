use bevy::prelude::*;
use common::LocalPlayer;
use std::collections::VecDeque;

use super::ClientSession;

// Two intervals bridge one lost update; the third covers tick/delivery/frame jitter.
const INTERPOLATION_DELAY: f64 = 3.0 * common::SNAPSHOT_INTERVAL as f64;

#[derive(Debug)]
struct Sample {
    time: f64,
    pose: LocalMotionPose,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LocalMotionPose {
    pub position: Vec3,
    pub crouch: f32,
    pub stride_phase: f32,
    pub stride_speed: f32,
    pub ads: f32,
    pub roll: f32,
}

/// Presentation history only. Gameplay/HUD retain the newest authoritative session.
#[derive(Debug, Default)]
pub(crate) struct LocalMotion {
    samples: VecDeque<Sample>,
    clock_offset: Option<f64>,
    last_received: f64,
}

impl LocalMotion {
    pub(crate) fn record(&mut self, time: f32, pose: LocalMotionPose, received: f64, reset: bool) {
        let time = f64::from(time);
        let reset = reset
            || self.samples.back().is_some_and(|last| {
                time <= last.time
                    || pose.position.distance(last.pose.position) > 4.0
                    || received - self.last_received > 0.25
            });
        if reset {
            self.samples.clear();
            self.clock_offset = None;
        }
        // Anchor once per continuous stream. Re-estimating the clock from packet
        // arrivals would turn delivery jitter into camera speed changes.
        self.clock_offset.get_or_insert(received - time);
        self.last_received = received;
        self.samples.push_back(Sample { time, pose });
        while self.samples.len() > 32 {
            self.samples.pop_front();
        }
    }

    pub(crate) fn sample(&self, now: f64) -> Option<LocalMotionPose> {
        let time = now - self.clock_offset? - INTERPOLATION_DELAY;
        let mut previous = self.samples.front()?;
        for next in self.samples.iter().skip(1) {
            if time <= next.time {
                let alpha =
                    ((time - previous.time) / (next.time - previous.time)).clamp(0.0, 1.0) as f32;
                use std::f32::consts::{PI, TAU};
                let phase_delta =
                    (next.pose.stride_phase - previous.pose.stride_phase + PI).rem_euclid(TAU) - PI;
                return Some(LocalMotionPose {
                    position: previous.pose.position.lerp(next.pose.position, alpha),
                    crouch: previous.pose.crouch.lerp(next.pose.crouch, alpha),
                    stride_phase: previous.pose.stride_phase + phase_delta * alpha,
                    stride_speed: previous
                        .pose
                        .stride_speed
                        .lerp(next.pose.stride_speed, alpha),
                    ads: previous.pose.ads.lerp(next.pose.ads, alpha),
                    roll: previous.pose.roll.lerp(next.pose.roll, alpha),
                });
            }
            previous = next;
        }
        // Never predict through a wall or keep moving during a packet outage.
        Some(previous.pose)
    }
}

pub(super) fn present_local_motion(
    session: Res<ClientSession>,
    time: Res<Time<Real>>,
    mut players: Query<&mut Transform, With<LocalPlayer>>,
) {
    if let Some(pose) = session.local_motion.sample(time.elapsed_secs_f64()) {
        for mut transform in &mut players {
            transform.translation = pose.position;
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::{prelude::*, time::TimeUpdateStrategy};
    use bevy_renet2::prelude::{DefaultChannel, RenetClient, RenetServer};
    use common::{ClientData, Lobby, LocalPlayer, PlayerId, WorldSnapshot, data};
    use std::time::Duration;

    use crate::{ClientSystems, sync::ClientSession};

    fn fixture(fps: f64) -> (App, RenetServer, Entity, [Entity; 2]) {
        let mut peer = RenetClient::new(data::renet_config(), false);
        peer.set_connected();
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, TransformPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                1.0 / fps,
            )))
            .insert_resource(peer)
            .insert_resource(PlayerId(7))
            .init_resource::<Lobby>()
            .init_resource::<common::LocalInput>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .configure_sets(
                Update,
                (ClientSystems::Receive, ClientSystems::Present).chain(),
            )
            .add_plugins(crate::sync::Plugin);
        let root = app
            .world_mut()
            .spawn((LocalPlayer, Transform::default()))
            .id();
        let cameras = [0, 1].map(|_| {
            app.world_mut()
                .spawn((
                    Transform::from_xyz(0.0, common::PLAYER_VIEW_HEIGHT, 0.0),
                    ChildOf(root),
                ))
                .id()
        });
        app.world_mut()
            .resource_mut::<Lobby>()
            .players
            .insert(7, root);
        let mut server = RenetServer::new(data::renet_config());
        server.add_connection(7, false);
        use common::{catalog::*, replication::*};
        let fingerprint = catalog_fingerprint(CATALOG_SOURCE);
        server.send_message(
            7,
            CONFIGURATION_CHANNEL,
            data::encode(&ConfigurationMessage::Epoch {
                epoch: 1,
                catalog: fingerprint,
            }),
        );
        server.send_message(
            7,
            CONFIGURATION_CHANNEL,
            data::encode(&ConfigurationMessage::Configuration(Configuration {
                epoch: 1,
                catalog: fingerprint,
                handle: 1,
                owner: 7,
                slot: EquipmentSlot::Primary,
                instance: WeaponInstanceId(41),
                revision: 0,
                weapon: catalog().configuration(WeaponId::M4A1).unwrap(),
                magazines: (1..=4)
                    .map(|id| common::replication::MagazineDefinition {
                        id: common::catalog::MagazineInstanceId(id),
                        definition: common::catalog::DefinitionId(101),
                    })
                    .collect(),
            })),
        );
        app.update();
        (app, server, root, cameras)
    }

    fn deliver(
        app: &mut App,
        server: &mut RenetServer,
        sequence: u64,
        stamp: f32,
        mut player: ClientData,
    ) {
        player.configuration = 1;
        player.weapon_instance = common::WeaponInstanceId(41);
        let snapshot = WorldSnapshot {
            configuration_epoch: 1,
            sequence,
            simulation_time: stamp,
            players: vec![player],
            raid: default(),
            destroyed_cells: default(),
            projectiles: vec![],
            impact_marks: vec![],
            fired_projectiles: vec![],
            physics_pieces: vec![],
        };
        server.broadcast_message(DefaultChannel::Unreliable, data::encode(&snapshot));
        for packet in server.get_packets_to_send(7).unwrap() {
            app.world_mut()
                .resource_mut::<RenetClient>()
                .process_packet(packet);
        }
    }

    #[test]
    fn local_camera_motion_is_frame_paced_at_walk_and_run_with_loss_and_jitter() {
        for fps in [60.0, 165.0] {
            for speed in [common::PLAYER_WALK_SPEED, common::PLAYER_RUN_SPEED] {
                let (mut app, mut server, root, cameras) = fixture(fps);
                let mut sequence = 0;
                let mut tick = 0;
                let mut snapshot_elapsed = 0.0;
                let mut pending = std::collections::VecDeque::new();
                let mut previous = 0.0;
                let mut max_error = 0.0_f32;
                for frame in 0..(fps as usize * 2) {
                    let now = (frame + 1) as f64 / fps;
                    while (tick + 1) as f64 / 128.0 <= now {
                        tick += 1;
                        snapshot_elapsed += 1.0 / 128.0;
                        if snapshot_elapsed >= f64::from(common::SNAPSHOT_INTERVAL) {
                            snapshot_elapsed %= f64::from(common::SNAPSHOT_INTERVAL);
                            sequence += 1;
                            // One missing update and repeatable 0..8 ms delivery jitter.
                            if sequence != 23 {
                                let stamp = tick as f64 / 128.0;
                                pending.push_back((
                                    stamp + (sequence % 3) as f64 * 0.004,
                                    sequence,
                                    stamp,
                                ));
                            }
                        }
                    }
                    while pending
                        .front()
                        .is_some_and(|&(arrival, _, _)| arrival <= now)
                    {
                        let (_, seq, stamp) = pending.pop_front().unwrap();
                        deliver(
                            &mut app,
                            &mut server,
                            seq,
                            stamp as f32,
                            ClientData {
                                id: 7,
                                pos: [speed * stamp as f32, 1.0, 0.0],
                                ..default()
                            },
                        );
                    }
                    // Mouse yaw must not be replaced by delayed authoritative yaw.
                    let look = Quat::from_rotation_y(frame as f32 * 0.003);
                    app.world_mut().get_mut::<Transform>(root).unwrap().rotation = look;
                    app.update();
                    let transform = app.world().get::<Transform>(root).unwrap();
                    assert_eq!(transform.rotation, look);
                    let x = transform.translation.x;
                    if frame > fps as usize / 2 {
                        let error = (x - previous - speed / fps as f32).abs();
                        max_error = max_error.max(error);
                        for camera in cameras {
                            let eye = app
                                .world()
                                .get::<GlobalTransform>(camera)
                                .unwrap()
                                .translation();
                            assert!((eye.x - x).abs() < 0.0001);
                            assert!((eye.y - 1.0 - common::PLAYER_VIEW_HEIGHT).abs() < 0.0001);
                        }
                        assert!(
                            x <= app
                                .world()
                                .resource::<ClientSession>()
                                .local(7)
                                .unwrap()
                                .pos[0]
                        );
                    }
                    previous = x;
                }
                eprintln!("camera fps={fps} speed={speed} max_step_error_m={max_error:.6}");
                assert!(
                    max_error < 0.001,
                    "camera must advance every render frame: {max_error}"
                );
            }
        }
    }

    #[test]
    fn local_camera_motion_replays_the_same_brake_and_lean_at_30_and_165_hz() {
        use super::{INTERPOLATION_DELAY, LocalMotion, LocalMotionPose};

        // Replay identical confirmed samples; unlike the native live route,
        // render cadence cannot change when authority receives the brake cue.
        let path = |t: f32| {
            let braking = (t - 0.5).clamp(0.0, 0.5);
            let z = -(5.5 * t.min(0.5) + 5.5 * braking - 5.5 * braking * braking);
            let roll = if t < 1.5 {
                0.6 * (t - 1.0).max(0.0)
            } else if t < 2.5 {
                0.3 - 0.6 * (t - 1.5)
            } else {
                (-0.3 + 0.6 * (t - 2.5)).min(0.0)
            };
            (z, roll)
        };
        for fps in [30, 165] {
            let mut motion = LocalMotion::default();
            let mut next_sample = 0;
            let mut last_roll = 0.0_f32;
            for frame in 0..=fps * 4 {
                let now = f64::from(frame) / f64::from(fps);
                while f64::from(next_sample) / 60.0 <= now {
                    let stamp = f64::from(next_sample) / 60.0;
                    let (z, roll) = path(stamp as f32);
                    motion.record(
                        stamp as f32,
                        LocalMotionPose {
                            position: Vec3::new(0.0, 1.0, z),
                            crouch: 0.0,
                            stride_phase: 0.0,
                            stride_speed: 0.0,
                            ads: 0.0,
                            roll,
                        },
                        stamp,
                        false,
                    );
                    next_sample += 1;
                }
                let sample = motion.sample(now).unwrap();
                let (expected_z, expected_roll) = path((now - INTERPOLATION_DELAY).max(0.0) as f32);
                // Linear interpolation of a constant-acceleration segment
                // contributes at most a * snapshot_dt² / 8 (< 0.4 mm).
                assert!((sample.position.z - expected_z).abs() < 0.0004);
                assert!((sample.roll - expected_roll).abs() < 0.00001);
                assert!((sample.roll - last_roll).abs() <= 0.6 / fps as f32 + 0.00001);
                last_roll = sample.roll;
            }
            assert_eq!(last_roll, 0.0);
        }
    }

    #[test]
    fn local_camera_motion_samples_the_confirmed_3d_path_with_a_50ms_delay() {
        use super::{LocalMotion, LocalMotionPose};
        let mut motion = LocalMotion::default();
        let standing = LocalMotionPose {
            position: Vec3::new(0.0, 1.0, 0.0),
            crouch: 0.0,
            stride_phase: std::f32::consts::TAU - 0.2,
            stride_speed: 3.0,
            ads: 0.0,
            roll: -0.2,
        };
        let crouched = LocalMotionPose {
            position: Vec3::new(2.0, 0.7, -1.0),
            crouch: 1.0,
            stride_phase: 0.2,
            stride_speed: 0.0,
            ads: 1.0,
            roll: 0.2,
        };
        motion.record(1.0, standing, 10.0, false);
        motion.record(1.04, crouched, 10.04, false);
        let eye = |now| {
            let sample = motion.sample(now).unwrap();
            sample.position
                + Vec3::Y
                    * (common::PLAYER_VIEW_HEIGHT
                        + sample.crouch * common::PLAYER_CROUCH_VIEW_OFFSET)
        };
        assert!((motion.sample(10.07).unwrap().crouch - 0.5).abs() < 0.00001);
        let middle = motion.sample(10.07).unwrap();
        assert!((middle.stride_phase - std::f32::consts::TAU).abs() < 0.00001);
        assert!((middle.stride_speed - 1.5).abs() < 0.00001);
        assert!((middle.ads - 0.5).abs() < 0.00001);
        assert!(middle.roll.abs() < 0.00001);
        assert!(eye(10.07).distance(eye(10.0).lerp(eye(20.0), 0.5)) < 0.00001);
        assert_eq!(motion.sample(10.0).unwrap().position, standing.position);
        assert_eq!(motion.sample(20.0).unwrap().position, crouched.position);
    }

    #[test]
    fn local_camera_motion_stops_at_confirmed_position_and_keeps_session_authoritative() {
        let (mut app, mut server, root, _) = fixture(165.0);
        for frame in 1..=165 {
            // Stop at x=1; the final confirmed sample repeats that position.
            if frame % 5 == 0 && frame <= 100 {
                let stamp = frame as f32 / 165.0;
                let position = (stamp * common::PLAYER_RUN_SPEED).min(1.0);
                deliver(
                    &mut app,
                    &mut server,
                    frame,
                    stamp,
                    ClientData {
                        id: 7,
                        pos: [position, 1.0, 0.0],
                        ..default()
                    },
                );
                app.update();
                assert_eq!(
                    app.world()
                        .resource::<ClientSession>()
                        .local(7)
                        .unwrap()
                        .pos,
                    [position, 1.0, 0.0]
                );
            } else {
                app.update();
            }
            let x = app.world().get::<Transform>(root).unwrap().translation.x;
            assert!(
                (0.0..=1.0).contains(&x),
                "no overshoot or extrapolation: {x}"
            );
        }
        assert_eq!(
            app.world().get::<Transform>(root).unwrap().translation,
            Vec3::new(1.0, 1.0, 0.0)
        );
    }

    #[test]
    fn local_camera_motion_resets_history_on_discontinuities_and_rejects_old_sequences() {
        let (mut app, mut server, root, _) = fixture(165.0);
        let mut player = ClientData {
            id: 7,
            pos: [0.0, 1.0, 0.0],
            ..default()
        };
        for sequence in 1..=9 {
            player.pos[0] += if sequence == 3 { 5.0 } else { 0.5 };
            match sequence {
                2 => player.alive = true,
                4 => player.pose.mode = common::humanoid::PhysicalMode::Fallen,
                5 => player.participant.status = common::raid::RaidStatus::Extracted,
                6 => {
                    // A new raid must discard an otherwise continuous short path.
                    app.world_mut().resource_mut::<ClientSession>().raid.raid_id = 99;
                }
                7 => {
                    app.world_mut().resource_mut::<ClientSession>().raid.phase =
                        common::raid::RaidPhase::Active;
                }
                8 => {
                    for _ in 0..50 {
                        app.update();
                    }
                }
                _ => {}
            }
            // The final sample restarts the server timeline.
            let stamp = if sequence == 9 {
                0.0
            } else {
                sequence as f32 * common::SNAPSHOT_INTERVAL
            };
            deliver(&mut app, &mut server, sequence, stamp, player.clone());
            app.update();
            assert_eq!(
                app.world().get::<Transform>(root).unwrap().translation,
                Vec3::from(player.pos),
                "reset case {sequence}"
            );
            assert_eq!(
                app.world()
                    .resource::<ClientSession>()
                    .local_motion
                    .samples
                    .len(),
                1
            );
        }
        for sequence in [8, 9] {
            let mut obsolete = player.clone();
            obsolete.pos[0] = 100.0;
            deliver(&mut app, &mut server, sequence, 1.0, obsolete);
            app.update();
            assert_eq!(
                app.world().get::<Transform>(root).unwrap().translation,
                Vec3::from(player.pos)
            );
            assert_eq!(
                app.world()
                    .resource::<ClientSession>()
                    .local(7)
                    .unwrap()
                    .pos,
                player.pos
            );
        }
    }
}
