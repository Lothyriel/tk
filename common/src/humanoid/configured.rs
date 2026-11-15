//! Configuration-aware anatomical pose used before hit testing and physics
//! handoff. Physical/fallen poses remain owned by the articulated body.
use super::*;
use crate::{
    catalog::{MechanicalSnapshot, ResolvedWeapon},
    weapon_animation::{reload_action, WeaponAction},
};
use handling::{HandlingError, HandlingSolution};

pub const BLADED_YAW: f32 = -1.05;

pub fn stock_pocket(cover: f32) -> Vec3 {
    Vec3::new(0.14, 0.10, -Segment::Thorax.half_size().z - cover)
}

pub fn brace_weights(ads: f32) -> (f32, f32) {
    let ads = ads.clamp(0.0, 1.0);
    (
        crate::weapon_animation::ease((ads * 2.0).min(1.0)),
        crate::weapon_animation::ease((ads * 2.0 - 1.0).max(0.0)),
    )
}

/// The visible chest plate sits at -0.17 m, with a 0.055 m depth and a
/// 1.2 depth scale for heavy armor. Worn clothing is the thorax surface.
pub fn shoulder_cover(armor: &crate::combat::ArmorState) -> f32 {
    if armor.vest <= 0.0 {
        0.0
    } else {
        0.17 + 0.055
            * 0.5
            * if armor.kind == crate::combat::ArmorKind::Heavy {
                1.2
            } else {
                1.0
            }
            - Segment::Thorax.half_size().z
    }
}

#[derive(Clone, Copy)]
pub struct Posture {
    pub preferred_root: Transform,
    pub shoulders: [Vec3; 2],
    pub poles: [Vec3; 2],
    pub stock_target: Option<Vec3>,
    pub max_adjustment: f32,
}

/// A bladed upper-body stance lets the support shoulder advance toward the
/// foregrip. ADS closes the stock onto a chest-local pocket; the whole rigid
/// weapon moves, never either grip independently. Reload/hip poses release it.
pub fn posture(
    state: &PoseState,
    segments: &mut [SegmentPose; SEGMENT_COUNT],
    contacts: handling::WeaponContacts,
    action: WeaponAction,
    cover: Option<f32>,
) -> Result<Posture, HandlingError> {
    let chest = Vec3::from(segments[Segment::Thorax.index()].position);
    let preferred_root = preferred_root(state, chest, contacts.trigger, action);
    let ads = if contacts.stock.is_some() && cover.is_some() {
        state.ads.clamp(0.0, 1.0)
    } else {
        0.0
    };
    // Turn before closing the pocket: a square torso cannot reach the long
    // foregrip with the stock fixed to the shoulder.
    let (turn, brace) = brace_weights(ads);
    let thorax = segments[Segment::Thorax.index()].transform();
    let target_rotation = Quat::from_rotation_x(state.aim_pitch)
        * thorax.rotation
        * Quat::from_rotation_y(BLADED_YAW);
    let shoulder_rotation = Quat::IDENTITY.slerp(target_rotation, turn);
    if turn > 0.0 {
        segments[Segment::Thorax.index()].rotation =
            thorax.rotation.slerp(target_rotation, turn).to_array();
    }
    let shoulders =
        [-1.0, 1.0].map(|side| chest + shoulder_rotation * Vec3::new(side * 0.30, 0.10, 0.0));
    let poles = [Vec3::new(-0.8, -1.0, 0.0), Vec3::new(0.8, -1.0, 0.0)]
        .map(|pole| shoulder_rotation * pole);
    let stock_target = if let Some(stock) = contacts.stock.filter(|_| brace > 0.0) {
        let free = handling::fit_weapon(preferred_root, contacts, shoulders, poles, None, 0.35)?;
        let pocket = chest + shoulder_rotation * stock_pocket(cover.unwrap_or(0.0));
        Some(
            free.root
                .transform_point(stock.translation)
                .lerp(pocket, brace),
        )
    } else {
        None
    };
    Ok(Posture {
        preferred_root,
        shoulders,
        poles,
        stock_target,
        max_adjustment: if ads > 0.0 { 0.50 } else { 0.35 },
    })
}

pub struct ConfiguredRig {
    pub segments: [SegmentPose; SEGMENT_COUNT],
    pub weapon: Option<HandlingSolution>,
    pub posture: Option<Posture>,
}

/// Unconstrained preference; `fit_weapon` checks configured contacts as one rigid
/// assembly. No legacy grip clamp runs before this solve.
pub fn preferred_root(
    state: &PoseState,
    chest: Vec3,
    trigger: Transform,
    action: WeaponAction,
) -> Transform {
    let aim = Quat::from_rotation_x(state.aim_pitch);
    let rotation = aim * action.root.rotation;
    Transform::from_translation(
        chest
            + Vec3::Y * (0.13 + state.ads * 0.07)
            + aim
                * (Vec3::new(0.10, 0.0, -0.18)
                    .lerp(Vec3::new(0.035, 0.0, -0.20), state.ads.clamp(0.0, 1.0))
                    + action.root.translation)
            - rotation * trigger.translation,
    )
    .with_rotation(rotation)
}

pub fn evaluate(
    state: &PoseState,
    legs: [f32; 2],
    spec: &ResolvedWeapon,
    mechanics: &MechanicalSnapshot,
    action: Option<WeaponAction>,
    cover: Option<f32>,
) -> Result<ConfiguredRig, HandlingError> {
    if cover.is_some_and(|v| !v.is_finite() || !(0.0..=0.10).contains(&v)) {
        return Err(HandlingError::InvalidInput);
    }
    let action = action.unwrap_or_else(|| {
        reload_action(
            spec.class,
            if mechanics.reloading() {
                mechanics.fraction
            } else {
                0.0
            },
            mechanics.kind == Some(crate::catalog::ManipulationKind::EmptyReload),
        )
    });
    let mut segments = evaluate_pose_with_action(state, legs[0], legs[1], Some(action));
    if state.physical.is_some()
        || !matches!(state.mode, PhysicalMode::Standing | PhysicalMode::Stumbling)
    {
        return Ok(ConfiguredRig {
            segments,
            weapon: None,
            posture: None,
        });
    }
    let contacts = handling::configured_contacts(spec, action, 0.0, mechanics)?;
    let posture = posture(state, &mut segments, contacts, action, cover)?;
    let shoulders = posture.shoulders;
    let solved = handling::fit_weapon(
        posture.preferred_root,
        contacts,
        shoulders,
        posture.poles,
        posture.stock_target,
        posture.max_adjustment,
    )?;
    for (arm, shoulder, ids) in [
        (
            solved.support,
            shoulders[0],
            [
                Segment::LeftUpperArm,
                Segment::LeftForearm,
                Segment::LeftHand,
            ],
        ),
        (
            solved.trigger,
            shoulders[1],
            [
                Segment::RightUpperArm,
                Segment::RightForearm,
                Segment::RightHand,
            ],
        ),
    ] {
        for (id, pose) in ids.into_iter().zip(arm.segments(shoulder)) {
            segments[id.index()] = pose;
        }
    }
    Ok(ConfiguredRig {
        segments,
        weapon: Some(solved),
        posture: Some(posture),
    })
}

#[cfg(test)]
mod tests;
