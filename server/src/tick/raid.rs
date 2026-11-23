use bevy::prelude::*;
use common::{combat::*, map::*, raid::*, *};

use super::{Bot, Projectile, WorldState, arena, bots, set_crouched_state};
use bevy_rapier3d::prelude::*;

#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct RaidResetEntities<'w, 's> {
    bots: Query<'w, 's, Entity, With<Bot>>,
    projectiles: Query<'w, 's, Entity, With<Projectile>>,
    pieces: Query<'w, 's, Entity, With<super::physical::PhysicsPiece>>,
    rubble: Query<'w, 's, Entity, With<super::destruction::Rubble>>,
    poses: Query<'w, 's, (Entity, &'static mut common::humanoid::PoseState)>,
    voxels: Query<'w, 's, (Entity, &'static mut super::BarrierVoxel)>,
}

pub(super) fn lobby_tick(
    mut commands: Commands,
    time: Res<Time>,
    mut raid: ResMut<RaidState>,
    mut world_state: ResMut<WorldState>,
    mut players: Query<
        (
            &Client,
            &mut ClientInput,
            &mut Participant,
            &mut BodyHealth,
            &mut ArmorState,
            &mut Arsenal,
            &mut MovementState,
            &mut Collider,
            &mut Transform,
            &mut arena::Life,
        ),
        Without<Bot>,
    >,
    reset_entities: RaidResetEntities,
    fixture: Res<crate::kit_fixture::KitFixture>,
) {
    let RaidResetEntities {
        bots,
        projectiles,
        pieces,
        rubble,
        mut poses,
        mut voxels,
    } = reset_entities;
    let previous_phase = raid.phase;
    let total = players.iter().count();
    if raid.phase == RaidPhase::Lobby && total != 1 && raid.mode == MatchMode::ArenaSolo {
        raid.mode = MatchMode::Raid;
        for (_, _, mut player, ..) in &mut players {
            player.ready = false;
        }
    }
    if raid.phase == RaidPhase::Lobby && !players.iter().any(|(_, _, p, ..)| p.ready) {
        // An idle preference must not block someone else's explicit ready
        // request. Break simultaneous requests by stable connection ID.
        raid.mode = players
            .iter()
            .filter(|(_, input, ..)| total == 1 || input.mode != MatchMode::ArenaSolo)
            .min_by_key(|(client, input, p, ..)| {
                (
                    input.arena_leave || !new_lobby_command(input, p),
                    input.mode == MatchMode::Raid,
                    client.id,
                )
            })
            .map_or(MatchMode::Raid, |(_, input, ..)| input.mode);
    }
    let mut ready = players.iter().filter(|(_, _, p, ..)| p.ready).count();
    let mut return_requested = false;
    for (_, input, mut player, _, _, mut arsenal, ..) in &mut players {
        if raid.phase == RaidPhase::Lobby
            && !player.ready
            && let Some(loadout) = input.loadout.validated()
        {
            if loadout.primary != arsenal.loadout.primary {
                let replacement = fixture.arsenal(loadout);
                let mut kits = arsenal.kits().to_vec();
                kits.iter_mut()
                    .find(|(slot, _)| *slot == EquipmentSlot::Primary)
                    .unwrap()
                    .1 = replacement
                    .kits()
                    .iter()
                    .find(|(slot, _)| *slot == EquipmentSlot::Primary)
                    .unwrap()
                    .1
                    .clone();
                *arsenal = Arsenal::from_kits(common::catalog::catalog(), loadout, &kits)
                    .expect("confirmed sidearm and validated primary kit");
            }
            arsenal.loadout = loadout;
            player.loadout = loadout;
        }
        if !new_lobby_command(&input, &player) {
            continue;
        }
        player.last_command = input.lobby_sequence;
        match raid.phase {
            RaidPhase::Lobby | RaidPhase::Countdown if !input.arena_leave => {
                if player.ready {
                    player.ready = false;
                    ready -= 1;
                } else if !(raid.mode == MatchMode::ArenaPvp && raid.phase == RaidPhase::Countdown)
                    && raid.mode == input.mode
                    && raid.mode.arena_players().is_none_or(|max| ready < max)
                {
                    player.ready = true;
                    ready += 1;
                }
            }
            RaidPhase::Debrief => return_requested = true,
            RaidPhase::Active
                if raid.mode.is_arena()
                    && input.arena_leave
                    && player.status != RaidStatus::Waiting =>
            {
                return_requested = true;
            }
            _ => (),
        }
    }

    if raid.phase == RaidPhase::Active
        && let Some(expected) = raid.mode.arena_players()
    {
        let roster = players
            .iter()
            .filter(|(_, _, p, ..)| p.status != RaidStatus::Waiting)
            .count();
        return_requested |= roster != expected;
    }
    if players.is_empty() || return_requested {
        raid.reset_lobby();
        if players.is_empty() {
            raid.mode = MatchMode::Raid;
        }
        for (_, input, mut player, mut body, _, mut arsenal, _, _, _, mut life) in &mut players {
            player.return_to_lobby();
            if raid.mode.is_arena() {
                *body = BodyHealth::default();
                *life = default();
            }
            *arsenal = arsenal
                .restocked(common::catalog::catalog(), player.loadout)
                .expect("confirmed raid kits");
            arsenal.synchronize_sequences(&input);
        }
    }

    let ready = players.iter().filter(|(_, _, p, ..)| p.ready).count();
    let started = raid.advance_lobby(ready, total, time.delta_secs());
    if started {
        // Sort by stable network id; query iteration order is not a spawn allocation policy.
        let mut ids: Vec<_> = players
            .iter()
            .filter(|(_, _, p, ..)| p.ready)
            .map(|(client, ..)| client.id)
            .collect();
        ids.sort_unstable();
        for (
            client,
            mut input,
            mut player,
            mut body,
            mut armor,
            mut arsenal,
            mut movement,
            mut collider,
            mut transform,
            mut life,
        ) in &mut players
        {
            let Some(index) = ids.iter().position(|id| *id == client.id) else {
                continue;
            };
            player.begin();
            *body = BodyHealth::default();
            *armor = ArmorState::from_loadout(player.loadout);
            *arsenal = arsenal
                .restocked(common::catalog::catalog(), player.loadout)
                .expect("confirmed raid kits");
            arsenal.synchronize_sequences(&input);
            movement.velocity = Vec3::ZERO;
            set_crouched_state(&mut movement, &mut collider, &mut transform, false);
            *collider = Collider::capsule_y(PLAYER_COLLIDER_HALF_HEIGHT, PLAYER_COLLIDER_RADIUS);
            *movement = MovementState::default();
            movement.prone_sequence = input.prone_sequence;
            *life = arena::Life::new(index == 1, world_state.next_projectile_id);
            transform.translation = if raid.mode.is_arena() {
                *input = arena::transition_input(&input);
                life.spawns()[0]
            } else {
                Vec3::from(SPAWN_CENTER)
                    + Vec3::new(
                        (index % 8) as f32 * 1.5 - 0.75 * (total.min(8) - 1) as f32,
                        0.0,
                        (index / 8) as f32 * 1.5,
                    )
            };
            transform.rotation = Quat::IDENTITY;
        }
    }

    if started
        || (raid.phase == RaidPhase::Lobby && previous_phase != RaidPhase::Lobby)
        || total == 0
    {
        for entity in &pieces {
            commands.entity(entity).despawn();
        }
        for entity in &rubble {
            commands.entity(entity).despawn();
        }
        for (entity, mut pose) in &mut poses {
            *pose = default();
            commands.entity(entity).remove::<super::physical::Corpsed>();
            commands.entity(entity).remove::<super::live_body::Body>();
        }
        for entity in &bots {
            commands.entity(entity).despawn();
        }
        for entity in &projectiles {
            commands.entity(entity).despawn();
        }
        let cells = barrier_cells();
        for (entity, mut voxel) in &mut voxels {
            if voxel.health != voxel.max_health || !world_state.cover_cuts.is_empty() {
                *voxel = super::BarrierVoxel::new(&cells[voxel.id as usize]);
                commands
                    .entity(entity)
                    .insert(voxel.shape.clone())
                    .remove::<ColliderDisabled>();
            }
        }
        world_state.destroyed_cells.clear();
        world_state.cover_cuts.clear();
        world_state.cover_clusters.clear();
        world_state.cover_pending.clear();
        world_state.cover_fines = 0.0;
        world_state.cover_dirty = true;
        world_state.support_dirty = false;
        world_state.support_structures.clear();
        world_state.deferred_structures.clear();
        world_state.impact_marks.clear();
        world_state.impact_cells.clear();
        world_state.fired_projectiles.clear();
    }
    if started && raid.mode == MatchMode::ArenaSolo {
        commands
            .spawn(bots::bundle(u64::MAX, arena::BOT_SPAWNS[0], &time))
            .insert(arena::Life::new(true, world_state.next_projectile_id));
    } else if started && raid.mode == MatchMode::Raid && !pov_fixture() {
        for (i, position) in BOT_POSITIONS.into_iter().enumerate() {
            commands.spawn(bots::bundle(
                u64::MAX - i as u64,
                Vec3::from(position),
                &time,
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

fn new_lobby_command(input: &ClientInput, participant: &Participant) -> bool {
    let advance = input.lobby_sequence.wrapping_sub(participant.last_command);
    advance != 0 && advance < (1 << 31)
}

pub(super) fn objectives_tick(
    time: Res<Time>,
    mut raid: ResMut<RaidState>,
    world: Res<super::WorldState>,
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
    if raid.phase != RaidPhase::Active || raid.mode != MatchMode::Raid {
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
            if player.try_loot(
                &mut raid,
                transform.translation,
                input.interact
                    && input.pickup_focus.is_none()
                    && !in_extraction_zone(transform.translation),
            ) {
                info!(
                    "Player {} collected loot; carried={}",
                    client.id, player.carried
                );
            }
            if player.tick_extraction_with_loot(
                &raid,
                transform.translation,
                input.interact && input.pickup_focus.is_none(),
                time.delta_secs(),
                world.inventory.carried(&raid, client.id),
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
