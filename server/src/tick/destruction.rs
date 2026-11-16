//! Authoritative sparse cover damage and its independent bounded snapshot.
use super::*;
use common::map::destruction::{self as shared, CoverSnapshot};

#[derive(Resource)]
pub(super) struct Structure {
    cells: Vec<common::map::BarrierCell>,
    neighbors: Vec<Vec<usize>>,
}

impl Default for Structure {
    fn default() -> Self {
        let cells = common::map::barrier_cells();
        let mut neighbors = vec![Vec::new(); cells.len()];
        for a in 0..cells.len() {
            for b in a + 1..cells.len() {
                let delta = (Vec3::from(cells[a].center) - Vec3::from(cells[b].center)).abs();
                let extent = Vec3::from(cells[a].half_size) + Vec3::from(cells[b].half_size);
                if (0..3).any(|axis| {
                    (delta[axis] - extent[axis]).abs() < 0.0001
                        && (0..3)
                            .filter(|&other| other != axis)
                            .all(|other| delta[other] < extent[other] - 0.0001)
                }) {
                    neighbors[a].push(b);
                    neighbors[b].push(a);
                }
            }
        }
        Self { cells, neighbors }
    }
}

#[derive(Component)]
pub(super) struct Rubble {
    pub cell: u16,
    pub remaining: f32,
}

pub(super) struct Hit {
    pub entity: Entity,
    pub point: Vec3,
    pub normal: Vec3,
}

pub(super) fn damage(
    commands: &mut Commands,
    state: &mut WorldState,
    voxel: &mut BarrierVoxel,
    hit: Hit,
    projectile: &Projectile,
) {
    let cell = &common::map::barrier_cells()[voxel.id as usize];
    voxel.health = (voxel.health - projectile.damage).max(0.0);
    state.remove_cell_marks(voxel.id);
    if voxel.health > 0.0 && state.cover_cuts.len() < shared::MAX_CUTS {
        let cut = shared::impact_cut(cell, hit.point, hit.normal, projectile.weapon.spec().class);
        let inserted = !state.cover_cuts.contains(&cut);
        if inserted {
            state.cover_cuts.push(cut);
        }
        let solids = shared::solids(cell, &state.cover_cuts);
        if solids.len() <= shared::MAX_SOLIDS && !solids.is_empty() {
            voxel.shape = shared::collider(&solids);
            commands.entity(hit.entity).insert(voxel.shape.clone());
        } else {
            if inserted {
                state.cover_cuts.pop();
            }
            voxel.health = 0.0;
        }
    } else {
        voxel.health = 0.0;
    }
    if voxel.health == 0.0 {
        state.support_dirty = true;
        if !state.destroyed_cells.contains(&voxel.id) {
            state.destroyed_cells.push(voxel.id);
        }
        commands.entity(hit.entity).insert(ColliderDisabled);
    }
    state.cover_dirty = true;
}

pub(super) fn release(
    mut commands: Commands,
    mut state: ResMut<WorldState>,
    structure: Res<Structure>,
    mut voxels: Query<(Entity, &mut BarrierVoxel, &Transform)>,
    rubble: Query<(Entity, &Rubble)>,
) {
    if !state.support_dirty {
        return;
    }
    state.support_dirty = false;
    let mut supported = vec![false; structure.cells.len()];
    let mut queue = std::collections::VecDeque::new();
    for cell in &structure.cells {
        if cell.center[1] - cell.half_size[1] <= 0.1001 && !state.destroyed_cells.contains(&cell.id)
        {
            supported[cell.id as usize] = true;
            queue.push_back(cell.id as usize);
        }
    }
    while let Some(cell) = queue.pop_front() {
        for &neighbor in &structure.neighbors[cell] {
            if !supported[neighbor] && !state.destroyed_cells.contains(&(neighbor as u16)) {
                supported[neighbor] = true;
                queue.push_back(neighbor);
            }
        }
    }
    let mut active: Vec<_> = rubble
        .iter()
        .map(|(entity, piece)| (entity, piece.remaining))
        .collect();
    active.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut active: std::collections::VecDeque<_> =
        active.into_iter().map(|(entity, _)| entity).collect();
    for (entity, mut voxel, transform) in &mut voxels {
        if voxel.released || (voxel.health > 0.0 && supported[voxel.id as usize]) {
            continue;
        }
        let fractured = voxel.health <= 0.0;
        voxel.health = 0.0;
        voxel.released = true;
        if !state.destroyed_cells.contains(&voxel.id) {
            state.destroyed_cells.push(voxel.id);
        }
        state.remove_cell_marks(voxel.id);
        commands.entity(entity).insert(ColliderDisabled);
        if active.len() == shared::MAX_RUBBLE
            && let Some(oldest) = active.pop_front()
        {
            commands.entity(oldest).despawn();
        }
        let cell = &structure.cells[voxel.id as usize];
        let mut physical_transform = *transform;
        if fractured {
            physical_transform.scale *= 0.45;
        }
        let piece = commands
            .spawn((
                Rubble {
                    cell: voxel.id,
                    remaining: shared::RUBBLE_LIFETIME,
                },
                cell.surface,
                RigidBody::Dynamic,
                voxel.shape.clone(),
                physical_transform,
                Velocity {
                    linear: if fractured {
                        Vec3::new(
                            if voxel.id.is_multiple_of(2) {
                                0.4
                            } else {
                                -0.4
                            },
                            0.1,
                            0.6,
                        )
                    } else {
                        Vec3::new(0.03, 0.0, 0.08)
                    },
                    angular: Vec3::new(0.25, 0.1, 0.2),
                },
                Damping {
                    linear_damping: 0.4,
                    angular_damping: 0.8,
                },
                Friction::coefficient(0.85),
                Restitution::coefficient(0.05),
                CollisionGroups::new(Group::GROUP_4, Group::GROUP_1 | Group::GROUP_4),
                ColliderMassProperties::Density(if matches!(cell.surface, SurfaceKind::Wood) {
                    500.0
                } else {
                    1800.0
                }),
                Sleeping::default(),
                Ccd::enabled(),
                AdditionalSolverIterations(4),
            ))
            .id();
        active.push_back(piece);
    }
    state.cover_dirty = true;
}

pub(super) fn expire(
    mut commands: Commands,
    time: Res<Time>,
    mut state: ResMut<WorldState>,
    mut rubble: Query<(Entity, &mut Rubble, &Sleeping)>,
) {
    for (entity, mut piece, sleeping) in &mut rubble {
        piece.remaining -= time.delta_secs();
        if piece.remaining <= 0.0 {
            commands.entity(entity).despawn();
            state.cover_dirty = true;
        } else if !sleeping.sleeping {
            state.cover_dirty = true;
        }
    }
}

pub(super) fn broadcast(
    time: Res<Time>,
    raid: Res<RaidState>,
    mut state: ResMut<WorldState>,
    mut server: ResMut<RenetServer>,
    rubble: Query<(&Rubble, &Transform)>,
) {
    state.cover_elapsed += time.delta_secs();
    state.cover_refresh += time.delta_secs();
    if state.cover_elapsed < 1.0 / 30.0 || (!state.cover_dirty && state.cover_refresh < 0.5) {
        return;
    }
    state.cover_elapsed %= 1.0 / 30.0;
    state.cover_refresh = 0.0;
    state.cover_dirty = false;
    state.cover_sequence += 1;
    let snapshot = CoverSnapshot {
        sequence: state.cover_sequence,
        raid_id: raid.raid_id,
        phase: raid.phase,
        removed: common::map::destroyed_mask(&state.destroyed_cells),
        cuts: state.cover_cuts.clone(),
        rubble: rubble
            .iter()
            .map(|(piece, transform)| shared::RubblePose {
                cell: piece.cell,
                position: transform.translation.into(),
                rotation: transform.rotation.to_array(),
                scale: transform.scale.x,
            })
            .collect(),
    };
    server.broadcast_message(shared::CHANNEL, data::encode(&snapshot));
}
