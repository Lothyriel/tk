use super::{aim_barrel, capture, head_target};
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
}

#[derive(Default)]
pub(super) struct Run {
    step: Step,
    raid_id: u32,
    full_ammo: u32,
    bot_life: u8,
    player_life: u8,
    until: f64,
}

pub(super) fn solo(
    mut commands: Commands,
    mut run: Local<Run>,
    time: Res<Time<Real>>,
    session: Res<ClientSession>,
    id: Res<PlayerId>,
    mut input: ResMut<LocalInput>,
    mut exit: MessageWriter<AppExit>,
) {
    let now = time.elapsed_secs_f64();
    assert!(now < 100.0, "Arena route timed out at {:?}", run.step);
    input.fire = false;
    let Some(player) = session.local(id.0) else {
        return;
    };
    let mut bots = session.players.iter().filter(|p| p.bot);
    let bot = bots.next();
    assert!(bots.next().is_none(), "More than one Arena bot");
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
            input.lobby_sequence = input.lobby_sequence.wrapping_add(1);
            info!("ARENA LOBBY CLEAN raid={}", session.raid.raid_id);
            run.step = Step::Reenter;
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
