//! Keyboard inventory controls. The confirmed replica remains the only item list.
use bevy::{prelude::*, window::PrimaryWindow};
use bevy_renet2::prelude::RenetClient;
use common::{PlayerId, catalog::ItemInstanceId, replication::inventory::CHANNEL};

use crate::sync::{ClientSession, inventory::State};

#[derive(Default, Resource)]
pub(crate) struct Panel {
    pub open: bool,
    pub consumed_escape: bool,
    pub selected: Option<ItemInstanceId>,
}

pub(super) fn update(
    keys: Res<ButtonInput<KeyCode>>,
    session: Res<ClientSession>,
    player: Res<PlayerId>,
    time: Res<Time<Real>>,
    window: Single<&Window, With<PrimaryWindow>>,
    (inventory, client): (Res<State>, ResMut<RenetClient>),
    mut panel: ResMut<Panel>,
) {
    let mut client = client;
    panel.consumed_escape = false;
    if !window.focused
        || !client.is_connected()
        || !session.playing(player.0)
        || time.elapsed_secs_f64() - session.last_received >= 2.0
    {
        *panel = Panel::default();
        return;
    }
    if panel.open && keys.just_pressed(KeyCode::Escape) {
        *panel = Panel {
            consumed_escape: true,
            ..default()
        };
        return;
    }
    if keys.just_pressed(KeyCode::Tab) {
        panel.open = !panel.open;
        panel.selected = None;
        if !panel.open {
            return;
        }
    }
    if !panel.open {
        return;
    }
    let raid = session.raid.raid_id;
    let Some(grid) = inventory
        .owned_grid(raid, player.0)
        .filter(|grid| !grid.items.is_empty())
    else {
        panel.selected = None;
        return;
    };
    let previous = panel
        .selected
        .and_then(|id| grid.items.iter().position(|(other, _)| id == *other));
    let index = match previous {
        Some(index) if keys.just_pressed(KeyCode::ArrowDown) => (index + 1) % grid.items.len(),
        Some(index) if keys.just_pressed(KeyCode::ArrowUp) => {
            (index + grid.items.len() - 1) % grid.items.len()
        }
        Some(index) => index,
        None => 0,
    };
    panel.selected = Some(grid.items[index].0);
    // An item disappearing in this frame must not redirect Delete to another
    // item. Only discard the selection that was already presented.
    if keys.just_pressed(KeyCode::Delete)
        && previous == Some(index)
        && let Some(request) = inventory.drop_request(raid, player.0, grid.items[index].0)
        && let Ok(bytes) = request.encode()
        && client.can_send_message(CHANNEL, bytes.len())
    {
        client.send_message(CHANNEL, bytes);
    }
}
