use super::{aim_barrel, capture, head_target, move_toward};
use crate::sync::ClientSession;
use bevy::prelude::*;
use bevy_renet2::prelude::RenetClient;
use common::{EquipmentSlot, LocalInput, PlayerId, raid::*};

#[derive(Debug, Default)]
pub(super) enum Step {
    #[default]
    Ready,
    Fight,
    BotReturns,
    PlayerDies,
    PlayerReturns,
    Leave,
    BackInLobby,
    Reenter,
    Finish,
    RaidReady,
    RaidDeploy,
    RaidActive,
    RaidDebrief {
        since: f32,
        snapshot: u64,
    },
}

#[derive(Default)]
pub(super) struct Run {
    step: Step,
    raid_id: u32,
    full_ammo: u32,
    bot_life: u8,
    player_life: u8,
    until: f64,
    arena_return: (u64, u64),
    normal_raid: u32,
    normal_life: u8,
    raid_waypoint: usize,
}

type RaidObservation<'w, 's> = (
    Res<'w, RenetClient>,
    Res<'w, crate::menu::Menu>,
    Single<'w, 's, &'static Text, With<crate::menu::Status>>,
);

pub(super) fn solo(
    mut commands: Commands,
    mut run: Local<Run>,
    (time, session, id): (Res<Time<Real>>, Res<ClientSession>, Res<PlayerId>),
    mut input: ResMut<LocalInput>,
    mut exit: MessageWriter<AppExit>,
    (network, menu, status): RaidObservation,
) {
    let now = time.elapsed_secs_f64();
    assert!(now < 100.0, "Arena route timed out at {:?}", run.step);
    let raid_after = std::env::var_os("TK_PLAYTEST_ARENA_THEN_RAID").is_some();
    if raid_after && run.arena_return.1 != 0 {
        assert!(network.is_connected());
        assert_eq!(id.0, run.arena_return.0);
        assert!(session.last_sequence >= run.arena_return.1);
        assert!(
            now - session.last_received < 1.0,
            "Authoritative snapshots stopped"
        );
    }
    input.fire = false;
    let Some(player) = session.local(id.0) else {
        return;
    };
    let mut bots = session.players.iter().filter(|p| p.bot);
    let bot = bots.next();
    if session.raid.mode.is_arena() {
        assert!(bots.next().is_none(), "More than one Arena bot");
    }
    match run.step {
        Step::Ready if session.raid.phase == RaidPhase::Lobby => {
            input.mode = MatchMode::ArenaSolo;
            if session.raid.mode == MatchMode::ArenaSolo {
                input.lobby_sequence = input.lobby_sequence.wrapping_add(1);
                run.step = Step::Fight;
            }
        }
        Step::Fight if session.raid.phase == RaidPhase::Active => {
            assert_eq!(session.raid.mode, MatchMode::ArenaSolo);
            let bot = bot.expect("Arena must replicate its opponent");
            if run.full_ammo == 0 {
                run.full_ammo = player.ammo_in_mag;
                run.raid_id = session.raid.raid_id;
                run.bot_life = bot.life;
                run.player_life = player.life;
            }
            assert!(session.playing(id.0), "Player died before the bot");
            if bot.participant.status == RaidStatus::Dead {
                assert!(player.ammo_in_mag < run.full_ammo);
                info!("ARENA BOT DEATH life={}", bot.life);
                run.step = Step::BotReturns;
            } else {
                input.weapon = EquipmentSlot::Primary;
                aim_barrel(&mut input, player, head_target(bot));
                input.fire = true;
            }
        }
        Step::BotReturns => {
            let bot = bot.expect("Arena must retain the bot identity");
            if bot.life != run.bot_life {
                assert!(bot.body.alive());
                assert_eq!(bot.participant.status, RaidStatus::Active);
                info!("ARENA BOT RETURN life={}", bot.life);
                run.step = Step::PlayerDies;
            }
        }
        Step::PlayerDies if player.participant.status == RaidStatus::Dead => {
            capture(&mut commands, "arena-eliminated");
            assert!(!session.playing(id.0));
            info!("ARENA PLAYER DEATH life={}", player.life);
            run.step = Step::PlayerReturns;
        }
        Step::PlayerReturns if session.playing(id.0) => {
            assert_ne!(player.life, run.player_life);
            assert_eq!(player.body.parts, [35.0, 85.0, 60.0, 70.0]);
            assert_eq!(player.ammo_in_mag, run.full_ammo);
            assert_eq!(session.raid.raid_id, run.raid_id);
            capture(&mut commands, "arena-respawn");
            info!("ARENA PLAYER RETURN life={}", player.life);
            if raid_after {
                run.arena_return = (id.0, session.last_sequence);
                info!(
                    "ARENA RAID handoff player={} arena={} snapshot={}",
                    id.0, run.raid_id, session.last_sequence
                );
            }
            run.until = now + 0.25;
            run.step = Step::Leave;
        }
        Step::Leave if now >= run.until => {
            input.arena_leave = true;
            input.lobby_sequence = input.lobby_sequence.wrapping_add(1);
            run.step = Step::BackInLobby;
        }
        Step::BackInLobby if session.raid.phase == RaidPhase::Lobby => {
            assert!(bot.is_none(), "Bot leaked into the lobby");
            assert!(!player.participant.ready && !session.playing(id.0));
            input.arena_leave = false;
            info!("ARENA LOBBY CLEAN raid={}", session.raid.raid_id);
            if raid_after {
                input.mode = MatchMode::Raid;
                run.step = Step::RaidReady;
            } else {
                input.lobby_sequence = input.lobby_sequence.wrapping_add(1);
                run.step = Step::Reenter;
            }
        }
        Step::Reenter if session.playing(id.0) => {
            assert_ne!(session.raid.raid_id, run.raid_id);
            assert_eq!(session.raid.mode, MatchMode::ArenaSolo);
            assert_eq!(player.ammo_in_mag, run.full_ammo);
            assert_eq!(bot.unwrap().participant.status, RaidStatus::Active);
            capture(&mut commands, "arena-reentry");
            run.until = now + 0.5;
            run.step = Step::Finish;
        }
        Step::Finish if now >= run.until => {
            info!("ARENA SOLO PASS second_session={}", session.raid.raid_id);
            exit.write(AppExit::Success);
        }
        Step::RaidReady => {
            assert_eq!(session.raid.phase, RaidPhase::Lobby);
            assert!(!player.participant.ready);
            input.mode = MatchMode::Raid;
            if session.raid.mode == MatchMode::Raid {
                input.lobby_sequence = input.lobby_sequence.wrapping_add(1);
                run.step = Step::RaidDeploy;
            }
        }
        Step::RaidDeploy => {
            input.mode = MatchMode::Raid;
            if session.playing(id.0) {
                assert_eq!(session.raid.mode, MatchMode::Raid);
                assert_ne!(session.raid.raid_id, run.raid_id);
                assert!(session.last_sequence > run.arena_return.1);
                assert_eq!(
                    session.players.iter().filter(|p| p.bot).count(),
                    common::map::BOT_POSITIONS.len()
                );
                assert_eq!((player.life, player.respawn_ticks), (0, 0));
                run.normal_raid = session.raid.raid_id;
                run.normal_life = player.life;
                run.step = Step::RaidActive;
                info!(
                    "ARENA RAID active player={} raid={} life={} snapshot={}",
                    id.0, run.normal_raid, player.life, session.last_sequence
                );
            }
        }
        Step::RaidActive => {
            assert_eq!(session.raid.mode, MatchMode::Raid);
            assert_eq!(session.raid.raid_id, run.normal_raid);
            assert_eq!((player.life, player.respawn_ticks), (run.normal_life, 0));
            if player.participant.status == RaidStatus::Dead {
                assert_eq!(session.raid.phase, RaidPhase::Debrief);
                assert!(!player.body.alive() && !session.playing(id.0));
                run.step = Step::RaidDebrief {
                    since: session.simulation_time,
                    snapshot: session.last_sequence,
                };
                info!(
                    "ARENA RAID dead player={} raid={} life={} snapshot={}",
                    id.0, run.normal_raid, player.life, session.last_sequence
                );
            } else {
                assert!(session.playing(id.0));
                // Walk a short route through the stock raid and let its ordinary
                // simulation reach the death result. No new combat input or fixture.
                let [x, _, z] = common::map::BOT_POSITIONS[1];
                let waypoints = [[x - 1.0, 0.0, z + 1.5], [x - 1.0, 0.0, z]];
                if let Some(&target) = waypoints.get(run.raid_waypoint)
                    && move_toward(&mut input, player.pos, target) < 0.4
                {
                    run.raid_waypoint += 1;
                    info!(
                        "ARENA RAID waypoint={} pos={:?}",
                        run.raid_waypoint, player.pos
                    );
                }
            }
        }
        Step::RaidDebrief { since, snapshot } => {
            assert_eq!(session.raid.mode, MatchMode::Raid);
            assert_eq!(session.raid.raid_id, run.normal_raid);
            assert_eq!(session.raid.phase, RaidPhase::Debrief);
            assert_eq!(player.participant.status, RaidStatus::Dead);
            assert_eq!((player.life, player.respawn_ticks), (run.normal_life, 0));
            assert!(!player.body.alive() && !session.playing(id.0));
            // Input runs before Present; this is the real menu from the previous frame.
            assert!(menu.owns_input());
            let status = status.as_str();
            assert!(status.starts_with("DEBRIEF\nResult: Dead |"), "{status:?}");
            assert!(!status.contains("Respawn"), "{status:?}");
            let elapsed = session.simulation_time - since;
            if elapsed >= ARENA_RESPAWN_SECONDS + 1.0 {
                assert!(session.last_sequence > snapshot);
                info!(
                    "ARENA RAID NO RESPAWN PASS player={} raid={} life={} snapshot={} elapsed={elapsed:.3} status={:?}",
                    id.0, run.normal_raid, player.life, session.last_sequence, status
                );
                exit.write(AppExit::Success);
            }
        }
        _ => (),
    }
}

#[derive(Default)]
pub(super) struct Duel {
    ready_sent: bool,
    lives: [Option<u8>; 2],
    saw_death: [bool; 2],
    finished_at: Option<f64>,
}

pub(super) fn pvp(
    mut commands: Commands,
    mut duel: Local<Duel>,
    time: Res<Time<Real>>,
    (session, id): (Res<ClientSession>, Res<PlayerId>),
    mut input: ResMut<LocalInput>,
    mut network: ResMut<RenetClient>,
    mut exit: MessageWriter<AppExit>,
) {
    let now = time.elapsed_secs_f64();
    assert!(
        now < 65.0,
        "PvP route timed out; deaths={:?}",
        duel.saw_death
    );
    let peer = std::env::var("TK_PLAYTEST_ROLE").as_deref() == Ok("1");
    let role = usize::from(peer);
    input.fire = false;
    if peer && duel.finished_at.is_some_and(|at| now - at > 1.0) {
        if !network.is_disconnected() {
            network.disconnect();
        }
        if duel.finished_at.is_some_and(|at| now - at > 1.5) {
            info!("ARENA PVP PASS peer disconnected");
            exit.write(AppExit::Success);
        }
        return;
    }
    let Some(player) = session.local(id.0) else {
        return;
    };
    if session.raid.phase == RaidPhase::Lobby {
        if !duel.ready_sent {
            input.mode = MatchMode::ArenaPvp;
            input.lobby_sequence = input.lobby_sequence.wrapping_add(1);
            duel.ready_sent = true;
        } else if duel.finished_at.is_some() && !peer {
            assert_eq!(session.players.len(), 1);
            assert!(!player.participant.ready);
            info!("ARENA PVP PASS survivor returned to clean lobby");
            exit.write(AppExit::Success);
        }
        return;
    }
    if session.raid.phase != RaidPhase::Active {
        return;
    }
    assert_eq!(session.raid.mode, MatchMode::ArenaPvp);
    assert_eq!(session.players.len(), 2);
    assert!(session.players.iter().all(|actor| !actor.bot));
    let other = session
        .players
        .iter()
        .find(|actor| actor.id != id.0)
        .unwrap();
    let roster = if peer {
        [other, player]
    } else {
        [player, other]
    };
    for (index, actor) in roster.into_iter().enumerate() {
        let initial = *duel.lives[index].get_or_insert(actor.life);
        if actor.participant.status == RaidStatus::Dead && !duel.saw_death[index] {
            duel.saw_death[index] = true;
            capture(
                &mut commands,
                if index == role {
                    "pvp-local-death"
                } else {
                    "pvp-remote-death"
                },
            );
            info!("ARENA PVP DEATH role={role} target={index} life={initial}");
        }
    }
    let returned = |index: usize| {
        duel.saw_death[index]
            && Some(roster[index].life) != duel.lives[index]
            && roster[index].participant.status == RaidStatus::Active
            && roster[index].body.alive()
    };
    if returned(0) && returned(1) {
        assert!(session.playing(id.0));
        if duel.finished_at.is_none() {
            assert_eq!(player.body.parts, [35.0, 85.0, 60.0, 70.0]);
            capture(&mut commands, "pvp-both-returned");
            duel.finished_at = Some(now);
            info!(
                "ARENA PVP RETURNS role={role} lives={}/{}",
                roster[0].life, roster[1].life
            );
        }
    } else if (peer && !duel.saw_death[0]) || (!peer && returned(0) && !duel.saw_death[1]) {
        input.weapon = EquipmentSlot::Primary;
        aim_barrel(&mut input, player, head_target(other));
        input.fire = true;
    }
}
