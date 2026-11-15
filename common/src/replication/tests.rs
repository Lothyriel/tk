use super::*;
use crate::{
    catalog::{catalog, DefinitionId, EquippedWeapon, CATALOG_SOURCE},
    combat::Loadout,
    WeaponId,
};

fn fingerprint() -> u64 {
    catalog_fingerprint(CATALOG_SOURCE)
}

fn weapon(id: WeaponId) -> EquippedWeapon {
    EquippedWeapon::new(
        catalog(),
        WeaponInstanceId(700),
        catalog().configuration(id).unwrap(),
    )
    .unwrap()
}

fn publisher() -> ConfigurationPublisher {
    ConfigurationPublisher::new(1, fingerprint()).unwrap()
}

fn packet() -> Configuration {
    let mut publisher = publisher();
    publisher
        .register(7, EquipmentSlot::Primary, weapon(WeaponId::M4A1).instance())
        .unwrap();
    publisher.entries()[0].clone()
}

fn snapshot(packet: &Configuration, sequence: u64) -> crate::WorldSnapshot {
    crate::WorldSnapshot {
        configuration_epoch: packet.epoch,
        sequence,
        players: vec![crate::ClientData {
            id: packet.owner,
            configuration: packet.handle,
            weapon_instance: packet.instance,
            weapon_id: packet.weapon.definition,
            weapon: packet.weapon.definition.class(),
            weapon_slot: packet.slot,
            ..Default::default()
        }],
        destroyed_cells: Default::default(),
        raid: Default::default(),
        projectiles: vec![],
        impact_marks: vec![],
        fired_projectiles: vec![],
        physics_pieces: vec![],
        simulation_time: sequence as f32 / 30.0,
    }
}

fn epoch_message(epoch: u64) -> bytes::Bytes {
    data::encode(&ConfigurationMessage::Epoch {
        epoch,
        catalog: fingerprint(),
    })
}

#[test]
fn replication_receiver_waits_for_latest_snapshot_configuration_in_both_arrival_orders() {
    for config_first in [false, true] {
        let packet = packet();
        let mut receiver = ConfigurationReceiver::default();
        let bytes = data::encode(&ConfigurationMessage::Configuration(packet.clone()));
        if config_first {
            receiver
                .message(&epoch_message(1), catalog(), fingerprint())
                .unwrap();
            receiver.message(&bytes, catalog(), fingerprint()).unwrap();
        }
        receiver.snapshot(snapshot(&packet, 2)).unwrap();
        receiver.snapshot(snapshot(&packet, 1)).unwrap();
        if !config_first {
            assert!(receiver.take_ready().unwrap().is_none());
            receiver
                .message(&epoch_message(1), catalog(), fingerprint())
                .unwrap();
            assert!(receiver.take_ready().unwrap().is_none());
            receiver.message(&bytes, catalog(), fingerprint()).unwrap();
        }
        let accepted = receiver.take_ready().unwrap().unwrap();
        assert_eq!(accepted.sequence, 2);
        assert!(receiver.cache.matches(1, &accepted.players[0]));
        receiver.snapshot(snapshot(&packet, 2)).unwrap();
        assert!(
            receiver.take_ready().unwrap().is_none(),
            "duplicate cannot replay actions"
        );
        let mut forged = snapshot(&packet, 3);
        forged.players[0].weapon_instance.0 += 1;
        receiver.snapshot(forged).unwrap();
        assert_eq!(
            receiver.take_ready().unwrap_err(),
            ConfigurationError::Conflict
        );
        assert!(receiver.take_ready().unwrap().is_none());
        let mut next = packet.clone();
        next.epoch = 2;
        receiver.snapshot(snapshot(&next, 4)).unwrap();
        assert!(receiver.take_ready().unwrap().is_none());
        receiver
            .message(&epoch_message(2), catalog(), fingerprint())
            .unwrap();
        assert!(receiver.cache.get(1, packet.handle).is_none());
        assert!(
            receiver.take_ready().unwrap().is_none(),
            "epoch prevents reused handle aliasing"
        );
        let bytes = data::encode(&ConfigurationMessage::Configuration(next));
        receiver.message(&bytes, catalog(), fingerprint()).unwrap();
        assert_eq!(receiver.take_ready().unwrap().unwrap().sequence, 4);
        assert_eq!(
            receiver.snapshot(snapshot(&packet, 5)).unwrap_err(),
            ConfigurationError::Epoch
        );
        assert_eq!(
            receiver.message(&epoch_message(1), catalog(), fingerprint()),
            Err(ConfigurationError::Epoch)
        );
    }
}

#[test]
fn replication_configuration_roundtrips_and_resolves_all_weapon_families() {
    for id in [WeaponId::M4A1, WeaponId::M1911, WeaponId::MP5] {
        let weapon = weapon(id);
        let mut publisher = publisher();
        let handle = publisher
            .register(7, EquipmentSlot::Primary, weapon.instance())
            .unwrap();
        assert_eq!(
            publisher.register(7, EquipmentSlot::Primary, weapon.instance()),
            Ok(handle)
        );
        assert_eq!(
            publisher.entries().len(),
            1,
            "unchanged trees are published once"
        );
        let packet = &publisher.entries()[0];
        let bytes = data::encode(packet);
        println!(
            "TK CONFIG definition={} components={} bytes={}",
            id.0,
            packet.weapon.components.len(),
            bytes.len()
        );
        assert!(bytes.len() <= MAX_CONFIGURATION_BYTES);
        // Real transport frames need not be aligned.
        let mut frame = vec![0];
        frame.extend_from_slice(&bytes);
        let decoded = Configuration::decode(&frame[1..]).unwrap();
        assert_eq!(&decoded, packet);
        let mut cache = ConfigurationCache::default();
        assert_eq!(cache.begin_epoch(1), Ok(true));
        assert_eq!(
            cache.apply(catalog(), fingerprint(), decoded.clone()),
            Ok(true)
        );
        let resolved = &cache.get(1, handle).unwrap().resolved;
        assert_eq!(resolved.definition, id);
        assert_eq!(resolved.assembly, weapon.spec().assembly);
        assert_eq!(resolved.magazine_size, weapon.spec().magazine_size);
        assert_eq!(cache.apply(catalog(), fingerprint(), decoded), Ok(false));
    }
}

#[test]
fn replication_cache_waits_for_exact_reference_and_preserves_atomic_revisions() {
    let mut original = packet();
    let mut revised = original.clone();
    revised.handle = 2;
    revised.revision = 1;
    revised.weapon.components[0].node = 2;
    for order in [
        [original.clone(), revised.clone()],
        [revised.clone(), original.clone()],
    ] {
        let mut cache = ConfigurationCache::default();
        cache.begin_epoch(1).unwrap();
        assert!(cache.get(1, 1).is_none());
        assert!(cache.get(1, 2).is_none());
        for message in order {
            let handle = message.handle;
            cache
                .apply(
                    catalog(),
                    fingerprint(),
                    Configuration::decode(&data::encode(&message)).unwrap(),
                )
                .unwrap();
            assert_eq!(
                cache.get(1, handle).unwrap().packet.revision,
                message.revision
            );
        }
        assert_eq!(cache.get(1, 1).unwrap().packet.weapon.components[0].node, 1);
        assert_eq!(cache.get(1, 2).unwrap().packet.weapon.components[0].node, 2);
        let pointer = cache.get(1, 2).unwrap().resolved.assembly.parts.as_ptr();
        let mut collision = revised.clone();
        collision.weapon.components[0].node = 3;
        assert_eq!(
            cache.apply(catalog(), fingerprint(), collision.clone()),
            Err(ConfigurationError::Conflict)
        );
        collision.handle = 3;
        assert_eq!(
            cache.apply(catalog(), fingerprint(), collision),
            Err(ConfigurationError::Conflict)
        );
        let mut invalid = revised.clone();
        invalid.handle = 3;
        invalid.revision = 2;
        invalid.weapon.components[0].definition = DefinitionId(u32::MAX);
        assert!(matches!(
            cache.apply(catalog(), fingerprint(), invalid),
            Err(ConfigurationError::Invalid(_))
        ));
        assert!(cache.get(1, 3).is_none(), "no partial publication");
        assert_eq!(
            cache.get(1, 2).unwrap().resolved.assembly.parts.as_ptr(),
            pointer
        );
        assert_eq!(cache.begin_epoch(1), Ok(false));
        assert!(
            cache.get(1, 2).is_some(),
            "duplicate bootstrap cannot clear valid entries"
        );
        assert_eq!(cache.begin_epoch(2), Ok(true));
        assert!(cache.get(1, 1).is_none());
        assert!(cache.get(2, 1).is_none());
        assert_eq!(
            cache.apply(catalog(), fingerprint(), original.clone()),
            Err(ConfigurationError::Epoch)
        );
        assert_eq!(cache.begin_epoch(1), Err(ConfigurationError::Epoch));
        original.epoch = 2;
        cache
            .apply(catalog(), fingerprint(), original.clone())
            .unwrap();
        assert!(cache.get(1, 1).is_none());
        assert!(cache.get(2, 1).is_some());
        original.epoch = 1;
    }
}

#[test]
fn replication_bounds_reject_bad_archives_ids_counts_strings_and_incompatible_catalogs() {
    assert_eq!(
        Configuration::decode(&vec![0; MAX_CONFIGURATION_BYTES + 1]),
        Err(ConfigurationError::PayloadSize)
    );
    assert_eq!(
        Configuration::decode(&[0; 3]),
        Err(ConfigurationError::Archive)
    );
    for mutate in [
        |p: &mut Configuration| p.epoch = 0,
        |p: &mut Configuration| p.handle = 0,
        |p: &mut Configuration| p.instance.0 = 0,
        |p: &mut Configuration| p.weapon.components[0].node = 0,
        |p: &mut Configuration| p.weapon.components.clear(),
        |p: &mut Configuration| {
            p.weapon.components = vec![p.weapon.components[0].clone(); MAX_COMPONENTS + 1]
        },
        |p: &mut Configuration| p.weapon.components[0].socket = "s".repeat(65),
    ] {
        let mut invalid = packet();
        mutate(&mut invalid);
        assert_eq!(
            Configuration::decode(&data::encode(&invalid)),
            Err(ConfigurationError::Bounds)
        );
    }
    let mut boundary = packet();
    boundary.weapon.components = vec![boundary.weapon.components[0].clone(); MAX_COMPONENTS];
    for node in &mut boundary.weapon.components {
        node.socket = "s".repeat(64);
    }
    let bytes = data::encode(&boundary);
    assert!(
        bytes.len() < MAX_CONFIGURATION_BYTES,
        "maximum wire tree must fit"
    );
    assert!(
        Configuration::decode(&bytes).is_ok(),
        "wire bounds are inclusive"
    );
    let mut cache = ConfigurationCache::default();
    cache.begin_epoch(1).unwrap();
    assert!(
        matches!(
            cache.apply(catalog(), fingerprint(), boundary),
            Err(ConfigurationError::Invalid(_))
        ),
        "duplicate nodes and unknown sockets are semantic errors"
    );
    let mut incompatible = packet();
    incompatible.catalog ^= 1;
    assert_eq!(
        cache.apply(catalog(), fingerprint(), incompatible),
        Err(ConfigurationError::Catalog)
    );
    assert!(cache.entries.is_empty());
    let mut cycle = packet();
    cycle.weapon.components[0].parent = Some(cycle.weapon.components[0].node);
    assert!(matches!(
        cache.apply(catalog(), fingerprint(), cycle),
        Err(ConfigurationError::Invalid(_))
    ));
    assert!(cache.entries.is_empty());
    assert_ne!(
        fingerprint(),
        catalog_fingerprint(&CATALOG_SOURCE.replace("capacity: 30", "capacity: 31"))
    );
}

#[test]
fn replication_cache_capacity_and_epoch_exhaustion_are_explicit_without_eviction() {
    let mut publisher = publisher();
    let mut cache = ConfigurationCache::default();
    cache.begin_epoch(1).unwrap();
    let mut instance = weapon(WeaponId::M4A1).instance().clone();
    for i in 0..MAX_CACHED_CONFIGURATIONS {
        instance.revision = i as u64;
        let handle = publisher
            .register(7, EquipmentSlot::Primary, &instance)
            .unwrap();
        assert_eq!(handle as usize, i + 1);
        cache
            .apply(
                catalog(),
                fingerprint(),
                publisher.entries().last().unwrap().clone(),
            )
            .unwrap();
    }
    instance.revision += 1;
    assert_eq!(
        publisher.register(7, EquipmentSlot::Primary, &instance),
        Err(ConfigurationError::Capacity)
    );
    let mut overflow = publisher.entries().last().unwrap().clone();
    overflow.handle += 1;
    overflow.revision += 1;
    assert_eq!(
        cache.apply(catalog(), fingerprint(), overflow),
        Err(ConfigurationError::Capacity)
    );
    assert!(
        cache.get(1, 1).is_some(),
        "no silent eviction of referenced configuration"
    );
    assert_eq!(publisher.reset(), Ok(2));
    cache.begin_epoch(2).unwrap();
    assert_eq!(
        publisher.register(7, EquipmentSlot::Primary, &instance),
        Ok(1)
    );
    cache
        .apply(catalog(), fingerprint(), publisher.entries()[0].clone())
        .unwrap();
    assert!(cache.get(1, 1).is_none());
    let mut exhausted = ConfigurationPublisher::new(u64::MAX, fingerprint()).unwrap();
    exhausted
        .register(7, EquipmentSlot::Primary, &instance)
        .unwrap();
    assert_eq!(exhausted.reset(), Err(ConfigurationError::Capacity));
    assert_eq!(exhausted.entries().len(), 1);
    assert_eq!(exhausted.epoch(), u64::MAX);
}

fn command(arsenal: &Arsenal) -> ConfigurationCommand {
    let weapon = arsenal.current().instance();
    let mut configuration = weapon.configuration.clone();
    configuration.components[0].node = 2;
    ConfigurationCommand {
        epoch: 1,
        request: 1,
        slot: EquipmentSlot::Primary,
        instance: weapon.id,
        expected_revision: weapon.revision,
        weapon: configuration,
    }
}

#[test]
fn replication_commands_compare_and_swap_deduplicate_and_preserve_physical_state() {
    let mut arsenal = Arsenal::new(Loadout::default());
    let before = data::encode(arsenal.current().mechanics());
    let command = command(&arsenal);
    let bytes = data::encode(&command);
    println!("TK CONFIG COMMAND bytes={}", bytes.len());
    let mut ledger = CommandLedger::default();
    let result = ledger.execute(
        1,
        ConfigurationCommand::decode(&bytes).unwrap(),
        true,
        &mut arsenal,
        catalog(),
    );
    assert_eq!(result.outcome, CommandOutcome::Applied);
    assert_eq!(result.revision, Some(1));
    assert_eq!(
        arsenal.current().instance().configuration.components[0].node,
        2
    );
    assert_eq!(data::encode(arsenal.current().mechanics()), before);
    let pointer = arsenal.current().spec().assembly.parts.as_ptr();
    assert_eq!(
        ledger.execute(1, command.clone(), false, &mut arsenal, catalog()),
        result,
        "duplicate returns original result even after the lobby locks"
    );
    assert_eq!(arsenal.current().spec().assembly.parts.as_ptr(), pointer);
    let mut conflicting = command.clone();
    conflicting.weapon.components[0].node = 3;
    assert_eq!(
        ledger
            .execute(1, conflicting, true, &mut arsenal, catalog())
            .outcome,
        CommandOutcome::RequestConflict
    );
    let mut stale = command.clone();
    stale.request = 2;
    assert_eq!(
        ledger
            .execute(1, stale, true, &mut arsenal, catalog())
            .outcome,
        CommandOutcome::StaleRevision
    );
    assert_eq!(
        ledger
            .execute(1, command.clone(), true, &mut arsenal, catalog())
            .outcome,
        CommandOutcome::StaleRequest
    );
    assert_eq!(arsenal.current().instance().revision, 1);
    let mut previous_epoch = command;
    previous_epoch.request = u64::MAX;
    assert_eq!(
        ledger
            .execute(2, previous_epoch.clone(), true, &mut arsenal, catalog())
            .outcome,
        CommandOutcome::EpochMismatch
    );
    previous_epoch.epoch = 2;
    previous_epoch.request = 1;
    previous_epoch.expected_revision = 1;
    assert_eq!(
        ledger
            .execute(2, previous_epoch, true, &mut arsenal, catalog())
            .outcome,
        CommandOutcome::Applied
    );
    assert_eq!(
        arsenal.current().instance().revision,
        1,
        "identical configuration is a no-op"
    );
}

#[test]
fn replication_commands_reject_forged_ownership_locks_and_invalid_assemblies_atomically() {
    let mut arsenal = Arsenal::new(Loadout::default());
    let original = data::encode(arsenal.current().instance());
    let base = command(&arsenal);
    let mut ledger = CommandLedger::default();
    let mut forged = base.clone();
    forged.instance.0 += 10_000;
    assert_eq!(
        ledger
            .execute(1, forged, true, &mut arsenal, catalog())
            .outcome,
        CommandOutcome::NotOwned
    );
    let mut locked = base.clone();
    locked.request = 2;
    assert_eq!(
        ledger
            .execute(1, locked, false, &mut arsenal, catalog())
            .outcome,
        CommandOutcome::Locked
    );
    let mut invalid = base.clone();
    invalid.request = 3;
    invalid.weapon.components[0].definition = DefinitionId(u32::MAX);
    assert_eq!(
        ledger
            .execute(1, invalid.clone(), true, &mut arsenal, catalog())
            .outcome,
        CommandOutcome::InvalidConfiguration
    );
    assert_eq!(data::encode(arsenal.current().instance()), original);
    invalid.request = 4;
    invalid.weapon.components[0].socket = "s".repeat(65);
    assert_eq!(
        ConfigurationCommand::decode(&data::encode(&invalid)),
        Err(ConfigurationError::Bounds)
    );
    assert_eq!(
        ConfigurationCommand::decode(&vec![0; MAX_CONFIGURATION_BYTES + 1]),
        Err(ConfigurationError::PayloadSize)
    );
    assert_eq!(
        ConfigurationCommand::decode(&[0; 3]),
        Err(ConfigurationError::Archive)
    );
}
