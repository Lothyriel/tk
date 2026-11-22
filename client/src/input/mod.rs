use bevy::{
    ecs::system::SystemParam,
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use common::{combat::*, raid::*, *};

use crate::{ClientSystems, sync::ClientSession};
pub(crate) mod inventory_panel;

pub struct Plugin;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub(crate) struct InputCapture;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LocalInput>()
            .init_resource::<inventory_panel::Panel>()
            .add_systems(
                Update,
                (inventory_panel::update, cursor_control, keyboard, mouse)
                    .chain()
                    .in_set(InputCapture)
                    .in_set(ClientSystems::Input),
            );
    }
}

fn cursor_control(
    keyboard: Res<ButtonInput<KeyCode>>,
    session: Res<ClientSession>,
    id: Res<PlayerId>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
    mut was_active: Local<bool>,
    panel: Option<Res<inventory_panel::Panel>>,
) {
    let active = session.playing(id.0);
    if active && !*was_active {
        cursor.grab_mode = CursorGrabMode::Locked;
    }
    if !active {
        cursor.grab_mode = CursorGrabMode::None;
    }
    if active
        && keyboard.just_pressed(KeyCode::Escape)
        && !panel.is_some_and(|panel| panel.consumed_escape)
    {
        cursor.grab_mode = if cursor.grab_mode == CursorGrabMode::None {
            CursorGrabMode::Locked
        } else {
            CursorGrabMode::None
        };
    }
    cursor.visible = cursor.grab_mode == CursorGrabMode::None;
    *was_active = active;
}

#[derive(SystemParam)]
struct InputOverlays<'w> {
    editor: Option<Res<'w, crate::loadout::Editor>>,
    inventory: Option<Res<'w, crate::sync::inventory::State>>,
    panel: Option<Res<'w, inventory_panel::Panel>>,
}

fn keyboard(
    (keys, buttons): (Res<ButtonInput<KeyCode>>, Res<ButtonInput<MouseButton>>),
    session: Res<ClientSession>,
    id: Res<PlayerId>,
    time: Res<Time<Real>>,
    window: Single<(&Window, &CursorOptions), With<PrimaryWindow>>,
    mut input: ResMut<LocalInput>,
    InputOverlays {
        editor,
        inventory,
        panel,
    }: InputOverlays,
) {
    let active = !panel.is_some_and(|panel| panel.open)
        && session.playing(id.0)
        && window.0.focused
        && window.1.grab_mode != CursorGrabMode::None
        && time.elapsed_secs_f64() - session.last_received < 2.0;
    input.forward = active && keys.pressed(KeyCode::KeyW);
    input.backward = active && keys.pressed(KeyCode::KeyS);
    input.left = active && keys.pressed(KeyCode::KeyA);
    input.right = active && keys.pressed(KeyCode::KeyD);
    input.run = active && keys.pressed(KeyCode::ShiftLeft);
    input.crouch = active && keys.pressed(KeyCode::ControlLeft);
    input.jump = active && keys.pressed(KeyCode::Space);
    input.interact = active && keys.pressed(KeyCode::KeyF);
    if !input.interact {
        input.pickup_focus = None;
    } else if keys.just_pressed(KeyCode::KeyF) {
        let raid = session.raid.raid_id;
        input.pickup_focus =
            inventory.and_then(|state| state.focused_ground(raid).map(|item| (raid, item.id)));
    }
    input.fire = active && buttons.pressed(MouseButton::Left);
    input.aim = active && buttons.pressed(MouseButton::Right);
    if active && buttons.just_pressed(MouseButton::Left) {
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
    let editing = editor.is_some_and(|editor| editor.visible || editor.pending.is_some());
    if keys.just_pressed(KeyCode::Enter) && !editing {
        input.lobby_sequence = input.lobby_sequence.wrapping_add(1);
    }
    if let Some(local) = session.local(id.0) {
        if keys.just_pressed(KeyCode::Enter)
            && !editing
            && session.raid.phase == RaidPhase::Lobby
            && !local.participant.ready
        {
            input.weapon = EquipmentSlot::Primary;
        }
        if active && keys.just_pressed(KeyCode::Digit1) {
            input.weapon = EquipmentSlot::Primary;
        }
        if active && keys.just_pressed(KeyCode::Digit2) {
            input.weapon = EquipmentSlot::Sidearm;
        }
        if session.raid.phase == RaidPhase::Lobby && !local.participant.ready && !editing {
            if keys.just_pressed(KeyCode::KeyG) {
                let primaries: Vec<_> = common::catalog::catalog()
                    .weapons()
                    .filter(|weapon| weapon.class != WeaponKind::Pistol)
                    .map(|weapon| weapon.definition)
                    .collect();
                if let Some(index) = primaries.iter().position(|id| *id == input.loadout.primary) {
                    input.loadout.primary = primaries[(index + 1) % primaries.len()];
                }
                input.weapon = EquipmentSlot::Primary;
            }
            if keys.just_pressed(KeyCode::KeyB) {
                input.loadout.ammo = if input.loadout.ammo == AmmoKind::Ball {
                    AmmoKind::Ap
                } else {
                    AmmoKind::Ball
                };
            }
            if keys.just_pressed(KeyCode::KeyH) {
                input.loadout.armor = match input.loadout.armor {
                    ArmorKind::None => ArmorKind::Light,
                    ArmorKind::Light => ArmorKind::Heavy,
                    ArmorKind::Heavy => ArmorKind::None,
                };
            }
            if keys.just_pressed(KeyCode::KeyJ) {
                input.loadout.helmet = !input.loadout.helmet;
            }
        }
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
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    mut input: ResMut<LocalInput>,
    panel: Option<Res<inventory_panel::Panel>>,
) {
    if cursor.grab_mode == CursorGrabMode::None || panel.is_some_and(|panel| panel.open) {
        return;
    }
    let scale = if input.aim { 0.65 } else { 1.0 };
    input.camera.yaw -= motion.delta.x * 0.003 * scale;
    input.camera.pitch = (input.camera.pitch - motion.delta.y * 0.002 * scale).clamp(-1.56, 1.56);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input_app() -> App {
        let mut app = App::new();
        let mut session = ClientSession::default();
        session.players.push(ClientData { id: 7, ..default() });
        app.insert_resource(session)
            .insert_resource(PlayerId(7))
            .init_resource::<LocalInput>()
            .init_resource::<ButtonInput<KeyCode>>()
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
    fn lobby_editor_blocks_ready_and_gear_keys_until_closed_and_confirmed() {
        let mut app = input_app();
        app.init_resource::<crate::loadout::Editor>();
        for (visible, pending) in [(true, None), (false, Some(1))] {
            {
                let mut editor = app.world_mut().resource_mut::<crate::loadout::Editor>();
                editor.visible = visible;
                editor.pending = pending;
            }
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            for key in [
                KeyCode::Enter,
                KeyCode::KeyG,
                KeyCode::KeyB,
                KeyCode::KeyH,
                KeyCode::KeyJ,
            ] {
                keys.press(key);
            }
            app.update();
            let input = app.world().resource::<LocalInput>();
            assert_eq!(input.lobby_sequence, 0);
            assert_eq!(input.loadout, Loadout::default());
        }
        app.world_mut()
            .resource_mut::<crate::loadout::Editor>()
            .pending = None;
        app.update();
        assert_eq!(app.world().resource::<LocalInput>().lobby_sequence, 1);
        assert_eq!(
            app.world().resource::<LocalInput>().loadout.primary,
            WeaponId::MP5
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
    fn lobby_keys_select_equipment_and_ready_without_enabling_combat() {
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
        assert_eq!(input.loadout.primary, WeaponId::MP5);
        assert_eq!(input.loadout.ammo, AmmoKind::Ap);
        assert_eq!(input.loadout.armor, ArmorKind::Heavy);
        assert!(!input.loadout.helmet);
        assert_eq!(input.weapon, EquipmentSlot::Primary);
        assert_eq!(input.lobby_sequence, 1);
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

    #[test]
    fn cursor_automatically_tracks_raid_and_escape_toggles_capture() {
        let mut app = input_app();
        app.add_systems(Update, cursor_control.before(keyboard));
        app.update();
        let entity = app
            .world_mut()
            .query_filtered::<Entity, With<Window>>()
            .single(app.world())
            .unwrap();
        assert_eq!(
            app.world().get::<CursorOptions>(entity).unwrap().grab_mode,
            CursorGrabMode::None
        );
        {
            let mut session = app.world_mut().resource_mut::<ClientSession>();
            session.raid.phase = RaidPhase::Active;
            session.players[0].participant.begin();
        }
        app.update();
        assert_eq!(
            app.world().get::<CursorOptions>(entity).unwrap().grab_mode,
            CursorGrabMode::Locked
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert_eq!(
            app.world().get::<CursorOptions>(entity).unwrap().grab_mode,
            CursorGrabMode::None
        );
        assert!(app.world().get::<CursorOptions>(entity).unwrap().visible);
        app.world_mut().resource_mut::<ClientSession>().raid.phase = RaidPhase::Debrief;
        app.update();
        assert_eq!(
            app.world().get::<CursorOptions>(entity).unwrap().grab_mode,
            CursorGrabMode::None
        );
    }
}
