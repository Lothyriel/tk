//! Inventory controls. The confirmed replica remains the only item list.
use bevy::{prelude::*, window::PrimaryWindow};
use bevy_renet2::prelude::RenetClient;
use common::{PlayerId, catalog::ItemInstanceId, replication::inventory::CHANNEL};

use crate::sync::{ClientSession, inventory::State};
pub(super) mod grid;

#[derive(Default, Resource)]
pub(crate) struct Panel {
    pub open: bool,
    pub consumed_escape: bool,
    pub selected: Option<ItemInstanceId>,
    pub inspection: Option<Inspection>,
    drag: Option<grid::Drag>,
    pending: Option<grid::Pending>,
    feedback: Option<&'static str>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Inspection {
    pub item: ItemInstanceId,
    pub yaw_steps: i8,
    pub pitch_steps: i8,
    pub zoom_steps: i8,
}

pub(super) fn update(
    keys: Res<ButtonInput<KeyCode>>,
    (session, player, time): (Res<ClientSession>, Res<PlayerId>, Res<Time<Real>>),
    window: Single<&Window, With<PrimaryWindow>>,
    (mut inventory, mut client): (ResMut<State>, ResMut<RenetClient>),
    mut panel: ResMut<Panel>,
    (menu, pointer): (Option<Res<crate::menu::Menu>>, grid::PointerInput),
) {
    panel.consumed_escape = false;
    if !window.focused
        || menu.is_some_and(|menu| menu.paused())
        || !client.is_connected()
        || !session.playing(player.0)
        || time.elapsed_secs_f64() - session.last_received >= 2.0
    {
        *panel = Panel::default();
        return;
    }
    if panel.open && keys.just_pressed(KeyCode::Escape) {
        if panel.inspection.take().is_some() || panel.drag.take().is_some() {
            panel.consumed_escape = true;
            return;
        }
        *panel = Panel {
            consumed_escape: true,
            ..default()
        };
        return;
    }
    if keys.just_pressed(KeyCode::Tab) {
        if panel.open {
            *panel = Panel::default();
            return;
        }
        panel.open = true;
    }
    if !panel.open {
        return;
    }
    let raid = session.raid.raid_id;
    grid::reconcile(&mut panel, &inventory, raid, player.0);
    if panel.inspection.is_none()
        && grid::interact(
            &mut panel,
            &mut inventory,
            &mut client,
            raid,
            player.0,
            &keys,
            &pointer,
        )
    {
        return;
    }
    let Some(grid) = inventory
        .owned_grid(raid, player.0)
        .filter(|grid| !grid.items.is_empty())
    else {
        panel.selected = None;
        panel.inspection = None;
        return;
    };
    if let Some(mut inspect) = panel.inspection {
        if !grid.items.iter().any(|(id, _)| *id == inspect.item) || keys.just_pressed(KeyCode::KeyI)
        {
            panel.inspection = None;
            return;
        }
        let axis = |positive, negative| {
            i8::from(keys.just_pressed(positive)) - i8::from(keys.just_pressed(negative))
        };
        inspect.yaw_steps =
            (inspect.yaw_steps + axis(KeyCode::ArrowRight, KeyCode::ArrowLeft)).rem_euclid(12);
        inspect.pitch_steps =
            (inspect.pitch_steps + axis(KeyCode::ArrowUp, KeyCode::ArrowDown)).clamp(-2, 2);
        inspect.zoom_steps =
            (inspect.zoom_steps + axis(KeyCode::Equal, KeyCode::Minus)).clamp(-2, 2);
        if keys.just_pressed(KeyCode::KeyR) {
            inspect = Inspection {
                item: inspect.item,
                yaw_steps: 0,
                pitch_steps: 0,
                zoom_steps: 0,
            };
        }
        panel.inspection = Some(inspect);
        return;
    }
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
    if keys.just_pressed(KeyCode::KeyI) && previous == Some(index) {
        panel.inspection = Some(Inspection {
            item: grid.items[index].0,
            yaw_steps: 0,
            pitch_steps: 0,
            zoom_steps: 0,
        });
        return;
    }
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
