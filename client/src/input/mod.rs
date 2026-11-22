use bevy::{
    ecs::system::SystemParam,
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use common::*;

use crate::{ClientSystems, sync::ClientSession};
pub(crate) mod inventory_panel;

pub struct Plugin;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub(crate) struct InputCapture;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub(crate) struct InventoryInput;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LocalInput>()
            .init_resource::<inventory_panel::Panel>()
            .add_systems(
                Update,
                inventory_panel::update
                    .in_set(InventoryInput)
                    .in_set(ClientSystems::Input),
            )
            .add_systems(
                Update,
                (cursor_control, keyboard, mouse)
                    .chain()
                    .after(InventoryInput)
                    .in_set(InputCapture)
                    .in_set(ClientSystems::Input),
            );
    }
}

fn cursor_control(
    session: Res<ClientSession>,
    id: Res<PlayerId>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
    menu: Res<crate::menu::Menu>,
) {
    let captured = session.playing(id.0) && !menu.owns_input();
    cursor.grab_mode = if captured {
        CursorGrabMode::Locked
    } else {
        CursorGrabMode::None
    };
    cursor.visible = !captured;
}

#[derive(SystemParam)]
struct InputOverlays<'w> {
    prone_binding: Option<Res<'w, ProneBinding>>,
    inventory: Option<Res<'w, crate::sync::inventory::State>>,
    panel: Option<Res<'w, inventory_panel::Panel>>,
    menu: Option<Res<'w, crate::menu::Menu>>,
}

/// A press that began on a UI surface must not turn into a held raid action
/// after a deploy/resume, or after closing another overlay.
#[derive(Default)]
struct GameplayPress {
    fire_blocked: bool,
    aim_blocked: bool,
}

fn gameplay_allowed(
    session: &ClientSession,
    id: u64,
    now: f64,
    window: &Window,
    cursor: &CursorOptions,
) -> bool {
    session.playing(id)
        && window.focused
        && cursor.grab_mode != CursorGrabMode::None
        && now - session.last_received < 2.0
}

#[derive(Resource)]
struct ProneBinding(KeyCode);

impl Default for ProneBinding {
    fn default() -> Self {
        let key = std::env::var("TK_PRONE_KEY").unwrap_or_default();
        let letters = [
            KeyCode::KeyA,
            KeyCode::KeyB,
            KeyCode::KeyC,
            KeyCode::KeyD,
            KeyCode::KeyE,
            KeyCode::KeyF,
            KeyCode::KeyG,
            KeyCode::KeyH,
            KeyCode::KeyI,
            KeyCode::KeyJ,
            KeyCode::KeyK,
            KeyCode::KeyL,
            KeyCode::KeyM,
            KeyCode::KeyN,
            KeyCode::KeyO,
            KeyCode::KeyP,
            KeyCode::KeyQ,
            KeyCode::KeyR,
            KeyCode::KeyS,
            KeyCode::KeyT,
            KeyCode::KeyU,
            KeyCode::KeyV,
            KeyCode::KeyW,
            KeyCode::KeyX,
            KeyCode::KeyY,
            KeyCode::KeyZ,
        ];
        let value = match key.as_bytes() {
            [letter] if letter.is_ascii_alphabetic() => {
                letters[usize::from(letter.to_ascii_lowercase() - b'a')]
            }
            _ => KeyCode::KeyZ,
        };
        Self(value)
    }
}

fn keyboard(
    (keys, buttons): (Res<ButtonInput<KeyCode>>, Res<ButtonInput<MouseButton>>),
    (session, id, time): (Res<ClientSession>, Res<PlayerId>, Res<Time<Real>>),
    window: Single<(&Window, &CursorOptions), With<PrimaryWindow>>,
    mut input: ResMut<LocalInput>,
    mut presses: Local<GameplayPress>,
    InputOverlays {
        prone_binding,
        inventory,
        panel,
        menu,
    }: InputOverlays,
) {
    let active = !panel.is_some_and(|panel| panel.open)
        && !menu.is_some_and(|menu| menu.blocks_gameplay())
        && gameplay_allowed(&session, id.0, time.elapsed_secs_f64(), window.0, window.1);
    input.forward = active && keys.pressed(KeyCode::KeyW);
    input.backward = active && keys.pressed(KeyCode::KeyS);
    input.left = active && keys.pressed(KeyCode::KeyA);
    input.right = active && keys.pressed(KeyCode::KeyD);
    input.run = active && keys.pressed(KeyCode::ShiftLeft);
    input.crouch = active && keys.pressed(KeyCode::ControlLeft);
    if active
        && keys.just_pressed(
            prone_binding
                .as_ref()
                .map_or(KeyCode::KeyZ, |binding| binding.0),
        )
    {
        input.prone_sequence = input.prone_sequence.wrapping_add(1);
    }
    input.jump = active && keys.pressed(KeyCode::Space);
    input.interact = active && keys.pressed(KeyCode::KeyF);
    if !input.interact {
        input.pickup_focus = None;
    } else if keys.just_pressed(KeyCode::KeyF) {
        let raid = session.raid.raid_id;
        input.pickup_focus =
            inventory.and_then(|state| state.focused_ground(raid).map(|item| (raid, item.id)));
    }
    let fire = buttons.pressed(MouseButton::Left);
    let aim = buttons.pressed(MouseButton::Right);
    presses.fire_blocked = fire && (!active || presses.fire_blocked);
    presses.aim_blocked = aim && (!active || presses.aim_blocked);
    input.fire = active && fire && !presses.fire_blocked;
    input.aim = active && aim && !presses.aim_blocked;
    if input.fire && buttons.just_pressed(MouseButton::Left) {
        input.fire_pressed_sequence = input.fire_pressed_sequence.wrapping_add(1);
    }
    if active && keys.just_pressed(KeyCode::KeyR) {
        input.reload_sequence = input.reload_sequence.wrapping_add(1);
    }
    if active && keys.just_pressed(KeyCode::KeyI) {
        input.inspect_sequence = input.inspect_sequence.wrapping_add(1);
    }
    if active && keys.just_pressed(KeyCode::KeyB) {
        input.selector_sequence = input.selector_sequence.wrapping_add(1);
    }
    input.camera.roll = step_lean(
        input.camera.roll,
        active && keys.pressed(KeyCode::KeyQ),
        active && keys.pressed(KeyCode::KeyE),
        time.delta_secs(),
    );
    if active && keys.just_pressed(KeyCode::Digit1) {
        input.weapon = EquipmentSlot::Primary;
    }
    if active && keys.just_pressed(KeyCode::Digit2) {
        input.weapon = EquipmentSlot::Sidearm;
    }
}

pub(crate) fn step_lean(roll: f32, left: bool, right: bool, dt: f32) -> f32 {
    let target = match (left, right) {
        (true, false) => 0.3,
        (false, true) => -0.3,
        _ => 0.0,
    };
    let max_change = 0.6 * dt;
    roll + (target - roll).clamp(-max_change, max_change)
}

fn mouse(
    motion: Res<AccumulatedMouseMotion>,
    window: Single<(&Window, &CursorOptions), With<PrimaryWindow>>,
    (session, id, time): (Res<ClientSession>, Res<PlayerId>, Res<Time<Real>>),
    mut input: ResMut<LocalInput>,
    (panel, menu): (
        Option<Res<inventory_panel::Panel>>,
        Option<Res<crate::menu::Menu>>,
    ),
) {
    if panel.is_some_and(|panel| panel.open)
        || menu.is_some_and(|menu| menu.blocks_gameplay())
        || !gameplay_allowed(&session, id.0, time.elapsed_secs_f64(), window.0, window.1)
    {
        return;
    }
    let scale = if input.aim { 0.65 } else { 1.0 };
    input.camera.yaw -= motion.delta.x * 0.003 * scale;
    input.camera.pitch = (input.camera.pitch - motion.delta.y * 0.002 * scale).clamp(-1.56, 1.56);
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::raid::*;

    fn input_app() -> App {
        let mut app = App::new();
        let mut session = ClientSession::default();
        session.players.push(ClientData { id: 7, ..default() });
        app.insert_resource(session)
            .insert_resource(PlayerId(7))
            .init_resource::<LocalInput>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ProneBinding>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<Time<Real>>()
            .add_systems(Update, keyboard);
        app.world_mut().spawn((
            PrimaryWindow,
            Window {
                focused: true,
                ..default()
            },
            CursorOptions {
                grab_mode: CursorGrabMode::Locked,
                ..default()
            },
        ));
        app
    }

    #[test]
    fn raid_inventory_delete_targets_the_displayed_confirmed_item() {
        use bevy_renet2::prelude::{RenetClient, RenetServer};
        use common::{
            catalog::{
                CompartmentId, ItemInstanceId,
                item_grid::{Placement, Rotation},
                item_inventory::{
                    Condition, InventorySnapshot, ItemContents, ProjectedItem, SessionKey,
                    VersionedCompartment,
                },
            },
            replication::inventory::{CHANNEL, InventoryRequest, InventoryUpdate, UpdateBody},
        };
        let mut app = input_app();
        let mut server = RenetServer::new(common::data::renet_config());
        server.add_connection(7, false);
        let mut client = RenetClient::new(common::data::renet_config(), false);
        client.set_connected();
        app.insert_resource(client)
            .init_resource::<inventory_panel::Panel>()
            .init_resource::<crate::sync::inventory::State>()
            .add_systems(Update, inventory_panel::update.before(keyboard));
        let session = SessionKey {
            raid_epoch: 1,
            session_epoch: 1,
        };
        {
            let mut raid = app.world_mut().resource_mut::<ClientSession>();
            raid.raid.raid_id = 1;
            raid.raid.phase = RaidPhase::Active;
            raid.players[0].participant.status = RaidStatus::Active;
            raid.players[0].alive = true;
        }
        let compartment = CompartmentId(7);
        let item = ProjectedItem {
            definition: "med.bandage".into(),
            condition: Condition::Intact,
            quantity: 1,
            contents: ItemContents::Dressing { sealed: true },
            placement: Placement {
                x: 0,
                y: 0,
                rotation: Rotation::Upright,
            },
        };
        let id = ItemInstanceId(1);
        for body in [
            UpdateBody::Begin,
            UpdateBody::Snapshot(InventorySnapshot {
                version: VersionedCompartment {
                    id: compartment,
                    revision: 4,
                },
                size: [4, 4],
                items: vec![(id, item)],
            }),
        ] {
            app.world_mut()
                .resource_mut::<crate::sync::inventory::State>()
                .apply(InventoryUpdate { session, body });
        }
        let press = |app: &mut App, key| {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(key);
        };
        let transfer = |app: &mut App, server: &mut RenetServer| {
            for packet in app
                .world_mut()
                .resource_mut::<RenetClient>()
                .get_packets_to_send()
            {
                server.process_packet_from(packet, 7).unwrap();
            }
        };
        press(&mut app, KeyCode::Delete);
        app.update();
        transfer(&mut app, &mut server);
        assert!(server.receive_message(7, CHANNEL).is_none());
        press(&mut app, KeyCode::Tab);
        app.update();
        assert_eq!(
            app.world().resource::<inventory_panel::Panel>().selected,
            Some(id)
        );
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        press(&mut app, KeyCode::Delete);
        app.update();
        assert!(!app.world().resource::<LocalInput>().fire);
        transfer(&mut app, &mut server);
        let bytes = server.receive_message(7, CHANNEL).unwrap();
        assert_eq!(
            InventoryRequest::decode(&bytes),
            Ok(InventoryRequest::Drop {
                session,
                item: id,
                revision: 4
            })
        );
        assert_eq!(
            app.world()
                .resource::<crate::sync::inventory::State>()
                .owned_grid(1, 7)
                .unwrap()
                .items
                .len(),
            1
        );
    }

    #[test]
    fn right_mouse_aim_is_held_only_during_captured_active_control() {
        let mut app = input_app();
        {
            let mut session = app.world_mut().resource_mut::<ClientSession>();
            session.raid.phase = RaidPhase::Active;
            session.players[0].participant.begin();
        }
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Right);
        app.update();
        assert!(app.world().resource::<LocalInput>().aim);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Right);
        app.update();
        assert!(!app.world().resource::<LocalInput>().aim);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Right);
        app.world_mut().resource_mut::<ClientSession>().raid.phase = RaidPhase::Debrief;
        app.update();
        assert!(!app.world().resource::<LocalInput>().aim);
    }

    #[test]
    fn q_and_e_release_and_reverse_continuously() {
        let mut app = input_app();
        app.init_resource::<AccumulatedMouseMotion>()
            .add_systems(Update, mouse.after(keyboard));
        {
            let mut session = app.world_mut().resource_mut::<ClientSession>();
            session.raid.phase = RaidPhase::Active;
            session.players[0].participant.begin();
        }

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyQ);
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_secs_f32(0.25));
        app.update();
        assert!((app.world().resource::<LocalInput>().camera.roll - 0.15).abs() < 0.001);

        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_secs_f32(0.25));
        app.update();
        assert!((app.world().resource::<LocalInput>().camera.roll - 0.3).abs() < 0.001);

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyQ);
        app.update();
        assert!((app.world().resource::<LocalInput>().camera.roll - 0.15).abs() < 0.001);

        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.clear_just_pressed(KeyCode::KeyQ);
            keys.press(KeyCode::KeyE);
        }
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_secs_f32(0.25));
        app.world_mut()
            .resource_mut::<AccumulatedMouseMotion>()
            .delta = Vec2::new(10.0, -5.0);
        app.update();
        let camera = app.world().resource::<LocalInput>().camera;
        assert!(camera.roll.abs() < 0.001);
        assert!((camera.yaw + 0.03).abs() < 0.00001);
        assert!((camera.pitch - 0.01).abs() < 0.00001);
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_secs_f32(0.25));
        app.update();
        assert!((app.world().resource::<LocalInput>().camera.roll + 0.15).abs() < 0.001);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyE);
        app.update();
        assert!(app.world().resource::<LocalInput>().camera.roll.abs() < 0.001);
    }

    #[test]
    fn lobby_keys_are_not_gameplay_or_hidden_ready_commands() {
        let mut app = input_app();
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            for key in [
                KeyCode::KeyG,
                KeyCode::KeyB,
                KeyCode::KeyH,
                KeyCode::KeyJ,
                KeyCode::Enter,
                KeyCode::KeyW,
                KeyCode::KeyF,
            ] {
                keys.press(key);
            }
        }
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        let input = app.world().resource::<LocalInput>();
        assert_eq!(input.loadout, common::combat::Loadout::default());
        assert_eq!(input.weapon, EquipmentSlot::Primary);
        assert_eq!(input.lobby_sequence, 0);
        assert!(!input.forward);
        assert!(!input.fire);
        assert!(!input.interact);
        let selected = input.loadout;
        app.world_mut().resource_mut::<ClientSession>().players[0]
            .participant
            .ready = true;
        app.update();
        assert_eq!(app.world().resource::<LocalInput>().loadout, selected);
    }

    #[test]
    fn raid_keys_control_weapons_and_escape_releases_held_actions() {
        let mut app = input_app();
        {
            let mut session = app.world_mut().resource_mut::<ClientSession>();
            session.raid.phase = RaidPhase::Active;
            session.players[0].participant.begin();
        }
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            for key in [
                KeyCode::KeyW,
                KeyCode::KeyF,
                KeyCode::KeyR,
                KeyCode::KeyI,
                KeyCode::KeyB,
                KeyCode::Digit2,
                KeyCode::Space,
            ] {
                keys.press(key);
            }
        }
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        let input = app.world().resource::<LocalInput>();
        assert!(input.forward && input.fire && input.interact && input.jump);
        assert_eq!(input.weapon, EquipmentSlot::Sidearm);
        assert_eq!(input.fire_pressed_sequence, 1);
        assert_eq!(input.reload_sequence, 1);
        assert_eq!((input.inspect_sequence, input.selector_sequence), (1, 1));
        let entity = app
            .world_mut()
            .query_filtered::<Entity, With<Window>>()
            .single(app.world())
            .unwrap();
        app.world_mut()
            .get_mut::<CursorOptions>(entity)
            .unwrap()
            .grab_mode = CursorGrabMode::None;
        app.update();
        let input = app.world().resource::<LocalInput>();
        assert!(!input.forward && !input.fire && !input.interact && !input.jump);
        assert_eq!(input.fire_pressed_sequence, 1);
        assert_eq!(input.reload_sequence, 1);
        assert_eq!((input.inspect_sequence, input.selector_sequence), (1, 1));
    }
}
