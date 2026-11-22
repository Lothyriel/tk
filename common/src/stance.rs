//! Body envelopes for voluntary player stances, independent of injury mode.
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::{
    PLAYER_COLLIDER_HALF_HEIGHT, PLAYER_COLLIDER_RADIUS, PLAYER_CROUCH_COLLIDER_HALF_HEIGHT,
    PLAYER_CROUCH_VIEW_OFFSET, PLAYER_VIEW_HEIGHT,
};

const PRONE_COLLIDER_HALF_LENGTH: f32 = 0.60;
const PRONE_RADIUS: f32 = 0.23;
const PRONE_EYE_ABOVE_ROOT: f32 = 0.10;
pub const PRONE_CRAWL_SPEED: f32 = 0.85;
pub const PRONE_TRANSITION_DELAY: f32 = 0.25;

/// Local simulation geometry only; this is not a network field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stance {
    Standing,
    Crouched,
    Prone,
}

impl Stance {
    pub fn from_crouched(crouched: bool) -> Self {
        if crouched {
            Self::Crouched
        } else {
            Self::Standing
        }
    }

    pub fn half_height(self) -> f32 {
        match self {
            Self::Standing => PLAYER_COLLIDER_HALF_HEIGHT + PLAYER_COLLIDER_RADIUS,
            Self::Crouched => PLAYER_CROUCH_COLLIDER_HALF_HEIGHT + PLAYER_COLLIDER_RADIUS,
            Self::Prone => PRONE_RADIUS,
        }
    }

    pub fn eye_above_root(self) -> f32 {
        match self {
            Self::Standing => PLAYER_VIEW_HEIGHT,
            Self::Crouched => PLAYER_VIEW_HEIGHT + PLAYER_CROUCH_VIEW_OFFSET,
            Self::Prone => PRONE_EYE_ABOVE_ROOT,
        }
    }

    pub fn eye_offset(self) -> Vec3 {
        Vec3::new(
            0.0,
            self.eye_above_root(),
            if self == Self::Prone { -0.61 } else { 0.0 },
        )
    }

    /// Candidate root preserving the bottom of the current collider. The
    /// caller still has to check clearance before publishing the new stance.
    pub fn centre_from(self, previous: Self, centre: Vec3) -> Vec3 {
        centre + Vec3::Y * (self.half_height() - previous.half_height())
    }

    pub fn collider(self) -> Collider {
        self.shape(0.0)
    }

    /// Inset the radius by the same skin used for existing headroom tests,
    /// so resting on the floor doesn't count as a blocked destination.
    pub fn clearance_shape(self) -> Collider {
        self.shape(0.015)
    }

    fn shape(self, inset: f32) -> Collider {
        match self {
            Self::Standing => {
                Collider::capsule_y(PLAYER_COLLIDER_HALF_HEIGHT, PLAYER_COLLIDER_RADIUS - inset)
            }
            Self::Crouched => Collider::capsule_y(
                PLAYER_CROUCH_COLLIDER_HALF_HEIGHT,
                PLAYER_COLLIDER_RADIUS - inset,
            ),
            Self::Prone => Collider::capsule_z(PRONE_COLLIDER_HALF_LENGTH, PRONE_RADIUS - inset),
        }
    }
}
