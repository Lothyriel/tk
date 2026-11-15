//! Native first-person producer. Camera framing translates the complete rigid
//! view rig; it never scales bones or separates the palms from their sockets.
use super::*;
use crate::render::voxel::configured::{ConfiguredPose, ConfiguredWeapon, native};

pub(super) fn prepare(
    mut commands: Commands,
    state: LocalFramingState,
    mut assets: native::ViewAssets,
    camera: Single<(&Transform, &Projection), (With<ViewModelCamera>, Without<WeaponViewModel>)>,
    mut views: Query<
        (&WeaponViewModel, &mut Transform),
        (
            With<LocalView>,
            Without<ViewModelCamera>,
            Without<WeaponMeshRoot>,
        ),
    >,
    roots: Query<
        (Entity, &ChildOf, &Transform, Option<&ConfiguredWeapon>),
        (
            With<WeaponMeshRoot>,
            Without<WeaponViewModel>,
            Without<ViewModelCamera>,
        ),
    >,
) {
    let LocalFramingState {
        time,
        id,
        session,
        mut feedback,
    } = state;
    let Some(player) = session.local(id.0) else {
        return;
    };
    let Some(response) = feedback.responses.get_mut(&id.0) else {
        return;
    };
    let displayed = response.displayed.unwrap_or(player.weapon);
    let action = response
        .action
        .unwrap_or_else(|| reload_action(displayed, 0.0, false));
    let (eye, projection) = *camera;
    let Projection::Perspective(projection) = projection else {
        return;
    };
    for (entity, parent, root, previous) in &roots {
        let Ok((weapon, mut view)) = views.get_mut(parent.0) else {
            continue;
        };
        if weapon.weapon != displayed {
            continue;
        }
        let mut request = if displayed == player.weapon {
            ConfiguredWeapon {
                epoch: session.configurations.cache.epoch(),
                handle: player.configuration,
                view_model: true,
                magazine: None,
            }
        } else if let Some(previous) = previous {
            *previous
        } else {
            continue;
        };
        let Some(entry) = session
            .configurations
            .cache
            .get(request.epoch, request.handle)
        else {
            continue;
        };
        request.magazine = entry.presented_magazine(&response.displayed_mechanics);
        let Some(visual) = entry.visual(request.magazine) else {
            continue;
        };
        let mut input = native::action(
            visual,
            action,
            response.recoil,
            &response.displayed_mechanics,
        );
        if displayed == WeaponKind::Pistol {
            // The pistol has no stock to locate the torso. Put the shoulders
            // behind and below the grips so the arms extend forward instead
            // of folding vertically beside the receiver.
            for shoulder in &mut input.shoulders {
                *shoulder += Vec3::new(0.0, -0.18, 0.40);
            }
        }
        if let Err(error) = native::brace_view(
            &mut input,
            visual,
            response.ads,
            common::humanoid::configured::shoulder_cover(&player.armor),
        ) {
            input.preflight_error = Some(error);
        }
        // Explicit first-person rig: shoulders/poles are in presentation space,
        // moving with framing and action layers, with the same 30/28 cm bones.
        // The world rig instead derives shoulders from its anatomical torso.
        for (side, segments) in [
            [
                common::humanoid::Segment::LeftUpperArm,
                common::humanoid::Segment::LeftForearm,
                common::humanoid::Segment::LeftHand,
            ],
            [
                common::humanoid::Segment::RightUpperArm,
                common::humanoid::Segment::RightForearm,
                common::humanoid::Segment::RightHand,
            ],
        ]
        .into_iter()
        .enumerate()
        {
            for (part, segment) in segments.into_iter().enumerate() {
                input.visible_hands[side * 3 + part] =
                    player.body.anatomy.missing & (1 << segment.index()) == 0;
            }
        }
        // Preview and publication share the solver and cached assets. A failed
        // preview still publishes the request so the assembler reports the
        // rejection and retains its previously accepted hierarchy/pose.
        let inspecting =
            response.displayed_mechanics.kind == Some(common::catalog::ManipulationKind::Inspect);
        let lowering =
            common::weapon_animation::manipulation::lowering(&response.displayed_mechanics)
                || !player.alive
                || !matches!(
                    player.pose.mode,
                    PhysicalMode::Standing | PhysicalMode::Stumbling
                );
        if (matches!(displayed, WeaponKind::Pistol | WeaponKind::Rifle)
            || (!response.displayed_mechanics.reloading() && !lowering)
            || (response.ads > 0.0
                && visual
                    .assembly
                    .anchors
                    .contains_key(&common::catalog::AnchorKind::Sight)))
            && let Ok(preview) = assets.preview(visual, &input)
        {
            if displayed != WeaponKind::Pistol
                && !inspecting
                && response.ads > 0.0
                && let Some(sight) = visual
                    .assembly
                    .anchors
                    .get(&common::catalog::AnchorKind::Sight)
            {
                // A shorter stock moves the solved weapon relative to its fixed
                // shoulder pocket. Frame the entire rigid rig from that accepted
                // pose, otherwise the receiver/rear iron crosses the near plane.
                // This preserves all palm/stock contacts and authored eye relief.
                let offset = input.preferred_root.transform_point(sight.translation)
                    - preview.root.transform_point(sight.translation);
                let correction = view.rotation
                    * root.rotation
                    * offset
                    * common::weapon_animation::ease(response.ads);
                view.translation += correction;
                response.frame_offset *= (-time.delta_secs() * 14.0).exp();
                view.translation += eye.rotation * response.frame_offset;
            } else {
                let to_eye = eye.to_matrix().inverse() * view.mul_transform(*root).to_matrix();
                let correction = if lowering
                    || (displayed != WeaponKind::Pistol && response.displayed_mechanics.reloading())
                {
                    response.frame_offset * (-time.delta_secs() * 14.0).exp()
                } else {
                    let points: Vec<_> = preview
                        .points
                        .into_iter()
                        .map(|p| to_eye.transform_point3(p))
                        .collect();
                    pistol_frame_correction(&points, projection, response.ads)
                };
                response.frame_offset = correction;
                view.translation += eye.rotation * correction;
            }
            if displayed == WeaponKind::Rifle {
                let to_eye = eye.to_matrix().inverse() * view.mul_transform(*root).to_matrix();
                let retreat = preview.near_points.iter().fold(0.0_f32, |depth, point| {
                    depth.max(to_eye.transform_point3(*point).z + projection.near + 0.01)
                });
                // Move the complete rig down the sight line, preserving
                // rigid contacts, while recoil/mechanisms approach the eye.
                view.translation -= eye.rotation * Vec3::Z * retreat;
            }
        } else if response.frame_offset.length_squared() > 1e-10 {
            // Retain the current camera correction through interrupted inspect,
            // then return smoothly instead of snapping on the cancellation frame.
            response.frame_offset *= (-time.delta_secs() * 14.0).exp();
            view.translation += eye.rotation * response.frame_offset;
        }
        commands.entity(entity).insert((request, input));
    }
}

pub(super) fn flash(
    id: Res<PlayerId>,
    session: Res<ClientSession>,
    feedback: Res<Feedback>,
    roots: Query<(&ConfiguredPose, &ChildOf, &Visibility), With<WeaponMeshRoot>>,
    views: Query<&WeaponViewModel>,
    mut flashes: Query<
        (&ChildOf, &mut Transform, &mut Visibility),
        (With<Flash>, Without<WeaponMeshRoot>),
    >,
) {
    for (parent, mut transform, mut visible) in &mut flashes {
        let Ok((pose, view, visibility)) = roots.get(parent.0) else {
            *visible = Visibility::Hidden;
            continue;
        };
        let Ok(weapon) = views.get(view.0) else {
            continue;
        };
        *visible = if *visibility != Visibility::Hidden
            && session
                .local(id.0)
                .is_some_and(|p| p.alive && p.weapon == weapon.weapon)
            && feedback.responses.get(&id.0).is_some_and(|r| r.flash > 0.0)
        {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        *transform = pose
            .muzzle
            .mul_transform(Transform::from_translation(Vec3::NEG_Z * 0.05));
    }
}

#[cfg(test)]
mod tests;
