//! Continuous Arena life cycle; movement, impacts, ragdolls and bot AI
//! still use the same simulation as a raid.
use super::{
    Bot, LastInput, Projectile, WorldState, can_occupy_character, character_bundle,
    physical::PhysicsPiece,
};
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;
use common::{combat::*, raid::*, *};

// Disjoint pools on the map floor, away from the seed crates and barriers.
// The primary positions are already in the existing bot's acquisition range.
pub(super) const PLAYER_SPAWNS: [Vec3; 3] = [
    Vec3::new(-6.0, 1.05, 10.0),
    Vec3::new(-8.0, 1.05, 10.0),
    Vec3::new(-4.0, 1.05, 10.0),
];
pub(super) const BOT_SPAWNS: [Vec3; 3] = [
    Vec3::new(-6.0, 1.05, 1.0),
    Vec3::new(-8.0, 1.05, 1.0),
    Vec3::new(-4.0, 1.05, 1.0),
];

#[derive(Component, Default)]
pub(super) struct Life {
    pub generation: u8,
    pub projectile_floor: u64,
    pub respawn_ticks: u16,
    respawn_at: f64,
    far_seat: bool,
}

impl Life {
    pub fn new(far_seat: bool, projectile_floor: u64) -> Self {
        Self {
            far_seat,
            projectile_floor,
            ..default()
        }
    }

    pub fn spawns(&self) -> [Vec3; 3] {
        if self.far_seat {
            BOT_SPAWNS
        } else {
            PLAYER_SPAWNS
        }
    }
}

pub(super) fn transition_input(input: &ClientInput) -> ClientInput {
    ClientInput {
        lobby_sequence: input.lobby_sequence,
        mode: input.mode,
        arena_leave: input.arena_leave,
        loadout: input.loadout,
        camera: input.camera,
        fire_pressed_sequence: input.fire_pressed_sequence,
        reload_sequence: input.reload_sequence,
        inspect_sequence: input.inspect_sequence,
        selector_sequence: input.selector_sequence,
        prone_sequence: input.prone_sequence,
        ..default()
    }
}

pub(super) fn tick(
    mut commands: Commands,
    clock: (Res<Time>, Res<WorldState>),
    mut raid: ResMut<RaidState>,
    context: ReadRapierContext,
    mut lobby: ResMut<Lobby>,
    mut actors: Query<(
        Entity,
        &Client,
        &Transform,
        &mut Participant,
        &mut BodyHealth,
        &Arsenal,
        &ClientInput,
        &LastInput,
        &mut Life,
        Has<Bot>,
    )>,
    remnants: (Query<(Entity, &PhysicsPiece)>, Query<(Entity, &Projectile)>),
) {
    if raid.phase != RaidPhase::Active || !raid.mode.is_arena() {
        return;
    }
    let Ok(context) = context.single() else {
        return;
    };
    let (time, world) = clock;
    let now = time.elapsed_secs_f64();
    let (pieces, projectiles) = &remnants;
    for (entity, client, root, mut participant, mut body, kit, input, last, mut life, bot) in
        &mut actors
    {
        if participant.status == RaidStatus::Active {
            if body.alive() && root.translation.y >= -5.0 {
                continue;
            }
            body.parts[1] = 0.0;
            participant.fail(RaidStatus::Dead);
            life.respawn_at = now + f64::from(ARENA_RESPAWN_SECONDS);
            for (projectile, _) in projectiles.iter().filter(|(_, p)| p.owner_entity == entity) {
                commands.entity(projectile).despawn();
            }
            info!("Arena actor {} died (life {})", client.id, life.generation);
        }
        if participant.status != RaidStatus::Dead {
            continue;
        }
        let remaining = (life.respawn_at - now).max(0.0) as f32;
        life.respawn_ticks = (remaining * f32::from(SIMULATION_HZ)).ceil() as u16;
        if !bot && raid.mode == MatchMode::ArenaSolo {
            raid.seconds_left = remaining;
        }
        if remaining > 0.0 {
            continue;
        }
        let spawns = life.spawns();
        let Some(spawn) = spawns
            .into_iter()
            .find(|center| can_occupy_character(entity, &context, *center, false))
        else {
            // Wait for an open position. Never spawn into a standing opponent
            // or the map, even when the nominal delay has already elapsed.
            continue;
        };
        for (piece, _) in pieces.iter().filter(|(_, p)| p.owner == client.id) {
            commands.entity(piece).despawn();
        }
        // Replace the body rather than partially clearing physical components.
        // Retain the connection and confirmed kit, not transient actions or wounds.
        let mut participant = participant.clone();
        participant.begin();
        let mut arsenal = kit
            .restocked(common::catalog::catalog(), participant.loadout)
            .expect("confirmed Arena kits");
        let input = transition_input(input);
        let movement = super::MovementState {
            prone_sequence: input.prone_sequence,
            ..default()
        };
        arsenal.synchronize_sequences(&input);
        let generation = life.generation.wrapping_add(1);
        let armor = ArmorState::from_loadout(participant.loadout);
        commands.entity(entity).despawn();
        let mut next = commands.spawn(character_bundle(client.id, spawn));
        next.insert((
            participant,
            armor,
            arsenal,
            input,
            movement,
            LastInput {
                received_at: last.received_at,
            },
            Life {
                generation,
                ..Life::new(life.far_seat, world.next_projectile_id)
            },
        ));
        if bot {
            next.insert(Bot {
                next_shot: time.elapsed_secs() + 2.0,
            });
        } else {
            lobby.players.insert(client.id, next.id());
        }
        info!("Arena actor {} respawned (life {generation})", client.id);
    }
}
