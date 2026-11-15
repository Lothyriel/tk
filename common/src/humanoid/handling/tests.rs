use super::*;

fn contacts() -> WeaponContacts {
    WeaponContacts {
        support: Transform::from_xyz(-0.025, 0.005, -0.35)
            .with_rotation(Quat::from_rotation_z(0.35)),
        trigger: Transform::from_xyz(0.025, -0.09, 0.035),
        stock: Some(Transform::from_xyz(0.0, 0.0, 0.18)),
    }
}

fn assert_arm(arm: ArmSolution, shoulder: Vec3, root: Transform, contact: Transform) {
    // Anatomical oracle: literal measured segment lengths, not solver constants.
    assert!((arm.elbow.distance(shoulder) - 0.30).abs() < 2e-6);
    assert!((arm.wrist.translation.distance(arm.elbow) - 0.28).abs() < 2e-6);
    let expected = root.mul_transform(contact);
    assert!(arm.wrist.translation.distance(expected.translation) < 1e-6);
    assert!(arm.wrist.rotation.angle_between(expected.rotation) < 0.001);
    let upper = (shoulder - arm.elbow).normalize();
    let lower = (arm.wrist.translation - arm.elbow).normalize();
    let elbow_angle = upper.angle_between(lower);
    assert!(
        (0.33..2.90).contains(&elbow_angle),
        "elbow bend {elbow_angle}"
    );
}

#[test]
fn handling_arm_preserves_lengths_orientation_and_pole_without_clamping_contact() {
    let shoulder = Vec3::new(-0.3, 0.1, 0.0);
    for distance in [0.10, 0.20, 0.40, 0.575] {
        for direction in [Vec3::NEG_Z, Vec3::Y, Vec3::new(0.2, -0.6, -0.4).normalize()] {
            for pole in [Vec3::NEG_Y, Vec3::X, direction] {
                let wrist = Transform::from_translation(shoulder + direction * distance)
                    .with_rotation(Quat::from_rotation_y(0.8));
                let solved = solve_arm(Hand::Support, shoulder, wrist, pole).unwrap();
                assert_arm(solved, shoulder, Transform::IDENTITY, wrist);
                let projected = pole - direction * pole.dot(direction);
                if projected.length_squared() > 1e-6 {
                    assert!((solved.elbow - shoulder).dot(projected) > 0.0);
                }
            }
        }
    }
    for distance in [0.0, 0.09, 0.58, 1.0] {
        assert_eq!(
            solve_arm(
                Hand::Trigger,
                shoulder,
                Transform::from_translation(shoulder + Vec3::NEG_Z * distance),
                Vec3::NEG_Y
            )
            .unwrap_err(),
            HandlingError::Unreachable(Hand::Trigger)
        );
    }
}

#[test]
fn handling_root_and_contacts_follow_crouch_lean_pitch_and_moving_magazine() {
    for crouch in [0.0, 1.0] {
        for lean in [-0.18, 0.0, 0.18] {
            for pitch in [-1.0_f32, 0.0, 1.0] {
                let chest = Vec3::new(lean, 0.2 - crouch * 0.14, -crouch * 0.12);
                let shoulders = [
                    chest + Vec3::new(-0.30, 0.10, 0.0),
                    chest + Vec3::new(0.30, 0.10, 0.0),
                ];
                let poles = [Vec3::new(-0.8, -1.0, 0.0), Vec3::new(0.8, -1.0, 0.0)];
                let preferred = Transform::from_translation(chest + Vec3::new(0.0, 0.10, -0.12))
                    .with_rotation(Quat::from_rotation_x(pitch));
                for drop in [0.0, 0.1, 0.2] {
                    let magazine = Transform::from_xyz(-0.02, -drop, -0.10)
                        .with_rotation(Quat::from_rotation_x(-drop));
                    let socket = Transform::from_xyz(-0.04, -0.025, 0.025);
                    let grip = if drop == 0.0 {
                        contacts()
                    } else {
                        contacts()
                            .with_support(magazine.mul_transform(socket))
                            .unwrap()
                    };
                    let solved = fit_weapon(preferred, grip, shoulders, poles, None, 0.35).unwrap();
                    assert_eq!(solved.root.rotation, preferred.rotation);
                    assert_arm(solved.support, shoulders[0], solved.root, grip.support);
                    assert_arm(solved.trigger, shoulders[1], solved.root, grip.trigger);
                    assert_eq!(grip.trigger, contacts().trigger);
                }
            }
        }
    }
}

#[test]
fn handling_stock_and_extreme_grips_adjust_root_or_reject_explicitly() {
    let shoulders = [Vec3::new(-0.30, 0.1, 0.0), Vec3::new(0.30, 0.1, 0.0)];
    let poles = [Vec3::new(-0.8, -1.0, 0.0), Vec3::new(0.8, -1.0, 0.0)];
    let preferred = Transform::from_xyz(0.05, 0.10, -0.15);
    // This long foregrip cannot be held with the stock pinned here. Keep the
    // rejected case explicit; a shorter authored grip makes the fixture viable.
    assert_eq!(
        fit_weapon(
            preferred,
            contacts(),
            shoulders,
            poles,
            Some(Vec3::new(0.08, 0.10, 0.0)),
            0.30
        )
        .unwrap_err(),
        HandlingError::Unreachable(Hand::Support)
    );
    for clothing in [0.0, 0.025, 0.05] {
        let stock_target = Vec3::new(0.08, 0.10, -clothing);
        let mut grip = contacts();
        grip.support.translation.z = -0.20;
        let solved =
            fit_weapon(preferred, grip, shoulders, poles, Some(stock_target), 0.30).unwrap();
        assert!(
            solved
                .root
                .transform_point(grip.stock.unwrap().translation)
                .distance(stock_target)
                < 1e-6
        );
        assert_arm(solved.support, shoulders[0], solved.root, grip.support);
        assert_arm(solved.trigger, shoulders[1], solved.root, grip.trigger);
    }
    let mut impossible = contacts();
    impossible.support.translation.z = -3.0;
    assert!(fit_weapon(preferred, impossible, shoulders, poles, None, 4.0).is_err());
    assert!(fit_weapon(
        preferred,
        impossible,
        shoulders,
        poles,
        Some(Vec3::new(0.08, 0.10, 0.0)),
        4.0
    )
    .is_err());
    let distant = Transform::from_xyz(0.0, 0.0, -1.0);
    assert_eq!(
        fit_weapon(distant, contacts(), shoulders, poles, None, 0.05).unwrap_err(),
        HandlingError::RootAdjustmentExceeded
    );
    let solved = fit_weapon(distant, contacts(), shoulders, poles, None, 1.5).unwrap();
    assert_arm(
        solved.support,
        shoulders[0],
        solved.root,
        contacts().support,
    );
    assert_arm(
        solved.trigger,
        shoulders[1],
        solved.root,
        contacts().trigger,
    );
}

#[test]
fn handling_configuration_requires_named_rigid_contacts_and_keeps_grip_rotation() {
    let mut assembly = crate::catalog::catalog()
        .weapon(crate::WeaponId::M4A1)
        .unwrap()
        .assembly
        .clone();
    assembly.anchors.remove(&AnchorKind::SupportGrip);
    assert_eq!(
        WeaponContacts::from_assembly(&assembly).unwrap_err(),
        HandlingError::MissingAnchor(AnchorKind::SupportGrip)
    );
    let grip = contacts();
    assembly
        .anchors
        .insert(AnchorKind::SupportGrip, grip.support);
    assembly
        .anchors
        .insert(AnchorKind::TriggerGrip, grip.trigger);
    assembly
        .anchors
        .insert(AnchorKind::StockContact, grip.stock.unwrap());
    let resolved = WeaponContacts::from_assembly(&assembly).unwrap();
    assert_eq!(resolved.support, grip.support);
    assert_eq!(resolved.stock, grip.stock);
    for bad in [
        Transform::from_scale(Vec3::splat(2.0)),
        Transform::from_xyz(f32::NAN, 0.0, 0.0),
        Transform::from_rotation(Quat::from_xyzw(0.0, 0.0, 0.0, 2.0)),
    ] {
        assembly.anchors.insert(AnchorKind::SupportGrip, bad);
        assert_eq!(
            WeaponContacts::from_assembly(&assembly).unwrap_err(),
            HandlingError::NonRigidTransform
        );
    }
}

#[test]
fn handling_solution_is_the_same_rigid_assembly_in_local_and_world_frames() {
    let shoulders = [Vec3::new(-0.30, 0.1, 0.0), Vec3::new(0.30, 0.1, 0.0)];
    let poles = [Vec3::new(-0.8, -1.0, 0.0), Vec3::new(0.8, -1.0, 0.0)];
    let preferred = Transform::from_xyz(0.0, 0.08, -0.4).with_rotation(Quat::from_rotation_x(0.4));
    let local = fit_weapon(preferred, contacts(), shoulders, poles, None, 0.4).unwrap();
    let frame = Transform::from_xyz(2.0, 1.5, -3.0).with_rotation(Quat::from_euler(
        EulerRot::YXZ,
        1.2,
        -0.3,
        0.1,
    ));
    let world = fit_weapon(
        frame.mul_transform(preferred),
        contacts(),
        shoulders.map(|s| frame.transform_point(s)),
        poles.map(|p| frame.rotation * p),
        None,
        0.4,
    )
    .unwrap();
    assert!(
        world
            .root
            .translation
            .distance(frame.transform_point(local.root.translation))
            < 2e-6
    );
    for (a, b) in [
        (local.support, world.support),
        (local.trigger, world.trigger),
    ] {
        assert!(b.elbow.distance(frame.transform_point(a.elbow)) < 2e-6);
        assert!(
            b.wrist
                .translation
                .distance(frame.transform_point(a.wrist.translation))
                < 2e-6
        );
        assert!(
            b.wrist
                .rotation
                .angle_between(frame.rotation * a.wrist.rotation)
                < 0.001
        );
    }
}
