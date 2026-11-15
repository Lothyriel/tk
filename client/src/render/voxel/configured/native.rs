//! Adapter from the existing authoritative action clock to configured parts.
use super::*;
use common::{
    catalog::{ComponentKind, ResolvedWeapon, catalog},
    weapon_animation::*,
};

/// The rest pose comes from the resolved node, while the action delta is relative
/// to the profile's original magazine. It therefore follows a replaced/rotated
/// magazine socket instead of resetting the attachment to a model-specific origin.
pub(crate) fn action(
    spec: &ResolvedWeapon,
    action: WeaponAction,
    recoil: f32,
    reloading: bool,
    magazine_inserted: bool,
) -> ConfiguredHandling {
    let mut motions = vec![
        PartMotion {
            node: 0,
            part: "bolt".into(),
            delta: bolt_transform(spec.class, &action, recoil, reloading),
        },
        PartMotion {
            node: 0,
            part: "control".into(),
            delta: control_transform(spec.class, &action),
        },
    ];
    let mut support_blends = Vec::new();
    let mut hidden_parts = Vec::new();
    if let Some(magazine) = spec.assembly.parts.iter().find(|part| {
        catalog()
            .component(part.definition)
            .is_some_and(|c| c.kind == ComponentKind::Magazine)
    }) {
        let mut delta = action.magazine;
        delta.translation -= WeaponRig::for_weapon(spec.class).magazine;
        motions.push(PartMotion {
            node: magazine.node,
            part: "magazine".into(),
            delta,
        });
        support_blends.push(SocketBlend {
            address: SocketAddress {
                node: magazine.node,
                socket: "magazine.grasp".into(),
            },
            weight: action.magazine_grip,
        });
        if !action.magazine_visible || (!magazine_inserted && !reloading) {
            hidden_parts.push((magazine.node, "magazine".into()));
        }
    }
    support_blends.push(SocketBlend {
        address: SocketAddress {
            node: 0,
            socket: "control.grasp".into(),
        },
        weight: action.control_grip,
    });
    ConfiguredHandling {
        preferred_root: Transform::IDENTITY,
        shoulders: [Vec3::new(-0.30, 0.1, 0.0), Vec3::new(0.30, 0.1, 0.0)],
        poles: [Vec3::new(-0.8, -1.0, 0.0), Vec3::new(0.8, -1.0, 0.0)],
        stock_target: None,
        max_adjustment: 0.35,
        motions,
        support_blends,
        hidden_parts,
        visible_hands: [true; 6],
    }
}
