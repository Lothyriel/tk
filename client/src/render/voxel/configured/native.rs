//! Adapter from the existing authoritative action clock to configured parts.
use super::*;
use common::{catalog::ResolvedWeapon, weapon_animation::*};

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
    ) -> Result<(Vec<Vec3>, Transform), String> {
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
        for (arm, visible, weights) in [
            (
                solution.support,
                input.visible_hands[2],
                input.support_fingers,
            ),
            (solution.trigger, input.visible_hands[5], [0.0; 2]),
        ] {
            if visible {
                points.extend(vertices.iter().enumerate().map(|(i, p)| {
                    arm.wrist
                        .transform_point(super::super::super::weapons::grasp::position(
                            palm,
                            i,
                            Vec3::from(*p),
                            &weights,
                        ))
                }));
            }
        }
        Ok((points, solution.root))
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

/// Explicit camera-space shoulder rig. Its reference pocket comes from baseline
/// class framing, not the current configuration: a changed stock moves/rejects
/// the weapon instead of moving its target. Clothing moves the virtual torso
/// behind the pocket, preserving the sight line in the first-person camera.
pub(crate) fn brace_view(
    input: &mut ConfiguredHandling,
    spec: &ResolvedWeapon,
    ads: f32,
    cover: f32,
) -> Result<(), String> {
    let contacts = WeaponContacts::from_assembly(&spec.assembly)
        .map_err(|e| format!("view contacts: {e:?}"))?;
    let Some(stock) = contacts.stock else {
        return Ok(());
    };
    let reference = spec
        .class
        .spec()
        .assembly
        .anchors
        .get(&common::catalog::AnchorKind::StockContact)
        .ok_or_else(|| "view stock framing reference is missing".to_owned())?;
    let (turn, brace) = common::humanoid::configured::brace_weights(ads);
    let rotation = Quat::from_rotation_y(common::humanoid::configured::BLADED_YAW);
    let pocket = common::humanoid::configured::stock_pocket(cover);
    let torso = Transform::from_translation(reference.translation - rotation * pocket)
        .with_rotation(rotation);
    for (i, side) in [-1.0, 1.0].into_iter().enumerate() {
        input.shoulders[i] = input.shoulders[i].lerp(
            torso.transform_point(Vec3::new(side * 0.30, 0.1, 0.0)),
            turn,
        );
        input.poles[i] = input.poles[i].lerp(rotation * input.poles[i], turn);
    }
    if brace > 0.0 {
        let free = handling::fit_weapon(
            input.preferred_root,
            contacts,
            input.shoulders,
            input.poles,
            None,
            input.max_adjustment,
        )
        .map_err(|e| format!("view reach: {e:?}"))?;
        input.stock_target = Some(
            free.root
                .transform_point(stock.translation)
                .lerp(torso.transform_point(pocket), brace),
        );
    }
    Ok(())
}

/// Rest placement comes from the resolved node; mechanism deltas stay local to
/// the attachment, including relocated/rotated magazine sockets.
pub(crate) fn action(
    spec: &ResolvedWeapon,
    action: WeaponAction,
    recoil: f32,
    mechanics: &common::catalog::MechanicalSnapshot,
) -> ConfiguredHandling {
    let reloading = mechanics.reloading();
    let parts = handling::mechanisms(spec, action, recoil, mechanics);
    let mut motions = vec![
        PartMotion {
            node: 0,
            part: "bolt".into(),
            delta: parts.bolt,
        },
        PartMotion {
            node: 0,
            part: "control".into(),
            delta: parts.control,
        },
    ];
    let mut support_blends = Vec::new();
    let mut hidden_parts = Vec::new();
    if let Some((node, _, delta)) = parts.magazine {
        motions.push(PartMotion {
            node,
            part: "magazine".into(),
            delta,
        });
        support_blends.push(SocketBlend {
            address: SocketAddress {
                node,
                socket: "magazine.grasp".into(),
            },
            weight: action.magazine_grip,
        });
        let resumed_without_source = spec.definition == common::WeaponId::M4A1
            && reloading
            && mechanics.reload_target.present()
            && common::weapon_animation::m4a1::magazine(mechanics).is_none();
        if !action.magazine_visible
            || (!mechanics.magazine_inserted && !reloading)
            || resumed_without_source
        {
            hidden_parts.push((node, "magazine".into()));
        }
    }
    support_blends.push(SocketBlend {
        address: SocketAddress {
            node: 0,
            socket: "control.grasp".into(),
        },
        weight: action.control_grip,
    });
    // The authored breech is ahead of the camera-space shoulders. Keeping the
    // legacy shoulders at the breech raised the upper arms beside the sights.
    // Derive the hip torso from the stock, then brace_view blends into ADS.
    let shoulders = spec
        .assembly
        .anchors
        .get(&common::catalog::AnchorKind::StockContact)
        .filter(|_| spec.assembly.pivots.contains_key("buffer_tube"))
        .map_or(
            [Vec3::new(-0.30, 0.1, 0.0), Vec3::new(0.30, 0.1, 0.0)],
            |stock| {
                [-1.0, 1.0].map(|side| stock.translation + Vec3::new(side * 0.30, -0.12, -0.17))
            },
        );
    ConfiguredHandling {
        preflight_error: None,
        preferred_root: Transform::IDENTITY,
        shoulders,
        poles: [Vec3::new(-0.8, -1.0, 0.0), Vec3::new(0.8, -1.0, 0.0)],
        stock_target: None,
        max_adjustment: 0.35,
        motions,
        support_blends,
        support_clearance: if spec.definition == common::WeaponId::M4A1 {
            m4a1::support_clearance(&action)
        } else {
            Vec3::ZERO
        },
        support_fingers: if spec.definition == common::WeaponId::M4A1 {
            super::super::super::weapons::grasp::weights(&action)
        } else {
            [0.0; 2]
        },
        hidden_parts,
        visible_hands: [true; 6],
    }
}
