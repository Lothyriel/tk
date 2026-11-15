use super::*;

fn state(weapon: WeaponId) -> MechanicalState {
    MechanicalState::new(
        catalog(),
        weapon.spec(),
        [11, 12, 13, 14].map(MagazineInstanceId),
    )
    .unwrap()
}

fn valid(state: &MechanicalState, weapon: WeaponId) {
    state
        .validate(catalog(), weapon.spec())
        .unwrap_or_else(|err| panic!("{err}: {state:?}"));
    let restored: MechanicalState = crate::data::decode(&crate::data::encode(state));
    restored.validate(catalog(), weapon.spec()).unwrap();
    assert_eq!(&restored, state);
}

fn shoot(state: &mut MechanicalState, shots: u32) {
    for _ in 0..shots {
        assert!(state.fire(true, true));
    }
}

#[test]
fn mechanics_three_weapons_move_real_round_into_chamber_and_retain_magazines() {
    for (weapon, capacity) in [
        (WeaponId::M4A1, 30),
        (WeaponId::M1911, 17),
        (WeaponId::MP5, 25),
    ] {
        let mut state = state(weapon);
        assert_eq!(state.loaded_rounds(), capacity);
        assert_eq!(state.inserted().unwrap().rounds, capacity - 1);
        assert_eq!(state.reserve_rounds(), capacity * 3);
        assert_eq!(state.total_rounds(), capacity * 4);
        shoot(&mut state, 3);
        assert!(state.start_reload(2.0));
        assert_eq!(
            state.action().unwrap().kind,
            ManipulationKind::TacticalReload
        );
        state.advance(0.5);
        assert_eq!(state.action().unwrap().phase, ManipulationPhase::Insert);
        assert!(state.inserted().is_none());
        assert!(state.chamber().is_some());
        let old = state
            .magazines()
            .iter()
            .find(|m| m.id == MagazineInstanceId(11))
            .unwrap();
        assert_eq!(old.rounds, capacity - 4);
        assert!(!state.fire(true, true));
        state.advance(0.9);
        assert_eq!(state.action().unwrap().phase, ManipulationPhase::Finish);
        assert_eq!(state.inserted().unwrap().id, MagazineInstanceId(12));
        assert_eq!(state.loaded_rounds(), capacity + 1);
        assert_eq!(state.total_rounds(), capacity * 4 - 3);
        state.advance(1.0);
        assert!(state.action().is_none());
        valid(&state, weapon);
    }
}

#[test]
fn mechanics_empty_reload_requires_feed_and_retains_an_empty_magazine() {
    let weapon = WeaponId::M4A1;
    let mut state = state(weapon);
    shoot(&mut state, 30);
    assert_eq!(state.loaded_rounds(), 0);
    assert!(state.bolt_open());
    assert!(!state.fire(true, true));
    assert!(state.start_reload(10.0));
    state.advance(7.0);
    assert_eq!(state.action().unwrap().phase, ManipulationPhase::Feed);
    assert!(state.chamber().is_none());
    assert_eq!(state.inserted().unwrap().rounds, 30);
    assert!(!state.fire(true, true));
    valid(&state, weapon);
    state.advance(2.0);
    assert_eq!(state.action().unwrap().phase, ManipulationPhase::Finish);
    assert_eq!(state.inserted().unwrap().rounds, 29);
    assert!(state.chamber().is_some());
    assert!(!state.bolt_open());
    state.advance(1.1);
    assert!(state.fire(true, true));
    assert_eq!(state.total_rounds(), 89);
    assert_eq!(state.magazines().len(), 4);
    valid(&state, weapon);
}

#[test]
fn mechanics_interrupt_every_commit_preserves_identity_location_and_conservation() {
    for empty in [false, true] {
        for time in [0.0, 2.49, 2.5, 2.51, 6.99, 7.0, 7.01, 8.99, 9.0, 9.01, 10.0] {
            let mut state = state(WeaponId::M4A1);
            let shots = if empty { 30 } else { 5 };
            shoot(&mut state, shots);
            assert!(state.start_reload(10.0));
            state.advance(time);
            let before = state.magazines().to_vec();
            let chamber = state.chamber();
            let inserted = state.inserted().map(|m| m.id);
            state.cancel();
            assert_eq!(state.magazines(), before);
            assert_eq!(state.chamber(), chamber);
            assert_eq!(state.inserted().map(|m| m.id), inserted);
            assert_eq!(state.total_rounds() + shots, 120);
            valid(&state, WeaponId::M4A1);
            if state.start_reload(10.0) {
                state.advance(20.0);
            }
            assert!(state.fire(true, true));
            assert_eq!(state.total_rounds() + shots + 1, 120);
            valid(&state, WeaponId::M4A1);
        }
    }
}

#[test]
fn mechanics_inspect_yields_to_fire_reload_and_switch_but_equip_blocks_fire() {
    let mut state = state(WeaponId::M4A1);
    let before = state.total_rounds();
    assert!(state.start_inspect(2.0));
    state.advance(0.5);
    assert_eq!(state.total_rounds(), before);
    assert!(state.fire(true, true));
    assert!(state.action().is_none());
    assert!(state.start_inspect(2.0));
    assert!(state.start_reload(2.0));
    assert_eq!(
        state.action().unwrap().kind,
        ManipulationKind::TacticalReload
    );
    state.advance(0.6); // removal committed
    assert!(state.start_unequip(0.2));
    assert!(!state.fire(true, true));
    state.advance(0.2);
    assert!(!state.equipped());
    assert!(!state.fire(true, true));
    assert!(state.start_equip(0.3));
    state.advance(0.29);
    assert!(!state.fire(true, true));
    state.advance(0.02);
    assert!(state.equipped());
    assert!(state.fire(true, true)); // retained chamber, no inserted magazine
    assert!(state.bolt_open());
    assert_eq!(state.total_rounds(), before - 2);
    assert!(state.start_reload(2.0));
    state.advance(2.0);
    valid(&state, WeaponId::M4A1);
}

#[test]
fn mechanics_selectors_respect_profile_and_empty_pool_never_creates_rounds() {
    for weapon in [WeaponId::M4A1, WeaponId::M1911, WeaponId::MP5] {
        let mut state = state(weapon);
        state.cycle_selector();
        assert_eq!(state.mode(), FireMode::Safe);
        assert!(!state.fire(true, true));
        state.cycle_selector();
        assert_eq!(state.mode(), FireMode::Semi);
        assert!(!state.fire(true, false));
        assert!(state.fire(false, true));
        state.cycle_selector();
        assert_eq!(
            state.mode(),
            if weapon.spec().automatic {
                FireMode::Auto
            } else {
                FireMode::Safe
            }
        );
        if state.mode() == FireMode::Safe {
            state.cycle_selector();
        }
        let initial = weapon.spec().magazine_size * 4;
        let mut fired = 1;
        for _ in 0..200 {
            if state.fire(true, true) {
                fired += 1;
            } else if state.start_reload(1.0) {
                state.advance(2.0);
            } else {
                break;
            }
            assert_eq!(state.total_rounds() + fired, initial);
        }
        assert_eq!(fired, initial);
        assert_eq!(state.total_rounds(), 0);
        assert!(!state.start_reload(1.0));
        valid(&state, weapon);
    }
}

#[test]
fn mechanics_generated_sequence_and_partitioned_time_preserve_commits() {
    let mut whole = state(WeaponId::M4A1);
    shoot(&mut whole, 30);
    let mut partitioned = whole.clone();
    assert!(whole.start_reload(2.0));
    assert!(partitioned.start_reload(2.0));
    whole.advance(2.01);
    for _ in 0..201 {
        partitioned.advance(0.01);
    }
    assert_eq!(whole, partitioned);
    let mut state = state(WeaponId::M4A1);
    let mut seed = 7_u64;
    let mut fired = 0;
    let mut sequence = 0;
    for _ in 0..10_000 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        match seed % 7 {
            0 => {
                fired += u32::from(state.fire(true, true));
            }
            1 => {
                state.start_reload(1.0);
            }
            2 => {
                state.start_inspect(0.4);
            }
            3 => state.cancel(),
            4 => {
                if state.equipped() {
                    state.start_unequip(0.2);
                } else {
                    state.start_equip(0.3);
                }
            }
            5 => state.cycle_selector(),
            _ => state.advance((seed % 50) as f32 / 100.0),
        }
        assert_eq!(state.total_rounds() + fired, 120);
        assert!(state.action_sequence() >= sequence);
        sequence = state.action_sequence();
        valid(&state, WeaponId::M4A1);
    }
}

#[test]
fn mechanics_rejects_malformed_restored_state_and_invalid_clocks_without_mutation() {
    type Mutation = fn(&mut MechanicalState);
    let mutations: &[Mutation] = &[
        |s| s.magazines[1].id = s.magazines[0].id,
        |s| s.magazines[0].rounds = 31,
        |s| s.magazines[0].capacity = 31,
        |s| s.magazines[0].definition = DefinitionId(9999),
        |s| s.inserted = Some(MagazineInstanceId(9999)),
        |s| s.chamber = Some(DefinitionId(9999)),
        |s| s.bolt_open = true,
        |s| s.automatic = false,
    ];
    for mutate in mutations {
        let mut s = state(WeaponId::M4A1);
        mutate(&mut s);
        assert!(s.validate(catalog(), WeaponId::M4A1.spec()).is_err());
    }
    let mut s = state(WeaponId::M4A1);
    shoot(&mut s, 5);
    let before = s.clone();
    for duration in [f32::NAN, f32::INFINITY, -1.0, 0.0] {
        assert!(!s.start_reload(duration));
        assert_eq!(s, before);
    }
    assert!(s.start_reload(2.0));
    let before = s.clone();
    for delta in [f32::NAN, f32::INFINITY, -1.0, 0.0] {
        s.advance(delta);
        assert_eq!(s, before);
    }
    s.action.as_mut().unwrap().elapsed = f32::NAN;
    assert!(s.validate(catalog(), WeaponId::M4A1.spec()).is_err());
}
