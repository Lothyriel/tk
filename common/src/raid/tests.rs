use super::*;

// ALPHA-03: solo and all-ready countdown, cancellation and exact raid duration.
#[test]
fn solo_ready_starts_three_second_countdown_then_five_minute_raid() {
    let mut raid = RaidState::default();
    assert!(!raid.advance_lobby(1, 1, 0.1));
    assert_eq!(raid.phase, RaidPhase::Countdown);
    assert_eq!(raid.seconds_left, 3.0);
    assert!(!raid.advance_lobby(1, 1, 2.0));
    assert_eq!(raid.seconds_left, 1.0);
    assert!(raid.advance_lobby(1, 1, 1.0));
    assert_eq!(raid.phase, RaidPhase::Active);
    assert_eq!(raid.seconds_left, 300.0);
    assert_eq!(raid.raid_id, 1);
}

#[test]
fn unready_cancels_countdown_and_empty_lobby_never_starts() {
    let mut raid = RaidState::default();
    raid.advance_lobby(1, 2, 10.0);
    assert_eq!(raid.phase, RaidPhase::Lobby);
    raid.advance_lobby(0, 0, 10.0);
    assert_eq!(raid.phase, RaidPhase::Lobby);
    raid.advance_lobby(2, 2, 0.1);
    raid.advance_lobby(1, 2, 3.0);
    assert_eq!(raid.phase, RaidPhase::Lobby);
    assert_eq!(raid.seconds_left, 0.0);
}

fn active_raid() -> RaidState {
    let mut raid = RaidState::default();
    raid.advance_lobby(1, 1, 0.1);
    raid.advance_lobby(1, 1, 3.0);
    raid
}

// ALPHA-09: server-shared crate availability and proximity.
#[test]
fn loot_requires_live_participant_interaction_and_proximity_and_is_unique() {
    let mut raid = active_raid();
    let mut player = Participant::default();
    player.begin();
    let crate_pos = Vec3::from(LOOT_POSITIONS[0]);
    assert!(!player.try_loot(&mut raid, crate_pos + Vec3::X * 3.0, true));
    assert!(!player.try_loot(&mut raid, crate_pos, false));
    assert!(player.try_loot(&mut raid, crate_pos, true));
    assert_eq!(player.carried, 1);
    assert!(!raid.loot_available[0]);
    assert!(!player.try_loot(&mut raid, crate_pos, true));
    let mut other = Participant::default();
    other.begin();
    assert!(!other.try_loot(&mut raid, crate_pos, true));
    assert_eq!(other.carried, 0);
}

// ALPHA-11: five continuous seconds, credit exactly once, no empty-handed extract.
#[test]
fn extraction_credits_once_after_five_seconds() {
    let raid = active_raid();
    let mut player = Participant::default();
    player.begin();
    player.carried = 2;
    let exit = Vec3::from(EXTRACTION_CENTER);
    assert!(!player.tick_extraction(&raid, exit, true, 4.0));
    assert_eq!(player.extraction_seconds, 4.0);
    assert_eq!(player.secured, 0);
    assert!(player.tick_extraction(&raid, exit, true, 1.0));
    assert_eq!(player.status, RaidStatus::Extracted);
    assert_eq!(player.secured, 2);
    assert_eq!(player.carried, 0);
    assert!(!player.tick_extraction(&raid, exit, true, 10.0));
    assert_eq!(player.secured, 2);
}

#[test]
fn leaving_releasing_or_moving_above_exit_resets_progress() {
    let raid = active_raid();
    let exit = Vec3::from(EXTRACTION_CENTER);
    let mut player = Participant::default();
    player.begin();
    player.carried = 1;
    player.tick_extraction(&raid, exit, true, 4.0);
    player.tick_extraction(&raid, exit, false, 1.0);
    assert_eq!(player.extraction_seconds, 0.0);
    player.tick_extraction(&raid, exit, true, 4.0);
    player.tick_extraction(&raid, exit + Vec3::X * 3.0, true, 1.0);
    assert_eq!(player.extraction_seconds, 0.0);
    player.tick_extraction(&raid, exit, true, 4.0);
    player.tick_extraction(&raid, exit + Vec3::Y * 3.0, true, 1.0);
    assert_eq!(player.extraction_seconds, 0.0);
    assert_eq!(player.secured, 0);
}

#[test]
fn dead_waiting_empty_and_non_active_cannot_extract_or_loot() {
    let mut raid = active_raid();
    let exit = Vec3::from(EXTRACTION_CENTER);
    let mut player = Participant {
        carried: 1,
        ..Default::default()
    };
    assert!(!player.tick_extraction(&raid, exit, true, 5.0));
    assert!(!player.try_loot(&mut raid, Vec3::from(LOOT_POSITIONS[0]), true));
    player.begin();
    assert!(!player.tick_extraction(&raid, exit, true, 5.0));
    player.carried = 1;
    player.fail(RaidStatus::Dead);
    assert!(!player.tick_extraction(&raid, exit, true, 5.0));
    assert_eq!(player.carried, 0);
    player.begin();
    player.carried = 1;
    raid.phase = RaidPhase::Lobby;
    assert!(!player.tick_extraction(&raid, exit, true, 5.0));
    assert!(!player.try_loot(&mut raid, Vec3::from(LOOT_POSITIONS[0]), true));
    assert_eq!(player.secured, 0);
}

// ALPHA-12/13: failure and another raid without duplicating secured loot.
#[test]
fn timeout_loses_carried_loot_and_debrief_waits_for_all_participants() {
    let mut raid = active_raid();
    assert!(!raid.advance_active(299.0));
    assert!(raid.advance_active(1.0));
    assert_eq!(raid.seconds_left, 0.0);
    let mut player = Participant::default();
    player.begin();
    player.carried = 3;
    player.fail(RaidStatus::Missing);
    assert_eq!(player.status, RaidStatus::Missing);
    assert_eq!(player.carried, 0);
    assert_eq!(player.secured, 0);
    raid.finish_if_complete(1);
    assert_eq!(raid.phase, RaidPhase::Active);
    raid.finish_if_complete(0);
    assert_eq!(raid.phase, RaidPhase::Debrief);
}

#[test]
fn next_raid_restocks_loot_and_preserves_secured_total_without_auto_ready() {
    let mut raid = active_raid();
    let mut player = Participant::default();
    player.begin();
    player.carried = 1;
    player.tick_extraction(&raid, Vec3::from(EXTRACTION_CENTER), true, 5.0);
    raid.loot_available[0] = false;
    raid.finish_if_complete(0);
    raid.reset_lobby();
    player.return_to_lobby();
    assert_eq!(raid.phase, RaidPhase::Lobby);
    assert_eq!(player.status, RaidStatus::Waiting);
    assert!(!player.ready);
    assert_eq!(player.secured, 1);
    raid.advance_lobby(1, 1, 0.1);
    raid.advance_lobby(1, 1, 3.0);
    player.begin();
    assert_eq!(raid.raid_id, 2);
    assert_eq!(raid.loot_available, [true; 3]);
    assert_eq!(player.carried, 0);
    assert_eq!(player.secured, 1);
}
