use std::f32::consts::FRAC_PI_2;

use bevy::{
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    window::{CursorGrabMode, PrimaryWindow},
};
use common::{CameraInput, ClientInput};

pub struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ClientInput>()
            .init_resource::<CameraSensitivity>()
            .add_systems(Startup, setup_cursor)
            .add_systems(Update, (keyboard, mouse));
    }
}

fn setup_cursor(q: Single<&mut Window, With<PrimaryWindow>>) {
    let mut window = q.into_inner();

    window.cursor_options.grab_mode = CursorGrabMode::Locked;
    window.cursor_options.visible = false;
}

fn keyboard(keyboard: Res<ButtonInput<KeyCode>>, mut input: ResMut<ClientInput>) {
    input.forward = keyboard.pressed(KeyCode::KeyW);
    input.left = keyboard.pressed(KeyCode::KeyA);
    input.backward = keyboard.pressed(KeyCode::KeyS);
    input.right = keyboard.pressed(KeyCode::KeyD);

    input.jump = keyboard.pressed(KeyCode::Space);

    const MAX_ROLL_ANGLE: f32 = 0.3;

    let left = keyboard.pressed(KeyCode::KeyQ) as i8;
    let right = keyboard.pressed(KeyCode::KeyE) as i8;

    input.camera.roll = (left - right) as f32 * MAX_ROLL_ANGLE;
}

fn mouse(
    accumulated_mouse_motion: Res<AccumulatedMouseMotion>,
    sense: Res<CameraSensitivity>,
    mut input: ResMut<ClientInput>,
) {
    let delta = accumulated_mouse_motion.delta;

    if delta == Vec2::ZERO {
        return;
    }

    // Note that we are not multiplying by delta_time here.
    // The reason is that for mouse movement, we already get the full movement that happened since the last frame.
    // This means that if we multiply by delta_time, we will get a smaller rotation than intended by the user.
    // This situation is reversed when reading e.g. analog input from a gamepad however, where the same rules
    // as for keyboard input apply. Such an input should be multiplied by delta_time to get the intended rotation
    // independent of the framerate.
    let delta_yaw = -delta.x * sense.x;
    let delta_pitch = -delta.y * sense.y;

    let CameraInput { yaw, pitch, roll } = input.camera;
    let yaw = yaw + delta_yaw;

    // If the pitch was +- 1/2 pi, the camera would look straight up or down.
    // When the user wants to move the camera back to the horizon, which way should the camera face?
    // The camera has no way of knowing what direction was "forward" before landing in that extreme position,
    // so the direction picked will for all intents and purposes be arbitrary.
    // Another issue is that for mathematical reasons, the yaw will effectively be flipped when the pitch is at the extremes.
    // To not run into these issues, we clamp the pitch to a safe range.
    const PITCH_LIMIT: f32 = FRAC_PI_2 - 0.01;

    let pitch = (pitch + delta_pitch).clamp(-PITCH_LIMIT, PITCH_LIMIT);

    input.camera = CameraInput { yaw, pitch, roll };
}

#[derive(Debug, Resource, Deref)]
pub struct CameraSensitivity(Vec2);

impl Default for CameraSensitivity {
    fn default() -> Self {
        Self(
            // These factors are just arbitrary mouse sensitivity values.
            // It's often nicer to have a faster horizontal sensitivity than vertical.
            // We use a component for them so that we can make them user-configurable at runtime
            // for accessibility reasons.
            // It also allows you to inspect them in an editor if you `Reflect` the component.
            Vec2::new(0.003, 0.002),
        )
    }
}
