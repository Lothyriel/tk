//! Adapter from the existing authoritative action clock to configured parts.
use super::*;
use common::{
    catalog::{ComponentKind, ResolvedWeapon, catalog},
    weapon_animation::*,
};

/// Preview uses the very same cache, bindings, moving parts and contact solver
/// as publication. Framing consumes bounds in request space, never legacy meshes.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct ViewAssets<'w> {
    assets: AssemblyAssets<'w>,
    bindings: Res<'w, VisualBindings>,
}

impl ViewAssets<'_> {
    pub(crate) fn preview(
        &mut self,
        spec: &ResolvedWeapon,
        input: &ConfiguredHandling,
    ) -> Result<Vec<Vec3>, String> {
        let pieces = self.bindings.pieces(spec)?;
        for (_, _, asset, _) in &pieces {
            self.assets
                .cache
                .load(asset, &mut self.assets.meshes, &mut self.assets.materials)?;
        }
        let contacts = WeaponContacts::from_assembly(&spec.assembly)
            .map_err(|e| format!("weapon contacts: {e:?}"))?;
        let (solution, _) = resolve_pose(
            &self.assets.cache,
            contacts,
            input,
            pieces
                .iter()
                .map(|(node, _, asset, pose)| (*node, asset.as_str(), *pose)),
        )?;
        let mut points = Vec::new();
        for (node, _, asset, placement) in pieces {
            let asset = &self.assets.cache.assets[&asset];
            let poses = part_poses(asset, node, &input.motions)?;
            for part in &asset.parts {
                if input.hidden_parts.contains(&(node, part.id.clone())) {
                    continue;
                }
                let pose = solution
                    .root
                    .mul_transform(placement)
                    .mul_transform(poses[&part.id].1);
                points.extend(corners(&part.bounds).map(|p| pose.transform_point(p)));
            }
        }
        let handles = self
            .assets
            .hands
            .load(&mut self.assets.meshes, &mut self.assets.materials);
        let palm = self
            .assets
            .meshes
            .get(&handles.2)
            .expect("shared palm mesh");
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(vertices)) =
            palm.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            unreachable!("palm positions")
        };
        for (arm, visible) in [
            (solution.support, input.visible_hands[2]),
            (solution.trigger, input.visible_hands[5]),
        ] {
            if visible {
                points.extend(
                    vertices
                        .iter()
                        .map(|p| arm.wrist.transform_point(Vec3::from(*p))),
                );
            }
        }
        Ok(points)
    }
}

fn corners(bounds: &Bounds) -> [Vec3; 8] {
    std::array::from_fn(|i| {
        Vec3::new(
            if i & 1 == 0 {
                bounds.min[0]
            } else {
                bounds.max[0]
            },
            if i & 2 == 0 {
                bounds.min[1]
            } else {
                bounds.max[1]
            },
            if i & 4 == 0 {
                bounds.min[2]
            } else {
                bounds.max[2]
            },
        )
    })
}

/// The rest pose comes from the resolved node, while the action delta is relative
/// to the profile's original magazine. It therefore follows a replaced/rotated
/// magazine socket instead of resetting the attachment to a model-specific origin.
pub(crate) fn action(
    spec: &ResolvedWeapon,
    action: WeaponAction,
    recoil: f32,
    mechanics: &common::catalog::MechanicalSnapshot,
) -> ConfiguredHandling {
    let reloading = mechanics.reloading();
    let mut bolt = bolt_transform(spec.class, &action, recoil, reloading);
    if spec.class == common::WeaponKind::Pistol && mechanics.bolt_open && !reloading {
        bolt.translation.z = WeaponRig::for_weapon(spec.class).bolt_travel;
    }
    let mut motions = vec![
        PartMotion {
            node: 0,
            part: "bolt".into(),
            delta: bolt,
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
        if !action.magazine_visible || (!mechanics.magazine_inserted && !reloading) {
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
