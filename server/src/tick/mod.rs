// Bevy system query tuples describe ECS access directly; aliases obscure the filters.
#![allow(clippy::type_complexity)]
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;
use bevy_renet2::prelude::{DefaultChannel, RenetServer, ServerEvent};
use common::{combat::*, raid::*, *};

mod bots;
mod destruction;
mod live_body;
mod locomotion;
mod metrics;
mod physical;
mod ragdoll;
mod raid;
mod replication;
mod simulation;
use bots::{Bot, bot_tick};
use raid::{lobby_tick, objectives_tick};
use simulation::{physx_tick, projectiles_tick, sync_ground_state, weapons_tick};

pub struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_hz(128.0))
            .init_resource::<WorldState>()
            .init_resource::<destruction::Structure>()
            .init_resource::<RaidState>()
            .init_resource::<crate::kit_fixture::KitFixture>()
            .init_resource::<replication::Replication>()
            .init_resource::<metrics::TickMetrics>()
            .add_systems(FixedFirst, metrics::begin)
            .add_systems(FixedLast, metrics::end)
            .add_systems(Startup, spawn_world_colliders)
            .add_systems(
                FixedUpdate,
                (
                    recv_connectivity,
                    recv_players_input,
                    expire_input,
                    lobby_tick,
                    replication::commands,
                    bot_tick,
                    simulation::advance_recoil,
                    physical::physical_tick,
                    live_body::begin_falls,
                    live_body::advance,
                    physx_tick,
                    simulation::resolve_eye_lean,
                    weapons_tick,
                    sync_actor_colliders,
                )
                    .chain()
                    .before(PhysicsSet::SyncBackend),
            )
            .add_systems(
                FixedUpdate,
                (
                    live_body::readback,
                    locomotion::advance,
                    ragdoll::capture,
                    projectiles_tick,
                    destruction::release,
                    physical::spawn_dead,
                    objectives_tick,
                    sync_ground_state,
                    physical::expire_pieces,
                    destruction::expire,
                    destruction::broadcast,
                    send_world_snapshot,
                )
                    .chain()
                    .after(PhysicsSet::Writeback),
            );
    }
}

#[derive(Debug, Default, Resource)]
struct WorldState {
    destroyed_cells: Vec<u16>,
    impact_cells: std::collections::BTreeMap<u32, u16>,
    cover_cuts: Vec<common::map::destruction::Cut>,
    cover_elapsed: f32,
    cover_refresh: f32,
    cover_sequence: u64,
    cover_dirty: bool,
    support_dirty: bool,
    next_projectile_id: u64,
    // Only 256 marks are retained, so wrapping 32-bit IDs stay unique in a snapshot.
    next_mark_id: u32,
    impact_marks: Vec<ImpactMarkData>,
    fired_projectiles: Vec<FiredProjectileData>,
    snapshot_elapsed: f32,
    snapshot_sequence: u64,
    shot_overflow_active: bool,
    next_piece_id: u64,
}

impl WorldState {
    fn impact(&mut self, position: Vec3, normal: Vec3, size_mm: u8) {
        if self.impact_marks.len() == 256 {
            let oldest = self.impact_marks.remove(0);
            self.impact_cells.remove(&oldest.id);
        }
        self.impact_marks.push(ImpactMarkData {
            id: self.next_mark_id,
            pos: position.into(),
            normal: normal.into(),
            size_mm,
        });
        self.next_mark_id = self.next_mark_id.wrapping_add(1);
    }

    fn impact_on_cell(&mut self, position: Vec3, normal: Vec3, size_mm: u8, cell: Option<u16>) {
        let id = self.next_mark_id;
        self.impact(position, normal, size_mm);
        if let Some(cell) = cell {
            self.impact_cells.insert(id, cell);
        }
    }

    fn remove_cell_marks(&mut self, cell: u16) {
        self.impact_marks
            .retain(|mark| self.impact_cells.get(&mark.id) != Some(&cell));
        self.impact_cells.retain(|_, anchor| *anchor != cell);
    }
}

#[derive(Debug, Default, Component)]
struct LastInput {
    received_at: f32,
}

#[derive(Debug, Component)]
struct Projectile {
    id: u64,
    weapon: WeaponId,
    velocity: Vec3,
    damage: f32,
    penetration: f32,
    lifetime: f32,
    owner_entity: Entity,
    penetrated: Vec<Entity>,
}

/// One first-tick segment, resolved against the same anatomy/surfaces as flight.
/// Server-only: no protocol growth or client-provided collision decisions.
#[derive(Component)]
struct MuzzleClearance(Vec3);

#[derive(Component)]
struct BarrierVoxel {
    id: u16,
    health: f32,
    max_health: f32,
    shape: Collider,
    released: bool,
}

impl BarrierVoxel {
    fn new(cell: &common::map::BarrierCell) -> Self {
        Self {
            id: cell.id,
            health: cell.health(),
            max_health: cell.health(),
            shape: Collider::cuboid(cell.half_size[0], cell.half_size[1], cell.half_size[2]),
            released: false,
        }
    }
}

fn spawn_world_colliders(mut commands: Commands) {
    for cell in common::map::barrier_cells() {
        commands.spawn((
            BarrierVoxel::new(&cell),
            cell.surface,
            Collider::cuboid(cell.half_size[0], cell.half_size[1], cell.half_size[2]),
            Transform::from_translation(Vec3::from(cell.center)),
        ));
    }
    for (index, block) in common::map::BLOCKS.iter().enumerate() {
        if common::map::destructible(index) {
            continue;
        }
        commands.spawn((
            block.surface,
            Collider::cuboid(block.half_size[0], block.half_size[1], block.half_size[2]),
            Transform::from_translation(Vec3::from(block.center)),
        ));
    }
    for position in common::map::LOOT_POSITIONS {
        commands.spawn((
            SurfaceKind::Wood,
            Collider::cuboid(0.55, 0.45, 0.55),
            Transform::from_translation(Vec3::from(position)),
        ));
    }
    if common::map::pov_fixture() {
        let step = common::map::POV_STEP;
        commands.spawn((
            step.surface,
            Collider::cuboid(step.half_size[0], step.half_size[1], step.half_size[2]),
            Transform::from_translation(Vec3::from(step.center)),
        ));
    }
}

fn character_bundle(id: u64, position: Vec3) -> impl Bundle {
    (
        Client { id },
        ClientInput::default(),
        BodyHealth::default(),
        LastInput::default(),
        Collider::capsule_y(PLAYER_COLLIDER_HALF_HEIGHT, PLAYER_COLLIDER_RADIUS),
        CollisionGroups::new(Group::GROUP_3, Group::GROUP_1 | Group::GROUP_3),
        KinematicCharacterController {
            // KCC shape queries do not inherit the collider's groups. Debris
            // must not become support/autosteps that launch the living actor
            // immediately before its momentum is transferred to the ragdoll.
            filter_groups: Some(CollisionGroups::new(
                Group::GROUP_3,
                Group::GROUP_1 | Group::GROUP_3,
            )),
            autostep: Some(CharacterAutostep {
                max_height: CharacterLength::Absolute(PLAYER_STEP_HEIGHT),
                min_width: CharacterLength::Absolute(PLAYER_COLLIDER_RADIUS * 2.0),
                include_dynamic_bodies: false,
            }),
            snap_to_ground: Some(CharacterLength::Absolute(0.2)),
            ..default()
        },
        MovementState::default(),
        common::humanoid::PoseState::default(),
        locomotion::Gait::default(),
        ragdoll::Motion::default(),
        ragdoll::Impact::default(),
        Transform::from_translation(position),
    )
}

fn validated_input(bytes: &[u8]) -> Option<ClientInput> {
    if bytes.len() > 2048 {
        return None;
    }
    let mut input: ClientInput = data::try_decode(bytes).ok()?;
    if !input.camera.yaw.is_finite()
        || !input.camera.pitch.is_finite()
        || !input.camera.roll.is_finite()
    {
        return None;
    }
    input.camera.pitch = input.camera.pitch.clamp(-1.56, 1.56);
    input.camera.roll = input.camera.roll.clamp(-0.3, 0.3);
    input.camera.yaw %= std::f32::consts::TAU;
    input.loadout = input.loadout.validated()?;
    Some(input)
}

fn recv_players_input(
    mut server: ResMut<RenetServer>,
    lobby: Res<Lobby>,
    time: Res<Time>,
    mut players: Query<(&mut ClientInput, &mut LastInput)>,
) {
    for client_id in server.clients_id() {
        // Bound work per client per tick; the transport also bounds its receive queues.
        for _ in 0..64 {
            let Some(message) = server.receive_message(client_id, DefaultChannel::ReliableOrdered)
            else {
                break;
            };
            let Some(input) = validated_input(&message) else {
                continue;
            };
            if let Some(&entity) = lobby.players.get(&client_id)
                && let Ok((mut current, mut last)) = players.get_mut(entity)
            {
                *current = input;
                last.received_at = time.elapsed_secs();
            }
        }
    }
}

fn expire_input(
    time: Res<Time>,
    mut players: Query<(&mut ClientInput, &LastInput, &mut Arsenal), Without<Bot>>,
) {
    for (mut input, last, mut arsenal) in &mut players {
        if time.elapsed_secs() - last.received_at > 0.5 {
            input.forward = false;
            input.backward = false;
            input.left = false;
            input.right = false;
            input.run = false;
            input.jump = false;
            input.fire = false;
            input.aim = false;
            input.interact = false;
            arsenal.cancel_actions();
            arsenal.synchronize_sequences(&input);
        }
    }
}

fn sync_actor_colliders(
    mut commands: Commands,
    raid: Res<RaidState>,
    actors: Query<(Entity, &Participant, &BodyHealth, Option<&ColliderDisabled>)>,
) {
    for (entity, p, body, disabled) in &actors {
        let enabled =
            raid.phase == RaidPhase::Active && p.status == RaidStatus::Active && body.alive();
        if enabled && disabled.is_some() {
            commands.entity(entity).remove::<ColliderDisabled>();
        } else if !enabled && disabled.is_none() {
            commands.entity(entity).insert(ColliderDisabled);
        }
    }
}

fn recv_connectivity(
    mut events: MessageReader<ServerEvent>,
    mut commands: Commands,
    mut lobby: ResMut<Lobby>,
    mut server: ResMut<RenetServer>,
    time: Res<Time>,
    mut replication: ResMut<replication::Replication>,
    fixture: Res<crate::kit_fixture::KitFixture>,
) {
    for event in events.read() {
        match event {
            ServerEvent::ClientConnected { client_id } => {
                replication.forget(*client_id);
                if *client_id >= (1_u64 << 63) {
                    server.disconnect(*client_id);
                    continue;
                }
                info!("Player {client_id} connected");
                let entity = commands
                    .spawn((
                        character_bundle(*client_id, Vec3::from(common::map::SPAWN_CENTER)),
                        Participant::default(),
                        ArmorState::default(),
                        fixture.arsenal(Loadout::default()),
                        ColliderDisabled,
                    ))
                    .id();
                commands.entity(entity).insert(LastInput {
                    received_at: time.elapsed_secs(),
                });
                for &id in lobby.players.keys() {
                    server.send_message(
                        *client_id,
                        DefaultChannel::ReliableOrdered,
                        data::encode(&ServerMessage::ClientConnected { id }),
                    );
                }
                lobby.players.insert(*client_id, entity);
                server.broadcast_message(
                    DefaultChannel::ReliableOrdered,
                    data::encode(&ServerMessage::ClientConnected { id: *client_id }),
                );
            }
            ServerEvent::ClientDisconnected { client_id, reason } => {
                replication.forget(*client_id);
                info!("Player {client_id} disconnected: {reason}");
                if let Some(entity) = lobby.players.remove(client_id) {
                    commands.entity(entity).despawn();
                }
                server.broadcast_message(
                    DefaultChannel::ReliableOrdered,
                    data::encode(&ServerMessage::ClientDisconnected { id: *client_id }),
                );
            }
        }
    }
}

fn send_world_snapshot(
    time: Res<Time>,
    mut server: ResMut<RenetServer>,
    raid: Res<RaidState>,
    mut state: ResMut<WorldState>,
    mut replication: ResMut<replication::Replication>,
    scene: (
        Query<(
            &Transform,
            &Client,
            &MovementState,
            &BodyHealth,
            &ArmorState,
            &Arsenal,
            &Participant,
            Has<Bot>,
            &common::humanoid::PoseState,
        )>,
        Query<(&Projectile, &Transform)>,
        Query<(&physical::PhysicsPiece, &Transform)>,
    ),
) {
    let (players, projectiles, pieces) = scene;
    state.snapshot_elapsed += time.delta_secs();
    if state.snapshot_elapsed < SNAPSHOT_INTERVAL {
        return;
    }
    state.snapshot_elapsed %= SNAPSHOT_INTERVAL;
    state.snapshot_sequence += 1;
    if let Err(error) = replication.synchronize_raid(&raid) {
        error!("Cannot publish configuration epoch: {error:?}");
        server.disconnect_all();
        return;
    }
    // Register every owned slot for late join and subsequent equip. At capacity,
    // reset once then rebuild the complete current set under the new epoch.
    for attempt in 0..2 {
        let result = players
            .iter()
            .try_for_each(|(_, client, _, _, _, arsenal, _, _, _)| {
                replication.register(client.id, arsenal)
            });
        match result {
            Ok(()) => break,
            Err(common::replication::ConfigurationError::Capacity) if attempt == 0 => {
                if replication.reset().is_err() {
                    error!("Configuration epoch exhausted");
                    server.disconnect_all();
                    return;
                }
            }
            Err(error) => {
                error!("Cannot publish weapon configuration: {error:?}");
                server.disconnect_all();
                return;
            }
        }
    }
    replication.publish(&mut server);
    let players = players
        .iter()
        .map(
            |(t, client, movement, body, armor, arsenal, participant, bot, pose)| ClientData {
                id: client.id,
                pos: t.translation.into(),
                rot: t.rotation.into(),
                crouched: movement.crouched,
                alive: body.alive()
                    && participant.status == RaidStatus::Active
                    && raid.phase == RaidPhase::Active,
                health: body.total(),
                weapon: arsenal.current().spec().class,
                weapon_id: arsenal.current().spec().definition,
                configuration: replication.handle(client.id, arsenal),
                weapon_instance: arsenal.current().instance().id,
                weapon_slot: arsenal.active,
                ammo_in_mag: arsenal.current().rounds(),
                reserve_ammo: arsenal.current().reserve(),
                mechanics: arsenal.current().mechanics().snapshot(),
                recoil: common::recoil::PackedRecoil::pack(arsenal.recoil),
                body: body.clone(),
                armor: armor.clone(),
                participant: participant.clone(),
                bot,
                pose: pose.clone(),
            },
        )
        .collect();
    let projectiles = projectiles
        .iter()
        .map(|(p, t)| ProjectileData {
            id: p.id,
            pos: t.translation.into(),
            vel: p.velocity.into(),
        })
        .collect();
    state
        .fired_projectiles
        .retain(|shot| time.elapsed_secs() - shot.time < 0.5);
    // Queue every new event reliably before fitting the redundant snapshot copy.
    replication.publish_shots(
        &mut server,
        raid.raid_id,
        time.elapsed_secs(),
        state.next_projectile_id,
        &state.fired_projectiles,
    );
    let snapshot = WorldSnapshot {
        destroyed_cells: common::map::destroyed_mask(&state.destroyed_cells),
        sequence: state.snapshot_sequence,
        configuration_epoch: replication.epoch(),
        raid: raid.clone(),
        players,
        projectiles,
        impact_marks: state.impact_marks.clone(),
        fired_projectiles: state.fired_projectiles.clone(),
        physics_pieces: pieces
            .iter()
            .map(|(piece, t)| PhysicsPieceData {
                id: piece.id,
                owner: piece.owner,
                segment: piece.segment,
                pose: common::humanoid::SegmentPose {
                    position: t.translation.into(),
                    rotation: t.rotation.to_array(),
                },
                detached: piece.detached,
                bot: piece.bot,
            })
            .collect(),
        simulation_time: time.elapsed_secs(),
    };
    let actors = snapshot.players.len();
    let pieces = snapshot.physics_pieces.len();
    let packed = match common::replication::shots::pack_snapshot(snapshot) {
        Ok(packed) => packed,
        Err(error) => {
            error!(
                "SNAPSHOT OVERFLOW mandatory_bytes={} actors={actors} pieces={pieces}; disconnecting instead of truncating state",
                error.mandatory_bytes
            );
            server.disconnect_all();
            return;
        }
    };
    if !packed.overflow.is_empty() && !state.shot_overflow_active {
        warn!(
            "SHOT OVERFLOW reliable_only={} snapshot_bytes={}",
            packed.overflow.len(),
            packed.bytes.len()
        );
    }
    state.shot_overflow_active = !packed.overflow.is_empty();
    if state.snapshot_sequence % 150 == 0 {
        info!(
            "TK PERF snapshot_bytes={} actors={} pieces={} reliable_only_shots={}",
            packed.bytes.len(),
            actors,
            pieces,
            packed.overflow.len()
        );
    }
    server.broadcast_message(DefaultChannel::Unreliable, packed.bytes);
}

fn set_crouched_state(
    movement: &mut MovementState,
    collider: &mut Collider,
    transform: &mut Transform,
    crouched: bool,
) {
    if movement.crouched == crouched {
        return;
    }
    movement.crouched = crouched;
    *collider = Collider::capsule_y(
        if crouched {
            PLAYER_CROUCH_COLLIDER_HALF_HEIGHT
        } else {
            PLAYER_COLLIDER_HALF_HEIGHT
        },
        PLAYER_COLLIDER_RADIUS,
    );
    let delta = PLAYER_COLLIDER_HALF_HEIGHT - PLAYER_CROUCH_COLLIDER_HALF_HEIGHT;
    transform.translation.y += if crouched { -delta } else { delta };
}

fn can_stand_up(entity: Entity, context: &RapierContext<'_>, translation: Vec3) -> bool {
    can_occupy_character(
        entity,
        context,
        translation + Vec3::Y * (PLAYER_COLLIDER_HALF_HEIGHT - PLAYER_CROUCH_COLLIDER_HALF_HEIGHT),
        false,
    )
}

fn can_occupy_character(
    entity: Entity,
    context: &RapierContext<'_>,
    center: Vec3,
    crouched: bool,
) -> bool {
    // The controller's contact skin must not turn a touching floor into blocked headroom.
    // Use overlap, not a zero-length sweep (which can miss initial intersections).
    let shape = Collider::capsule_y(
        if crouched {
            PLAYER_CROUCH_COLLIDER_HALF_HEIGHT
        } else {
            PLAYER_COLLIDER_HALF_HEIGHT
        },
        PLAYER_COLLIDER_RADIUS - 0.015,
    );
    let mut clear = true;
    context.intersect_shape(
        center,
        Quat::IDENTITY,
        &shape,
        QueryFilter::new()
            .groups(CollisionGroups::new(
                Group::ALL,
                Group::GROUP_1 | Group::GROUP_3,
            ))
            .exclude_collider(entity)
            .exclude_sensors(),
        |_, _| {
            clear = false;
            false
        },
    );
    clear
}

#[cfg(test)]
mod tests;
