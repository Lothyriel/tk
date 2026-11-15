//! Rigid weapon contacts and anatomical two-bone arms, in one caller-selected
//! frame. Configured presentation must handle rejection, never stretch a limb
//! or silently move a wrist away from the requested contact.
use bevy::prelude::*;

use crate::catalog::{AnchorKind, ResolvedAssembly};

mod configured;
pub use configured::{configured_contacts, mechanisms, Mechanisms};

pub const UPPER_ARM_METRES: f32 = 0.30;
pub const FOREARM_METRES: f32 = 0.28;
// Keep the established rig's bend limits, away from folded/straight singularities.
pub const MIN_REACH_METRES: f32 = 0.10;
pub const MAX_REACH_METRES: f32 = 0.575;
const CONTACT_EPSILON: f32 = 0.000001;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hand {
    Support,
    Trigger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandlingError {
    MissingAnchor(AnchorKind),
    NonRigidTransform,
    InvalidInput,
    Unreachable(Hand),
    RootAdjustmentExceeded,
}

#[derive(Debug, Clone, Copy)]
pub struct WeaponContacts {
    pub support: Transform,
    pub trigger: Transform,
    pub stock: Option<Transform>,
}

impl WeaponContacts {
    /// Resolve once per assembly revision. Root-local orientations are as
    /// important as positions: a rotated foregrip must rotate the support palm.
    pub fn from_assembly(assembly: &ResolvedAssembly) -> Result<Self, HandlingError> {
        let anchor = |kind| {
            assembly
                .anchors
                .get(&kind)
                .copied()
                .ok_or(HandlingError::MissingAnchor(kind))
        };
        let contacts = Self {
            support: anchor(AnchorKind::SupportGrip)?,
            trigger: anchor(AnchorKind::TriggerGrip)?,
            stock: assembly.anchors.get(&AnchorKind::StockContact).copied(),
        };
        contacts.validate()?;
        Ok(contacts)
    }

    /// Use the current magazine/control socket after its mechanical transform,
    /// expressed in weapon-root space. Trigger grip and stock remain fixed.
    pub fn with_support(mut self, contact: Transform) -> Result<Self, HandlingError> {
        rigid(contact)?;
        self.support = contact;
        Ok(self)
    }

    fn validate(self) -> Result<(), HandlingError> {
        rigid(self.support)?;
        rigid(self.trigger)?;
        if let Some(stock) = self.stock {
            rigid(stock)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ArmSolution {
    pub elbow: Vec3,
    pub wrist: Transform,
}

impl ArmSolution {
    /// Anatomical upper-arm, forearm and hand poses. Rendering and tissue
    /// volumes use the same segment centres/orientations, without bone scaling.
    pub fn segments(self, shoulder: Vec3) -> [super::SegmentPose; 3] {
        [
            super::bone(shoulder, self.elbow),
            super::bone(self.elbow, self.wrist.translation),
            super::SegmentPose {
                position: self.wrist.translation.to_array(),
                rotation: self.wrist.rotation.to_array(),
            },
        ]
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HandlingSolution {
    pub root: Transform,
    pub support: ArmSolution,
    pub trigger: ArmSolution,
}

fn rigid(pose: Transform) -> Result<(), HandlingError> {
    if !pose.translation.is_finite()
        || !pose.rotation.is_finite()
        || !pose.scale.is_finite()
        || (pose.rotation.length_squared() - 1.0).abs() > 1e-5
        || pose.scale.distance(Vec3::ONE) > 1e-6
    {
        return Err(HandlingError::NonRigidTransform);
    }
    Ok(())
}

/// Shared by the legacy rig after its existing reach clamp, and by the checked
/// configured solver below. Only callers which established reach may use this.
pub(super) fn elbow_for_reachable(shoulder: Vec3, wrist: Vec3, pole: Vec3) -> Vec3 {
    let delta = wrist - shoulder;
    let distance = delta.length();
    let axis = delta / distance;
    let along = (UPPER_ARM_METRES.powi(2) - FOREARM_METRES.powi(2) + distance * distance)
        / (2.0 * distance);
    let bend = (UPPER_ARM_METRES.powi(2) - along * along).max(0.0).sqrt();
    let projected = pole - axis * pole.dot(axis);
    // A collinear pole has no preferred bend plane. Choose a deterministic
    // perpendicular axis instead of collapsing both bones onto one line.
    let down = if projected.length_squared() > 1e-10 * pole.length_squared() {
        projected.normalize()
    } else {
        axis.any_orthonormal_vector()
    };
    shoulder + axis * along + down * bend
}

pub fn solve_arm(
    hand: Hand,
    shoulder: Vec3,
    wrist: Transform,
    pole: Vec3,
) -> Result<ArmSolution, HandlingError> {
    rigid(wrist)?;
    if !shoulder.is_finite()
        || !pole.is_finite()
        || !pole.length_squared().is_finite()
        || pole.length_squared() < 1e-12
    {
        return Err(HandlingError::InvalidInput);
    }
    let distance = wrist.translation.distance(shoulder);
    if !distance.is_finite()
        || !(MIN_REACH_METRES - CONTACT_EPSILON..=MAX_REACH_METRES + CONTACT_EPSILON)
            .contains(&distance)
    {
        return Err(HandlingError::Unreachable(hand));
    }
    Ok(ArmSolution {
        elbow: elbow_for_reachable(shoulder, wrist.translation, pole),
        wrist,
    })
}

/// Shoulder/pole arrays are ordered support, trigger. Root rotation is kept:
/// aim and the whole attachment tree remain one rigid unit. Translate within a
/// bounded budget to meet both arms, then verify exact contacts and lengths.
///
/// When braced, stock_target is the shoulder/clothing surface point chosen by
/// the caller. It pins the authored StockContact exactly; an incompatible grip
/// is rejected instead of pulling the stock away or stretching the forearm.
pub fn fit_weapon(
    preferred_root: Transform,
    contacts: WeaponContacts,
    shoulders: [Vec3; 2],
    poles: [Vec3; 2],
    stock_target: Option<Vec3>,
    max_adjustment: f32,
) -> Result<HandlingSolution, HandlingError> {
    rigid(preferred_root)?;
    contacts.validate()?;
    if !max_adjustment.is_finite()
        || max_adjustment < 0.0
        || shoulders.iter().chain(&poles).any(|v| !v.is_finite())
        || poles
            .iter()
            .any(|p| !p.length_squared().is_finite() || p.length_squared() < 1e-12)
    {
        return Err(HandlingError::InvalidInput);
    }
    let mut root = preferred_root;
    if let Some(target) = stock_target {
        if !target.is_finite() {
            return Err(HandlingError::InvalidInput);
        }
        let stock = contacts
            .stock
            .ok_or(HandlingError::MissingAnchor(AnchorKind::StockContact))?;
        root.translation = target - root.rotation * stock.translation;
    } else {
        let offsets = [contacts.support.translation, contacts.trigger.translation];
        // Bounded alternating projection. Nonconvergence is a conservative
        // rejection; never return an almost-reachable but disconnected pose.
        for _ in 0..64 {
            for ((shoulder, offset), pole) in shoulders.into_iter().zip(offsets).zip(poles) {
                let delta = root.transform_point(offset) - shoulder;
                let distance = delta.length();
                if !distance.is_finite() {
                    return Err(HandlingError::InvalidInput);
                }
                let clamped = distance.clamp(MIN_REACH_METRES, MAX_REACH_METRES);
                if distance != clamped {
                    let axis = delta.try_normalize().unwrap_or_else(|| pole.normalize());
                    root.translation += axis * (clamped - distance);
                }
            }
            if shoulders.into_iter().zip(offsets).all(|(s, o)| {
                (MIN_REACH_METRES - CONTACT_EPSILON..=MAX_REACH_METRES + CONTACT_EPSILON)
                    .contains(&root.transform_point(o).distance(s))
            }) {
                break;
            }
        }
    }
    if root.translation.distance(preferred_root.translation) > max_adjustment + CONTACT_EPSILON {
        return Err(HandlingError::RootAdjustmentExceeded);
    }
    let support = solve_arm(
        Hand::Support,
        shoulders[0],
        root.mul_transform(contacts.support),
        poles[0],
    )?;
    let trigger = solve_arm(
        Hand::Trigger,
        shoulders[1],
        root.mul_transform(contacts.trigger),
        poles[1],
    )?;
    Ok(HandlingSolution {
        root,
        support,
        trigger,
    })
}

#[cfg(test)]
mod tests;
