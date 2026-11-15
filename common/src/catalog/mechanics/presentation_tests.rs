use super::*;

#[test]
fn reload_snapshot_preserves_mixed_magazine_identity_across_commits_and_restore() {
    let mut kit =
        WeaponKit::from_configuration(catalog(), catalog().configuration(WeaponId::M4A1).unwrap())
            .unwrap();
    kit.weapon
        .components
        .iter_mut()
        .find(|p| p.node == 1)
        .unwrap()
        .definition = DefinitionId(120);
    kit.reserves = vec![DefinitionId(101)];
    for empty in [false, true] {
        let mut weapon = EquippedWeapon::from_kit(catalog(), WeaponInstanceId(99), &kit).unwrap();
        let count = if empty { 20 } else { 3 };
        for _ in 0..count {
            assert!(weapon.fire());
        }
        let old = weapon.mechanics().inserted().unwrap().clone();
        let incoming = weapon.mechanics().magazines()[1].clone();
        let mut publisher = crate::replication::ConfigurationPublisher::new(1, 1).unwrap();
        publisher
            .register(7, EquipmentSlot::Primary, weapon.instance())
            .unwrap();
        let packet = &publisher.entries()[0];
        let id = |value: MagazineSnapshot| packet.magazine(value).unwrap().id;
        assert!(weapon.mechanics_mut().start_reload(10.0));
        for target in [0.0, 2.49, 2.51, 6.99, 7.01, 8.99, 9.01, 10.01] {
            let mut at = EquippedWeapon::restore(catalog(), weapon.instance().clone()).unwrap();
            at.advance(target);
            let snapshot = at.mechanics().snapshot();
            let decoded: MechanicalSnapshot = crate::data::decode(&crate::data::encode(&snapshot));
            assert_eq!(decoded, snapshot);
            assert_eq!(at.mechanics().total_rounds(), 50 - count);
            assert_eq!(
                snapshot.inserted_magazine().map(id),
                at.mechanics().inserted().map(|m| m.id)
            );
            if target < 10.0 {
                assert_eq!(id(snapshot.magazine), old.id);
                assert_eq!(
                    packet.magazine(snapshot.magazine).unwrap().definition,
                    DefinitionId(120)
                );
                assert_eq!(snapshot.magazine.rounds(), old.rounds);
                assert_eq!(id(snapshot.reload_target), incoming.id);
                assert_eq!(
                    packet.magazine(snapshot.reload_target).unwrap().definition,
                    DefinitionId(101)
                );
                assert_eq!(
                    snapshot.reload_target.rounds(),
                    if empty && target >= 9.0 { 29 } else { 30 }
                );
            } else {
                assert_eq!(id(snapshot.magazine), incoming.id);
                assert!(!snapshot.reload_target.present());
            }
            // Cancellation does not leave a carried visual or invent an inserted
            // magazine; a new observer reconstructs exactly the committed state.
            let inserted = at.mechanics().inserted().map(|m| m.id);
            at.mechanics_mut().cancel();
            let cancelled = at.mechanics().snapshot();
            assert!(!cancelled.reload_target.present());
            assert_eq!(cancelled.inserted_magazine().map(id), inserted);
            assert_eq!(cancelled.magazine.present(), inserted.is_some());
            let restored = EquippedWeapon::restore(catalog(), at.instance().clone()).unwrap();
            assert_eq!(restored.mechanics().snapshot(), cancelled);
            if restored.mechanics().chamber().is_none()
                && restored
                    .mechanics()
                    .inserted()
                    .is_some_and(|m| m.rounds > 0)
            {
                // Feed-only resume has no outgoing magazine to animate.
                assert!(at.mechanics_mut().start_reload(10.0));
                let resumed = at.mechanics().snapshot();
                assert_eq!(resumed.phase, Some(ManipulationPhase::Feed));
                assert!(!resumed.magazine.present());
                assert_eq!(resumed.inserted_magazine().map(id), inserted);
            }
        }
    }
}

#[test]
fn reload_restore_rejects_forged_or_duplicated_source_magazine() {
    let mut state = MechanicalState::new(
        catalog(),
        WeaponId::M4A1.spec(),
        [1, 2, 3, 4].map(MagazineInstanceId),
    )
    .unwrap();
    assert!(state.fire(true, true));
    assert!(state.start_reload(2.4));
    for source in [
        None,
        Some(MagazineInstanceId(2)),
        Some(MagazineInstanceId(999)),
    ] {
        let mut forged = state.clone();
        forged.action.as_mut().unwrap().source_magazine = source;
        assert!(forged.validate(catalog(), WeaponId::M4A1.spec()).is_err());
    }
}
