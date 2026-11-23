use bevy::prelude::*;
use bevy_renet2::prelude::RenetClient;
use common::{
    data,
    map::destruction::{self, CoverSnapshot},
};

#[derive(Default, Resource)]
pub(crate) struct State {
    pub snapshot: Option<CoverSnapshot>,
}

impl State {
    pub(crate) fn current(&self, raid: &common::raid::RaidState) -> Option<&CoverSnapshot> {
        self.snapshot.as_ref().filter(|snapshot| {
            snapshot.raid_id == raid.raid_id
                && (snapshot.phase == raid.phase
                    || matches!(
                        (snapshot.phase, raid.phase),
                        (
                            common::raid::RaidPhase::Active,
                            common::raid::RaidPhase::Debrief
                        )
                    ))
        })
    }
}

pub(super) fn receive(mut client: ResMut<RenetClient>, mut state: ResMut<State>) {
    while let Some(bytes) = client.receive_message(destruction::CHANNEL) {
        if bytes.len() > destruction::MAX_SNAPSHOT_BYTES {
            continue;
        }
        let Ok(snapshot) = data::try_decode::<CoverSnapshot>(&bytes) else {
            continue;
        };
        if !snapshot.valid()
            || state.snapshot.as_ref().is_some_and(|old| {
                old.raid_id == snapshot.raid_id
                    && snapshot.rubble.iter().any(|fragment| {
                        old.rubble
                            .iter()
                            .find(|previous| previous.id == fragment.id)
                            .is_some_and(|previous| {
                                previous.cell != fragment.cell || previous.boxes != fragment.boxes
                            })
                    })
            })
        {
            continue;
        }
        if state
            .snapshot
            .as_ref()
            .is_none_or(|old| snapshot.sequence > old.sequence)
        {
            state.snapshot = Some(snapshot);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_renet2::prelude::RenetServer;
    use common::{
        map::{
            destroyed_mask,
            destruction::{Cut, RubblePose},
        },
        raid::RaidPhase,
    };

    #[test]
    fn destruction_invalid_or_mutated_fragment_state_never_replaces_accepted_geometry() {
        let mut app = App::new();
        let mut peer = RenetClient::new(data::renet_config(), false);
        peer.set_connected();
        app.add_plugins(MinimalPlugins)
            .insert_resource(peer)
            .init_resource::<State>()
            .add_systems(Update, receive);
        let mut server = RenetServer::new(data::renet_config());
        server.add_connection(7, false);
        let fragment = |id| RubblePose {
            id,
            cell: 72,
            position: [0.0, 1.0, 0.0],
            rotation: Quat::IDENTITY.to_array(),
            boxes: vec![
                destruction::SolidBox {
                    center: Vec3::ZERO,
                    half_size: Vec3::splat(0.02),
                }
                .into(),
            ],
        };
        let base = CoverSnapshot {
            sequence: 1,
            raid_id: 3,
            phase: RaidPhase::Active,
            removed: destroyed_mask(&[]),
            cuts: vec![],
            rubble: vec![fragment(1), fragment(2)],
        };
        let mut samples = vec![(base.clone(), 1)];
        let mut duplicate = base.clone();
        duplicate.sequence = 2;
        duplicate.rubble[1].id = 1;
        samples.push((duplicate, 1));
        let mut invalid = base.clone();
        invalid.sequence = 3;
        invalid.rubble[0].boxes[0].half_size[1] = f32::INFINITY;
        samples.push((invalid, 1));
        let mut mutated = base.clone();
        mutated.sequence = 4;
        mutated.rubble[0].boxes[0].half_size[1] = 0.03;
        samples.push((mutated, 1));
        let mut over = base.clone();
        over.sequence = 5;
        over.rubble[0].boxes =
            vec![over.rubble[0].boxes[0].clone(); destruction::MAX_FRAGMENT_SOLIDS + 1];
        samples.push((over, 1));
        let mut valid = base.clone();
        valid.sequence = 6;
        valid.rubble[0].position[1] = 0.5;
        valid.cuts.push(Cut {
            cell: 72,
            center: [0.0; 3],
            half_size: [0.003; 3],
        });
        samples.push((valid, 6));
        for (snapshot, expected) in samples {
            server.broadcast_message(destruction::CHANNEL, data::encode(&snapshot));
            for packet in server.get_packets_to_send(7).unwrap() {
                app.world_mut()
                    .resource_mut::<RenetClient>()
                    .process_packet(packet);
            }
            app.update();
            let accepted = app.world().resource::<State>().snapshot.as_ref().unwrap();
            assert_eq!(accepted.sequence, expected);
            assert_eq!(accepted.rubble[0].boxes, base.rubble[0].boxes);
            assert_eq!(
                accepted.rubble.len(),
                2,
                "independent fragments may share an original cell"
            );
        }
    }

    #[test]
    fn destruction_full_state_recovers_late_arrival_and_rejects_old_or_duplicate_updates() {
        let mut app = App::new();
        let mut peer = RenetClient::new(data::renet_config(), false);
        peer.set_connected();
        app.add_plugins(MinimalPlugins)
            .insert_resource(peer)
            .init_resource::<State>()
            .add_systems(Update, receive);
        let mut server = RenetServer::new(data::renet_config());
        server.add_connection(7, false);
        for (sequence, cell, expected) in [(4, 72, 72), (2, 73, 72), (4, 73, 72), (5, 74, 74)] {
            let snapshot = CoverSnapshot {
                sequence,
                raid_id: 2,
                phase: RaidPhase::Active,
                removed: destroyed_mask(&[cell]),
                cuts: vec![Cut {
                    cell,
                    center: [0.01, 0.02, 0.0],
                    half_size: [0.003, 0.003, 0.1],
                }],
                rubble: vec![RubblePose {
                    id: cell as u32,
                    cell,
                    position: [1.0, 0.4, 2.0],
                    rotation: Quat::IDENTITY.to_array(),
                    boxes: vec![
                        destruction::SolidBox {
                            center: Vec3::ZERO,
                            half_size: Vec3::splat(0.1),
                        }
                        .into(),
                    ],
                }],
            };
            server.broadcast_message(destruction::CHANNEL, data::encode(&snapshot));
            for packet in server.get_packets_to_send(7).unwrap() {
                app.world_mut()
                    .resource_mut::<RenetClient>()
                    .process_packet(packet);
            }
            app.update();
            let current = app.world().resource::<State>().snapshot.as_ref().unwrap();
            assert_eq!(current.raid_id, 2);
            assert_eq!(current.phase, RaidPhase::Active);
            assert_eq!(current.removed, destroyed_mask(&[expected]));
            assert_eq!(
                current.cuts,
                vec![Cut {
                    cell: expected,
                    center: [0.01, 0.02, 0.0],
                    half_size: [0.003, 0.003, 0.1]
                }]
            );
            assert_eq!(
                current.rubble,
                vec![RubblePose {
                    id: expected as u32,
                    cell: expected,
                    position: [1.0, 0.4, 2.0],
                    rotation: Quat::IDENTITY.to_array(),
                    boxes: vec![
                        destruction::SolidBox {
                            center: Vec3::ZERO,
                            half_size: Vec3::splat(0.1)
                        }
                        .into()
                    ],
                }]
            );
        }
    }
}
