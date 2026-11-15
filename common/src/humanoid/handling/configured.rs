//! Logical moving contacts. Server anatomy and client piece motion share these
//! deltas; neither needs render assets to interpret the accepted configuration.
use super::*;
use crate::{
    catalog::{ComponentKind, MechanicalSnapshot, ResolvedWeapon},
    weapon_animation::*,
};

pub struct Mechanisms {
    pub bolt: Transform,
    pub control: Transform,
    /// Stable node, attachment placement and attachment-local mechanical delta.
    pub magazine: Option<(u16, Transform, Transform)>,
}

pub fn mechanisms(
    spec: &ResolvedWeapon,
    action: WeaponAction,
    recoil: f32,
    mechanics: &MechanicalSnapshot,
) -> Mechanisms {
    let mut bolt = bolt_transform(spec.class, &action, recoil, mechanics.reloading());
    if matches!(
        spec.class,
        crate::WeaponKind::Pistol | crate::WeaponKind::Rifle
    ) && mechanics.bolt_open
        && !mechanics.reloading()
    {
        bolt.translation.z = WeaponRig::for_weapon(spec.class).bolt_travel;
    }
    let magazine = spec
        .assembly
        .parts
        .iter()
        .find(|p| p.kind == ComponentKind::Magazine)
        .map(|part| {
            let mut delta = action.magazine;
            delta.translation -= WeaponRig::for_weapon(spec.class).magazine;
            (part.node, part.pose, delta)
        });
    Mechanisms {
        bolt,
        control: control_transform(spec.class, &action),
        magazine,
    }
}

pub fn configured_contacts(
    spec: &ResolvedWeapon,
    action: WeaponAction,
    recoil: f32,
    mechanics: &MechanicalSnapshot,
) -> Result<WeaponContacts, HandlingError> {
    let mut contacts = WeaponContacts::from_assembly(&spec.assembly)?;
    let parts = mechanisms(spec, action, recoil, mechanics);
    let anchor = |kind| {
        spec.assembly
            .anchors
            .get(&kind)
            .copied()
            .ok_or(HandlingError::MissingAnchor(kind))
    };
    for (weight, kind) in [
        (action.magazine_grip, AnchorKind::MagazineGrip),
        (action.control_grip, AnchorKind::ControlGrip),
    ] {
        if !weight.is_finite() || !(0.0..=1.0).contains(&weight) {
            return Err(HandlingError::InvalidInput);
        }
        if weight == 0.0 {
            continue;
        }
        let rest = anchor(kind)?;
        let moving = if kind == AnchorKind::MagazineGrip {
            let (_, placement, delta) = parts
                .magazine
                .ok_or(HandlingError::MissingAnchor(AnchorKind::MagazineGrip))?;
            // The resolved anchor is root-local. Conjugating the local delta
            // preserves a relocated or rotated attachment and its authored grip.
            placement
                .mul_transform(delta)
                .mul_transform(Transform::from_matrix(placement.to_matrix().inverse()))
                .mul_transform(rest)
        } else {
            let motion = if spec.class == crate::WeaponKind::Rifle {
                parts.control
            } else {
                parts.bolt
            };
            // The renderer applies delta after the authored part pivot. Use
            // the same conjugation here, including off-origin rotating catches.
            let pivot = spec
                .assembly
                .pivots
                .get(if spec.class == crate::WeaponKind::Rifle {
                    "control"
                } else {
                    "bolt"
                })
                .copied()
                .unwrap_or(Transform::IDENTITY);
            pivot
                .mul_transform(motion)
                .mul_transform(Transform::from_matrix(pivot.to_matrix().inverse()))
                .mul_transform(rest)
        };
        contacts = contacts.with_support(
            Transform::from_translation(
                contacts
                    .support
                    .translation
                    .lerp(moving.translation, weight),
            )
            .with_rotation(contacts.support.rotation.slerp(moving.rotation, weight)),
        )?;
    }
    if spec.definition == crate::WeaponId::M4A1 {
        let mut support = contacts.support;
        support.translation += m4a1::support_clearance(&action);
        contacts = contacts.with_support(support)?;
    }
    Ok(contacts)
}
