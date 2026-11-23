//! Directional intent and bounded world-space momentum shared by movement and handling.
use bevy::prelude::*;

use crate::{ClientInput, PLAYER_RUN_SPEED, PLAYER_WALK_SPEED};

pub const STRAFE_SPEED: f32 = 2.8;
pub const BACKWARD_SPEED: f32 = 2.3;
pub const ACCELERATION: f32 = 9.0;
pub const BRAKING: f32 = 14.0;

pub fn intent(input: &ClientInput) -> Vec3 {
    Vec3::new(
        (input.right as i8 - input.left as i8) as f32,
        0.0,
        (input.backward as i8 - input.forward as i8) as f32,
    )
    .normalize_or_zero()
}

pub fn requests_sprint(input: &ClientInput) -> bool {
    input.run && input.forward && !input.backward && !input.crouch
}

/// Elliptical directional envelope; diagonals never gain free speed.
pub fn speed(direction: Vec3, sprinting: bool) -> f32 {
    if direction == Vec3::ZERO {
        return 0.0;
    }
    let longitudinal = if direction.z > 0.0 {
        BACKWARD_SPEED
    } else if sprinting {
        PLAYER_RUN_SPEED
    } else {
        PLAYER_WALK_SPEED
    };
    (direction.x.powi(2) / STRAFE_SPEED.powi(2) + direction.z.powi(2) / longitudinal.powi(2))
        .sqrt()
        .recip()
}

pub fn accelerate(velocity: Vec3, target: Vec3, grounded: bool, dt: f32) -> Vec3 {
    let rate = if grounded {
        if target == Vec3::ZERO || velocity.dot(target) < 0.0 {
            BRAKING
        } else {
            ACCELERATION
        }
    } else if target == Vec3::ZERO {
        0.0
    } else {
        crate::PLAYER_AIR_ACCELERATION * crate::PLAYER_AIR_CONTROL
    };
    velocity + (target - velocity).clamp_length_max(rate * dt)
}
