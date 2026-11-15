use bevy::{
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use common::{combat::*, raid::*, *};

use crate::{ClientSystems, sync::ClientSession};

pub struct Plugin;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub(crate) struct InputCapture;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LocalInput>().add_systems(
            Update,
            (cursor_control, keyboard, mouse)
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
) {
    let active = session.playing(id.0);
    if active && !*was_active {
        cursor.grab_mode = CursorGrabMode::Locked;
    }
    if !active {
        cursor.grab_mode = CursorGrabMode::None;
    }
    if active && keyboard.just_pressed(KeyCode::Escape) {
        cursor.grab_mode = if cursor.grab_mode == CursorGrabMode::None {
            CursorGrabMode::Locked
        } else {
            CursorGrabMode::None
        };
    }
    cursor.visible = cursor.grab_mode == CursorGrabMode::None;
    *was_active = active;
}

fn keyboard(
    (keys, buttons): (Res<ButtonInput<KeyCode>>, Res<ButtonInput<MouseButton>>),
    session: Res<ClientSession>,
    id: Res<PlayerId>,
    time: Res<Time<Real>>,
    window: Single<(&Window, &CursorOptions), With<PrimaryWindow>>,
    mut input: ResMut<LocalInput>,
    mut tilt_target: Local<f32>,
) {
    let active = session.playing(id.0)
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
    input.fire = active && buttons.pressed(MouseButton::Left);
    input.aim = active && buttons.pressed(MouseButton::Right);
    if active && buttons.just_pressed(MouseButton::Left) {
        input.fire_pressed_sequence = input.fire_pressed_sequence.wrapping_add(1);
    }
    if active && keys.just_pressed(KeyCode::KeyR) {
        input.reload_sequence = input.reload_sequence.wrapping_add(1);
    }
    if active {
        if keys.just_pressed(KeyCode::KeyQ) {
            *tilt_target = if input.camera.roll < 0.0 { 0.0 } else { 0.3 };
        }
        if keys.just_pressed(KeyCode::KeyE) {
            *tilt_target = if input.camera.roll > 0.0 { 0.0 } else { -0.3 };
        }
        if !keys.pressed(KeyCode::KeyQ) && !keys.pressed(KeyCode::KeyE) && *tilt_target != 0.0 {
            *tilt_target = input.camera.roll;
        }
    } else {
        *tilt_target = 0.0;
    }
    let max_tilt_change = 0.6 * time.delta_secs();
    input.camera.roll +=
        (*tilt_target - input.camera.roll).clamp(-max_tilt_change, max_tilt_change);
    if keys.just_pressed(KeyCode::Enter) {
        input.lobby_sequence = input.lobby_sequence.wrapping_add(1);
    }
    if let Some(local) = session.local(id.0) {
        if keys.just_pressed(KeyCode::Enter)
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
        if session.raid.phase == RaidPhase::Lobby && !local.participant.ready {
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

fn mouse(
    motion: Res<AccumulatedMouseMotion>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    mut input: ResMut<LocalInput>,
) {
    if cursor.grab_mode == CursorGrabMode::None {
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
    fn q_and_e_hold_tilt_gradually_and_opposite_press_returns_to_level() {
        let mut app = input_app();
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

        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(KeyCode::KeyQ);
            keys.clear_just_pressed(KeyCode::KeyQ);
            keys.press(KeyCode::KeyE);
        }
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_secs_f32(0.25));
        app.update();
        assert!((app.world().resource::<LocalInput>().camera.roll - 0.15).abs() < 0.001);
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_secs_f32(0.25));
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
