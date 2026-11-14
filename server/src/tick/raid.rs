use bevy::prelude::*;
use common::{combat::*, map::*, raid::*, *};

use super::{Bot, Projectile, WorldState, character_bundle, set_crouched_state};
use bevy_rapier3d::prelude::*;

pub(super) fn lobby_tick(
    mut commands: Commands,
    time: Res<Time>,
    mut raid: ResMut<RaidState>,
    mut world_state: ResMut<WorldState>,
    mut players: Query<
        (
            &Client,
            &ClientInput,
            &mut Participant,
            &mut BodyHealth,
            &mut ArmorState,
            &mut Arsenal,
            &mut MovementState,
            &mut Collider,
            &mut Transform,
        ),
        Without<Bot>,
    >,
    bots: Query<Entity, With<Bot>>,
    projectiles: Query<Entity, With<Projectile>>,
) {
    let previous_phase = raid.phase;
    let mut return_requested = false;
    for (_, input, mut player, ..) in &mut players {
        if raid.phase == RaidPhase::Lobby && !player.ready {
            player.loadout = input.loadout.validated();
        }
        if player.last_command == input.lobby_sequence {
            continue;
        }
        player.last_command = input.lobby_sequence;
        match raid.phase {
            RaidPhase::Lobby | RaidPhase::Countdown => player.ready = !player.ready,
            RaidPhase::Debrief => return_requested = true,
            RaidPhase::Active => (),
        }
    }

    if players.is_empty() || return_requested {
        raid.reset_lobby();
        for (_, _, mut player, ..) in &mut players {
            player.return_to_lobby();
        }
    }

    let total = players.iter().count();
    let ready = players.iter().filter(|(_, _, p, ..)| p.ready).count();
    let started = raid.advance_lobby(ready, total, time.delta_secs());
    if started {
        // Sort by stable network id; query iteration order is not a spawn allocation policy.
        let mut ids: Vec<_> = players.iter().map(|(client, ..)| client.id).collect();
        ids.sort_unstable();
        for (
            client,
            input,
            mut player,
            mut body,
            mut armor,
            mut arsenal,
            mut movement,
            mut collider,
            mut transform,
        ) in &mut players
        {
            player.begin();
            *body = BodyHealth::default();
            *armor = ArmorState::from_loadout(player.loadout);
            *arsenal = Arsenal::new(player.loadout);
            arsenal.synchronize_sequences(input);
            movement.velocity = Vec3::ZERO;
            set_crouched_state(&mut movement, &mut collider, &mut transform, false);
            let index = ids.iter().position(|id| *id == client.id).unwrap();
            transform.translation = Vec3::from(SPAWN_CENTER)
                + Vec3::new(
                    (index % 8) as f32 * 1.5 - 0.75 * (total.min(8) - 1) as f32,
                    0.0,
                    (index / 8) as f32 * 1.5,
                );
            transform.rotation = Quat::IDENTITY;
        }
    }

    if started
        || (raid.phase == RaidPhase::Lobby && previous_phase != RaidPhase::Lobby)
        || total == 0
    {
        for entity in &bots {
            commands.entity(entity).despawn();
        }
        for entity in &projectiles {
            commands.entity(entity).despawn();
        }
        world_state.impact_marks.clear();
        world_state.fired_projectiles.clear();
    }
    if started {
        for (i, position) in BOT_POSITIONS.into_iter().enumerate() {
            let loadout = Loadout {
                primary: WeaponKind::Smg,
                armor: ArmorKind::Light,
                helmet: false,
                ..default()
            };
            let mut participant = Participant {
                loadout,
                ..default()
            };
            participant.begin();
            commands.spawn((
                character_bundle(u64::MAX - i as u64, Vec3::from(position)),
                Bot {
                    next_shot: time.elapsed_secs() + 2.0,
                },
                participant,
                ArmorState::from_loadout(loadout),
                Arsenal::new(loadout),
            ));
        }
    }
    if previous_phase != raid.phase {
        info!(
            "Raid {}: {:?} -> {:?}",
            raid.raid_id, previous_phase, raid.phase
        );
    }
}

pub(super) fn objectives_tick(
    time: Res<Time>,
    mut raid: ResMut<RaidState>,
    mut players: Query<
        (
            &Client,
            &ClientInput,
            &Transform,
            &mut BodyHealth,
            &mut Participant,
            &mut MovementState,
        ),
        Without<Bot>,
    >,
) {
    if raid.phase != RaidPhase::Active {
        return;
    }
    let expired = raid.advance_active(time.delta_secs());
    let mut active_count = 0;
    for (client, input, transform, mut body, mut player, mut movement) in &mut players {
        if player.status != RaidStatus::Active {
            continue;
        }
        if !body.alive() || transform.translation.y < -5.0 {
            body.parts[1] = 0.0;
            player.fail(RaidStatus::Dead);
            info!("Player {} died", client.id);
        } else if expired {
            player.fail(RaidStatus::Missing);
            info!("Player {} missing at raid timeout", client.id);
        } else {
            if player.try_loot(&mut raid, transform.translation, input.interact) {
                info!(
                    "Player {} collected loot; carried={}",
                    client.id, player.carried
                );
            }
            if player.tick_extraction(
                &raid,
                transform.translation,
                input.interact,
                time.delta_secs(),
            ) {
                info!("Player {} extracted; secured={}", client.id, player.secured);
            }
        }
        if player.status == RaidStatus::Active {
            active_count += 1;
        } else {
            movement.velocity = Vec3::ZERO;
        }
    }
    raid.finish_if_complete(active_count);
    if raid.phase == RaidPhase::Debrief {
        info!("Raid {} debrief", raid.raid_id);
    }
}
