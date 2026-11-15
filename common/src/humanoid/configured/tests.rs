use super::*;
use crate::{
    catalog::{catalog, AnchorKind, ManipulationKind},
    WeaponKind,
};

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
