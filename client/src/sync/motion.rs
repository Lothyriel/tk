use bevy::prelude::*;
use common::LocalPlayer;
use std::collections::VecDeque;

use super::ClientSession;

// Two intervals bridge one lost update; the third covers tick/delivery/frame jitter.
const INTERPOLATION_DELAY: f64 = 3.0 / 30.0;

#[derive(Debug)]
struct Sample {
    time: f64,
    position: Vec3,
}

/// Presentation history only. Gameplay/HUD retain the newest authoritative session.
#[derive(Debug, Default)]
pub(crate) struct LocalMotion {
    samples: VecDeque<Sample>,
    clock_offset: Option<f64>,
    last_received: f64,
}

impl LocalMotion {
    pub(super) fn record(&mut self, time: f32, position: Vec3, received: f64, reset: bool) {
        let time = f64::from(time);
        let reset = reset
            || self.samples.back().is_some_and(|last| {
                time <= last.time
                    || position.distance(last.position) > 4.0
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
        self.samples.push_back(Sample { time, position });
        while self.samples.len() > 32 {
            self.samples.pop_front();
        }
    }

    fn position(&self, now: f64) -> Option<Vec3> {
        let time = now - self.clock_offset? - INTERPOLATION_DELAY;
        let mut previous = self.samples.front()?;
        for next in self.samples.iter().skip(1) {
            if time <= next.time {
                let alpha = ((time - previous.time) / (next.time - previous.time)).clamp(0.0, 1.0);
                return Some(previous.position.lerp(next.position, alpha as f32));
            }
            previous = next;
        }
        // Never predict through a wall or keep moving during a packet outage.
        Some(previous.position)
    }
}

pub(super) fn present_local_motion(
    session: Res<ClientSession>,
    time: Res<Time<Real>>,
    mut players: Query<&mut Transform, With<LocalPlayer>>,
) {
    if let Some(position) = session.local_motion.position(time.elapsed_secs_f64()) {
        for mut transform in &mut players {
            transform.translation = position;
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
                        if snapshot_elapsed >= 1.0 / 30.0 {
                            snapshot_elapsed %= 1.0 / 30.0;
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
    fn local_camera_motion_samples_the_confirmed_3d_path_with_a_100ms_delay() {
        let mut motion = super::LocalMotion::default();
        motion.record(1.0, Vec3::new(0.0, 1.0, 0.0), 10.0, false);
        motion.record(1.04, Vec3::new(2.0, 2.0, -1.0), 10.04, false);
        assert!(
            motion
                .position(10.12)
                .unwrap()
                .distance(Vec3::new(1.0, 1.5, -0.5))
                < 0.00001
        );
        assert_eq!(motion.position(10.0), Some(Vec3::new(0.0, 1.0, 0.0)));
        assert_eq!(motion.position(20.0), Some(Vec3::new(2.0, 2.0, -1.0)));
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
                sequence as f32 / 30.0
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
