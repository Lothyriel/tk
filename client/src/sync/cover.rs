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

pub(super) fn receive(mut client: ResMut<RenetClient>, mut state: ResMut<State>) {
    while let Some(bytes) = client.receive_message(destruction::CHANNEL) {
        let Ok(snapshot) = data::try_decode::<CoverSnapshot>(&bytes) else {
            continue;
        };
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
                    half_size: [0.003, 0.003, 0.2],
                }],
                rubble: vec![RubblePose {
                    cell,
                    position: [1.0, 0.4, 2.0],
                    rotation: Quat::IDENTITY.to_array(),
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
                    half_size: [0.003, 0.003, 0.2]
                }]
            );
            assert_eq!(
                current.rubble,
                vec![RubblePose {
                    cell: expected,
                    position: [1.0, 0.4, 2.0],
                    rotation: Quat::IDENTITY.to_array()
                }]
            );
        }
    }
}
