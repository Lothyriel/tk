use super::{aim_barrel, capture, head_target};
use crate::sync::ClientSession;
use bevy::prelude::*;
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
