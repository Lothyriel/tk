//! One armed stride layered over the rigid configured rig, in actor-local axes.
use super::{PhysicalMode, PoseState};
use crate::{weapon_animation::WeaponAction, PLAYER_RUN_SPEED, PLAYER_WALK_SPEED};
use bevy::prelude::*;

#[derive(Default)]
pub struct Carry {
    pub weight: f32,
    pub translation: Vec3,
    pub angles: Vec3,
    pub torso: Quat,
}

impl Carry {
    pub fn rotation(&self) -> Quat {
        Quat::from_euler(EulerRot::YXZ, self.angles.y, self.angles.x, self.angles.z)
    }
}

/// Same phase for both grips. A lower support socket (e.g. RVG rather than
/// a handguard hold) needs less lowering and cant to keep the hands in close.
/// The socket determines the pose gains, not the cadence or the contact.
pub fn sample(state: &PoseState, support: Transform, action: WeaponAction) -> Carry {
    if state.prone
        || state.vault != 0
        || state.physical.is_some()
        || !matches!(state.mode, PhysicalMode::Standing | PhysicalMode::Stumbling)
    {
        return Carry::default();
    }
    let speed = state.velocity[0].hypot(state.velocity[2]);
    let ease = crate::weapon_animation::ease;
    let sprint = if state.sprinting {
        ease((speed - PLAYER_WALK_SPEED) / (PLAYER_RUN_SPEED - PLAYER_WALK_SPEED))
    } else {
        0.0
    };
    // Fade against the *presented* action, not its switch/inspect/reload edge.
    let action_weight = (action.root.translation.length() / 0.11)
        .max(action.root.rotation.angle_between(Quat::IDENTITY) / 0.26)
        .max(action.magazine_grip)
        .max(action.control_grip);
    let weight = sprint * (1.0 - ease(state.ads)) * (1.0 - ease(action_weight));
    let lower_grip = ((-support.translation.y - 0.03048) / (0.077 - 0.03048)).clamp(0.0, 1.0);
    let (step, _) = state.phase.sin_cos();
    let impact = (2.0 * state.phase).cos();
    // These signals already come from confirmed local-space acceleration and yaw.
    let side = (state.acceleration[0] / 12.0).clamp(-1.0, 1.0);
    let push = (state.acceleration[1] / 12.0).clamp(-1.0, 1.0);
    let turn = (state.turn_rate / 6.0).clamp(-1.0, 1.0);
    Carry {
        weight,
        translation: Vec3::new(
            0.012 * step - 0.009 * side,
            (-0.040_f32).lerp(-0.027, lower_grip) + 0.009 * impact,
            -0.016 - 0.018 * push,
        ) * weight,
        angles: Vec3::new(
            0.16_f32.lerp(0.11, lower_grip) + 0.023 * impact - 0.035 * push,
            -0.065 * turn,
            (0.09_f32.lerp(0.055, lower_grip) + 0.030 * step) - 0.050 * side,
        ) * weight,
        torso: Quat::from_euler(
            EulerRot::YXZ,
            (0.033 * step - 0.035 * turn) * weight,
            (-0.09 + 0.04 * push) * weight,
            (-0.028 * step - 0.038 * side) * weight,
        ),
    }
}
