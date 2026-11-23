use super::*;
use crate::{
    catalog::{catalog, AnchorKind, ManipulationKind},
    WeaponKind,
};

#[test]
fn vault_pose_tucks_feet_and_keeps_configured_grips_through_start_clear_and_release() {
    for weapon in [WeaponKind::Rifle, WeaponKind::Smg, WeaponKind::Pistol] {
        let spec = weapon.spec();
        let mut previous: Option<[SegmentPose; SEGMENT_COUNT]> = None;
        for step in 0..=100 {
            let mut state = PoseState {
                weapon,
                ..default()
            };
            state.set_vault(Some(step as f32 / 100.0));
            let rig = evaluate(
                &state,
                [1.0; 2],
                spec,
                &MechanicalSnapshot::default(),
                None,
                Some(0.058),
            )
            .unwrap_or_else(|e| panic!("{weapon:?} step={step}: {e:?}"));
            let configured = rig.weapon.unwrap();
            for (hand, grip) in [
                (Segment::LeftHand, configured.support.wrist),
                (Segment::RightHand, configured.trigger.wrist),
            ] {
                assert!(
                    Vec3::from(rig.segments[hand.index()].position).distance(grip.translation)
                        < 2e-6
                );
            }
            for (i, part) in rig.segments.iter().enumerate() {
                let position = Vec3::from(part.position);
                assert!(position.is_finite() && Quat::from_array(part.rotation).is_finite());
                if let Some(before) = &previous {
                    assert!(position.distance(before[i].position.into()) < 0.025);
                }
            }
            if step == 50 {
                assert!(rig.segments[Segment::LeftFoot.index()].position[1] > -0.62);
                assert!(rig.segments[Segment::RightFoot.index()].position[1] > -0.62);
            }
            previous = Some(rig.segments);
        }
    }
}

#[test]
fn voluntary_prone_configured_rig_keeps_low_body_and_rigid_grips() {
    for weapon in [WeaponKind::Rifle, WeaponKind::Smg, WeaponKind::Pistol] {
        let spec = weapon.spec();
        for (ads, kind) in [
            (0.0, None),
            (1.0, None),
            (0.0, Some(ManipulationKind::TacticalReload)),
        ] {
            let state = PoseState {
                weapon,
                ads,
                prone: true,
                grounded: true,
                ..default()
            };
            let mechanics = MechanicalSnapshot {
                kind,
                fraction: 0.42,
                ..default()
            };
            let rig = evaluate(&state, [1.0; 2], spec, &mechanics, None, Some(0.058))
                .unwrap_or_else(|e| panic!("{weapon:?} {ads} {kind:?}: {e:?}"));
            let centre = |segment: Segment| Vec3::from(rig.segments[segment.index()].position);
            assert!(centre(Segment::Head).z < centre(Segment::Thorax).z);
            assert!(centre(Segment::Thorax).z < centre(Segment::Pelvis).z);
            assert!(centre(Segment::Thorax).y.abs() < 0.1);
            let weapon = rig.weapon.unwrap();
            assert!(centre(Segment::LeftHand).distance(weapon.support.wrist.translation) < 2e-6);
            assert!(centre(Segment::RightHand).distance(weapon.trigger.wrist.translation) < 2e-6);
        }
    }
}

#[test]
fn configured_handling_tracks_preserve_anatomy_and_grips_at_authoritative_fractions() {
    for weapon in [WeaponKind::Rifle, WeaponKind::Smg, WeaponKind::Pistol] {
        for kind in [
            ManipulationKind::Inspect,
            ManipulationKind::Unequip,
            ManipulationKind::Equip,
        ] {
            let mut previous: Option<Transform> = None;
            for step in 0..=280 {
                let mechanics = MechanicalSnapshot {
                    kind: Some(kind),
                    fraction: step as f32 / 280.0,
                    ..default()
                };
                let state = PoseState {
                    weapon,
                    ..default()
                };
                let rig = evaluate(
                    &state,
                    [1.0; 2],
                    weapon.spec(),
                    &mechanics,
                    None,
                    Some(0.058),
                )
                .unwrap_or_else(|e| panic!("{weapon:?} {kind:?} step={step}: {e:?}"));
                let root = rig.weapon.unwrap().root;
                for (hand, anchor) in [
                    (Segment::LeftHand, AnchorKind::SupportGrip),
                    (Segment::RightHand, AnchorKind::TriggerGrip),
                ] {
                    assert!(
                        rig.segments[hand.index()].transform().translation.distance(
                            root.transform_point(
                                weapon.spec().assembly.anchors[&anchor].translation
                            )
                        ) < 2e-6
                    );
                }
                if let Some(previous) = previous {
                    assert!(
                        root.translation.distance(previous.translation) < 0.02,
                        "{weapon:?} {kind:?} step={step}: discontinuous solver"
                    );
                    assert!(root.rotation.angle_between(previous.rotation) < 0.03);
                }
                previous = Some(root);
            }
        }
    }
}

#[test]
fn configured_stock_stance_keeps_both_hands_and_shoulder_contact_through_ads() {
    for weapon in [WeaponKind::Rifle, WeaponKind::Smg] {
        let spec = weapon.spec();
        let stock = spec.assembly.anchors[&AnchorKind::StockContact];
        for cover in [0.0, 0.025, 0.0525, 0.058] {
            for (crouch, pitch, lean) in [(0.0, 0.0, 0.0), (1.0, -0.8, -0.3), (0.5, 0.8, 0.3)] {
                let mut previous: Option<Transform> = None;
                for step in 0..=120 {
                    let state = PoseState {
                        weapon,
                        crouch,
                        aim_pitch: pitch,
                        aim_roll: lean,
                        ads: step as f32 / 120.0,
                        ..default()
                    };
                    let rig = evaluate(
                        &state,
                        [1.0; 2],
                        spec,
                        &MechanicalSnapshot::default(),
                        None,
                        Some(cover),
                    )
                    .unwrap_or_else(|e| {
                        panic!(
                            "{weapon:?} cover={cover} crouch={crouch} pitch={pitch} ads={} {e:?}",
                            state.ads
                        )
                    });
                    let solution = rig.weapon.unwrap();
                    let thorax = rig.segments[Segment::Thorax.index()].transform();
                    if step == 120 {
                        let pocket = thorax.transform_point(Vec3::new(0.14, 0.1, -0.145 - cover));
                        assert!(
                            solution
                                .root
                                .transform_point(stock.translation)
                                .distance(pocket)
                                < 2e-6
                        );
                    }
                    for (hand, anchor) in [
                        (Segment::LeftHand, AnchorKind::SupportGrip),
                        (Segment::RightHand, AnchorKind::TriggerGrip),
                    ] {
                        let expected = solution.root.mul_transform(spec.assembly.anchors[&anchor]);
                        let actual = rig.segments[hand.index()].transform();
                        assert!(actual.translation.distance(expected.translation) < 2e-6);
                    }
                    if let Some(previous) = previous {
                        assert!(solution.root.translation.distance(previous.translation) < 0.025);
                    }
                    previous = Some(solution.root);
                }
            }
        }
    }
}

#[test]
fn configured_stock_revision_adjusts_the_weapon_or_rejects_without_stretching() {
    let state = PoseState {
        ads: 1.0,
        ..default()
    };
    let mut spec = WeaponKind::Rifle.spec().clone();
    for extension in [-0.015, 0.015, 4.0] {
        spec.assembly
            .anchors
            .get_mut(&AnchorKind::StockContact)
            .unwrap()
            .translation
            .z = 0.335 + extension;
        let solved = evaluate(
            &state,
            [1.0; 2],
            &spec,
            &MechanicalSnapshot::default(),
            None,
            Some(0.058),
        );
        if extension > 1.0 {
            assert!(
                solved.is_err(),
                "an impossible stock must not discard its shoulder contact"
            );
        } else {
            let rig = solved.unwrap();
            let root = rig.weapon.unwrap().root;
            let pocket = rig.segments[Segment::Thorax.index()]
                .transform()
                .transform_point(Vec3::new(0.14, 0.1, -0.203));
            assert!(
                root.transform_point(spec.assembly.anchors[&AnchorKind::StockContact].translation)
                    .distance(pocket)
                    < 2e-6
            );
            for (side, upper, forearm, hand) in [
                (
                    -1.0,
                    Segment::LeftUpperArm,
                    Segment::LeftForearm,
                    Segment::LeftHand,
                ),
                (
                    1.0,
                    Segment::RightUpperArm,
                    Segment::RightForearm,
                    Segment::RightHand,
                ),
            ] {
                let torso = rig.segments[Segment::Thorax.index()].transform();
                let shoulder = torso.transform_point(Vec3::new(side * 0.3, 0.1, 0.0));
                let upper = rig.segments[upper.index()].transform();
                let forearm = rig.segments[forearm.index()].transform();
                let hand = Vec3::from(rig.segments[hand.index()].position);
                assert!(upper.transform_point(Vec3::Y * 0.15).distance(shoulder) < 2e-6);
                assert!(
                    upper
                        .transform_point(Vec3::NEG_Y * 0.15)
                        .distance(forearm.transform_point(Vec3::Y * 0.14))
                        < 2e-6
                );
                assert!(forearm.transform_point(Vec3::NEG_Y * 0.14).distance(hand) < 2e-6);
            }
        }
    }
}

#[test]
fn configured_anatomy_preserves_rigid_contacts_and_all_body_ids_across_stances_and_reload() {
    for weapon in [WeaponKind::Rifle, WeaponKind::Pistol, WeaponKind::Smg] {
        let spec = weapon.spec();
        for empty in [false, true] {
            for fraction in [0.0, 0.05, 0.22, 0.55, 0.82, 0.94] {
                let mechanics = MechanicalSnapshot {
                    kind: Some(if empty {
                        ManipulationKind::EmptyReload
                    } else {
                        ManipulationKind::TacticalReload
                    }),
                    fraction,
                    magazine_inserted: true,
                    equipped: true,
                    ..default()
                };
                let action = reload_action(weapon, fraction, empty);
                for (crouch, pitch, lean) in [(0.0, -0.8, -0.3), (1.0, 0.8, 0.3), (0.5, 0.0, 0.0)] {
                    let state = PoseState {
                        weapon,
                        crouch,
                        aim_pitch: pitch,
                        aim_roll: lean,
                        ads: if fraction == 0.0 { 1.0 } else { 0.0 },
                        ..default()
                    };
                    let solved = evaluate(&state, [1.0; 2], spec, &mechanics, None, None).unwrap();
                    let base = evaluate_pose_with_action(&state, 1.0, 1.0, Some(action));
                    let root = solved.weapon.unwrap().root;
                    let chest = Vec3::from(solved.segments[Segment::Thorax.index()].position);
                    for (side, upper, lower, palm, grip) in [
                        (
                            -1.0,
                            Segment::LeftUpperArm,
                            Segment::LeftForearm,
                            Segment::LeftHand,
                            None,
                        ),
                        (
                            1.0,
                            Segment::RightUpperArm,
                            Segment::RightForearm,
                            Segment::RightHand,
                            Some(spec.assembly.anchors[&AnchorKind::TriggerGrip]),
                        ),
                    ] {
                        let shoulder = chest + Vec3::new(side * 0.30, 0.10, 0.0);
                        let upper = solved.segments[upper.index()].transform();
                        let lower = solved.segments[lower.index()].transform();
                        let palm = solved.segments[palm.index()].transform();
                        assert!(upper.transform_point(Vec3::Y * 0.15).distance(shoulder) < 2e-6);
                        assert!(
                            upper
                                .transform_point(Vec3::NEG_Y * 0.15)
                                .distance(lower.transform_point(Vec3::Y * 0.14))
                                < 2e-6
                        );
                        assert!(
                            lower
                                .transform_point(Vec3::NEG_Y * 0.14)
                                .distance(palm.translation)
                                < 2e-6
                        );
                        if let Some(grip) = grip {
                            assert!(
                                palm.translation
                                    .distance(root.transform_point(grip.translation))
                                    < 2e-6
                            );
                        }
                    }
                    for segment in SEGMENTS {
                        if !matches!(
                            segment,
                            Segment::LeftUpperArm
                                | Segment::LeftForearm
                                | Segment::LeftHand
                                | Segment::RightUpperArm
                                | Segment::RightForearm
                                | Segment::RightHand
                        ) {
                            assert_eq!(
                                solved.segments[segment.index()].position,
                                base[segment.index()].position
                            );
                            assert_eq!(
                                solved.segments[segment.index()].rotation,
                                base[segment.index()].rotation
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn configured_anatomy_grip_revision_moves_damage_volume_and_rejects_infeasible_contacts() {
    let mut spec = catalog()
        .weapon(crate::catalog::WeaponId::M4A1)
        .unwrap()
        .clone();
    let state = PoseState::default();
    let mechanics = MechanicalSnapshot::default();
    let old = evaluate(&state, [1.0; 2], &spec, &mechanics, None, None).unwrap();
    let authored =
        Transform::from_xyz(-0.46, 0.02, -0.20).with_rotation(Quat::from_rotation_z(0.4));
    spec.assembly
        .anchors
        .insert(AnchorKind::SupportGrip, authored);
    let new = evaluate(&state, [1.0; 2], &spec, &mechanics, None, None).unwrap();
    let hand = new.segments[Segment::LeftHand.index()].transform();
    let expected = new.weapon.unwrap().root.mul_transform(authored);
    assert!(hand.translation.distance(expected.translation) < 2e-6);
    assert!(
        hand.rotation
            .normalize()
            .angle_between(expected.rotation.normalize())
            < 0.001
    );
    let ray = hand.translation + Vec3::X * 0.1;
    assert!(ray_box(
        ray,
        Vec3::NEG_X,
        new.segments[Segment::LeftHand.index()],
        Segment::LeftHand.half_size(),
        0.2
    )
    .is_some());
    assert!(ray_box(
        ray,
        Vec3::NEG_X,
        old.segments[Segment::LeftHand.index()],
        Segment::LeftHand.half_size(),
        0.2
    )
    .is_none());
    spec.assembly
        .anchors
        .insert(AnchorKind::SupportGrip, Transform::from_xyz(4.0, 0.0, 0.0));
    assert!(evaluate(&state, [1.0; 2], &spec, &mechanics, None, None).is_err());
    // Once physics owns the 17 segments, even an infeasible held configuration
    // cannot overwrite a physical pose or synthesize a fresh aiming arm.
    let mut state = state;
    state.physical = Some(Box::new(PackedRig::pack(&new.segments)));
    let physical = evaluate(&state, [1.0; 2], &spec, &mechanics, None, None).unwrap();
    assert!(physical.weapon.is_none());
    let unpacked = state.physical.as_ref().unwrap().unpack();
    for (actual, expected) in physical.segments.into_iter().zip(unpacked) {
        assert_eq!(actual.position, expected.position);
        assert_eq!(actual.rotation, expected.rotation);
    }
}
