//! Rigid external manipulation tracks. The mechanical snapshot is the only clock;
//! these curves never alter ammunition, controls, or attachment identity.
use super::*;
use crate::catalog::{ManipulationKind, MechanicalSnapshot};

pub fn action(weapon: WeaponKind, mechanics: &MechanicalSnapshot) -> WeaponAction {
    if mechanics.reloading() {
        return reload_action(
            weapon,
            mechanics.fraction,
            mechanics.kind == Some(ManipulationKind::EmptyReload),
        );
    }
    let mut action = reload_action(weapon, 0.0, false);
    let t = mechanics.fraction.clamp(0.0, 1.0);
    match mechanics.kind {
        Some(ManipulationKind::Inspect) => {
            // Present one side, dwell, cross to the other side, dwell, regrip.
            // Translation and rotation share eased keys with zero endpoint speed.
            let keys = [
                (0.0, Vec3::ZERO, Vec3::ZERO),
                (
                    0.23,
                    Vec3::new(-0.06, 0.035, 0.025),
                    Vec3::new(0.12, -0.65, 0.38),
                ),
                (
                    0.38,
                    Vec3::new(-0.06, 0.035, 0.025),
                    Vec3::new(0.12, -0.65, 0.38),
                ),
                (
                    0.66,
                    Vec3::new(-0.03, 0.045, 0.015),
                    Vec3::new(0.08, 0.65, -0.38),
                ),
                (
                    0.78,
                    Vec3::new(-0.03, 0.045, 0.015),
                    Vec3::new(0.08, 0.65, -0.38),
                ),
                (1.0, Vec3::ZERO, Vec3::ZERO),
            ];
            for pair in keys.windows(2) {
                if t <= pair[1].0 {
                    let blend = ramp(t, pair[0].0, pair[1].0);
                    let angles = pair[0].2.lerp(pair[1].2, blend);
                    action.root = Transform::from_translation(pair[0].1.lerp(pair[1].1, blend))
                        .with_rotation(Quat::from_euler(
                            EulerRot::XYZ,
                            angles.x,
                            angles.y,
                            angles.z,
                        ));
                    break;
                }
            }
        }
        Some(ManipulationKind::Unequip | ManipulationKind::Equip) => {
            let lowered = if mechanics.kind == Some(ManipulationKind::Unequip) {
                ease(t)
            } else {
                1.0 - ease(t)
            };
            action.root = Transform::from_translation(Vec3::new(0.0, -0.34, 0.0) * lowered)
                .with_rotation(Quat::from_rotation_x(0.6 * lowered));
        }
        _ => {}
    }
    action
}

pub fn lowering(mechanics: &MechanicalSnapshot) -> bool {
    matches!(
        mechanics.kind,
        Some(ManipulationKind::Equip | ManipulationKind::Unequip)
    )
}
