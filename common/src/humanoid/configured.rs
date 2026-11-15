//! Configuration-aware anatomical pose used before hit testing and physics
//! handoff. Physical/fallen poses remain owned by the articulated body.
use super::*;
use crate::{
    catalog::{MechanicalSnapshot, ResolvedWeapon},
    weapon_animation::{reload_action, WeaponAction},
};
use handling::{HandlingError, HandlingSolution};

pub struct ConfiguredRig {
    pub segments: [SegmentPose; SEGMENT_COUNT],
    pub weapon: Option<HandlingSolution>,
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
    stock_target: Option<Vec3>,
) -> Result<ConfiguredRig, HandlingError> {
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
        });
    }
    let contacts = handling::configured_contacts(spec, action, 0.0, mechanics)?;
    let chest = Vec3::from(segments[Segment::Thorax.index()].position);
    let shoulders = [-1.0, 1.0].map(|side| chest + Vec3::new(side * 0.30, 0.10, 0.0));
    let solved = handling::fit_weapon(
        preferred_root(state, chest, contacts.trigger, action),
        contacts,
        shoulders,
        [Vec3::new(-0.8, -1.0, 0.0), Vec3::new(0.8, -1.0, 0.0)],
        stock_target,
        0.35,
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
    })
}

#[cfg(test)]
mod tests;
