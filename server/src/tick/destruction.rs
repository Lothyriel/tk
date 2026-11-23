//! Authoritative sparse cover damage and its independent bounded snapshot.
use super::*;
use common::map::destruction::{self as shared, CoverSnapshot};

#[derive(Resource)]
pub(super) struct Structure {
    cells: Vec<common::map::BarrierCell>,
}
impl Default for Structure {
    fn default() -> Self {
        Self {
            cells: common::map::barrier_cells(),
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct DamageCluster {
    structure: u16,
    center: Vec3,
    normal: Vec3,
    dose: f32,
    direction: Vec3,
    impulse: f32,
}

#[derive(Component)]
pub(super) struct Rubble {
    pub id: u32,
    pub cell: u16,
    pub boxes: Vec<shared::FragmentBox>,
    pub remaining: f32,
}

pub(super) struct Hit {
    pub entity: Entity,
    pub point: Vec3,
    pub normal: Vec3,
    pub direction: Vec3,
}

pub(super) type Voxels<'w, 's> =
    Query<'w, 's, (Entity, &'static mut BarrierVoxel, &'static Transform), Without<Projectile>>;

fn contains(a: &shared::Cut, b: &shared::Cut) -> bool {
    a.cell == b.cell
        && ((Vec3::from(a.center) - Vec3::from(b.center)).abs() + Vec3::from(b.half_size))
            .cmple(Vec3::from(a.half_size) + Vec3::splat(1e-7))
            .all()
}

/// Prepare all crossed cells atomically. Detail pressure rejects the additional
/// cut conservatively; it can never act as a physical destruction threshold.
pub(super) fn add_cuts(
    existing: &[shared::Cut],
    cells: &[common::map::BarrierCell],
    structure: u16,
    cutters: &[shared::SolidBox],
    removed: &[u16],
) -> Option<Vec<shared::Cut>> {
    let mut next: Vec<_> = existing
        .iter()
        .filter(|cut| !removed.contains(&cut.cell))
        .cloned()
        .collect();
    for cell in cells
        .iter()
        .filter(|cell| cell.structure == structure && !removed.contains(&cell.id))
    {
        let bounds = shared::SolidBox {
            center: cell.center.into(),
            half_size: cell.half_size.into(),
        };
        for cutter in cutters {
            let Some(overlap) = bounds.intersection(*cutter) else {
                continue;
            };
            let cut = shared::Cut {
                cell: cell.id,
                center: (overlap.center - bounds.center).into(),
                half_size: overlap.half_size.into(),
            };
            if next.iter().any(|old| contains(old, &cut)) {
                continue;
            }
            next.retain(|old| !contains(&cut, old));
            next.push(cut);
            if next.len() > shared::MAX_CUTS {
                return None;
            }
        }
        shared::bounded_solids(cell, &next)?;
    }
    let mut total = 0;
    for cell in cells {
        total += shared::bounded_solids(cell, &next)?.len();
        if total > shared::MAX_STATIC_SOLIDS {
            return None;
        }
    }
    Some(next)
}

#[derive(Debug)]
pub(super) struct PendingFragment {
    cell: u16,
    boxes: Vec<shared::SolidBox>,
    point: Vec3,
    direction: Vec3,
    impulse: f32,
}

fn spawn_fragment(commands: &mut Commands, state: &mut WorldState, fragment: PendingFragment) {
    let cells = common::map::barrier_cells();
    let surface = cells[fragment.cell as usize].surface;
    let material = shared::material::properties(surface);
    let volume: f32 = fragment.boxes.iter().map(|s| s.volume()).sum();
    let center = fragment
        .boxes
        .iter()
        .map(|s| s.center * s.volume())
        .sum::<Vec3>()
        / volume;
    let boxes: Vec<_> = fragment
        .boxes
        .iter()
        .map(|s| s.translated(-center))
        .collect();
    let mass = (volume * material.density).max(0.001);
    let velocity = fragment.direction * (fragment.impulse / mass).min(3.0);
    let lever = fragment.point - center;
    let angular = lever.cross(fragment.direction * fragment.impulse) / (mass * 0.04).max(0.01);
    state.next_fragment_id = state
        .next_fragment_id
        .checked_add(1)
        .expect("bounded raid fragment IDs");
    commands.spawn((
        Rubble {
            id: state.next_fragment_id,
            cell: fragment.cell,
            boxes: boxes.iter().copied().map(Into::into).collect(),
            remaining: shared::RUBBLE_LIFETIME,
        },
        surface,
        RigidBody::Dynamic,
        shared::collider(&boxes),
        Transform::from_translation(center),
        Velocity {
            linear: velocity,
            angular: angular.clamp_length_max(4.0),
        },
        Damping {
            linear_damping: 0.4,
            angular_damping: 0.8,
        },
        Friction::coefficient(0.85),
        Restitution::coefficient(0.05),
        CollisionGroups::new(Group::GROUP_4, Group::GROUP_1 | Group::GROUP_4),
        ColliderMassProperties::Density(material.density),
        Sleeping::default(),
        Ccd::enabled(),
        AdditionalSolverIterations(4),
    ));
}

pub(super) fn damage(
    commands: &mut Commands,
    state: &mut WorldState,
    voxels: &mut Voxels,
    hit: Hit,
    projectile: &Projectile,
    live_capacity: (usize, usize),
) {
    let Ok((_, voxel, _)) = voxels.get(hit.entity) else {
        return;
    };
    let id = voxel.id;
    let cells = common::map::barrier_cells();
    let cell = &cells[id as usize];
    let material = shared::material::properties(cell.surface);
    let mut clusters = state.cover_clusters.clone();
    let index = clusters.iter().position(|cluster| {
        cluster.structure == cell.structure
            && cluster.center.distance(hit.point) < material.cluster_distance
    });
    let index = if let Some(index) = index {
        index
    } else {
        if clusters.len() == shared::MAX_CUTS {
            return;
        }
        clusters.push(DamageCluster {
            structure: cell.structure,
            center: hit.point,
            normal: hit.normal.try_normalize().unwrap_or(-hit.direction),
            dose: 0.0,
            direction: Vec3::ZERO,
            impulse: 0.0,
        });
        clusters.len() - 1
    };
    clusters[index].dose += projectile.damage;
    clusters[index].direction = hit.direction;
    clusters[index].impulse = (projectile.damage.max(0.0).sqrt() * 0.025).min(1.5);
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    for part in cells.iter().filter(|c| c.structure == cell.structure) {
        lo = lo.min(Vec3::from(part.center) - Vec3::from(part.half_size));
        hi = hi.max(Vec3::from(part.center) + Vec3::from(part.half_size));
    }
    let cutters = shared::material::cutters(
        shared::SolidBox {
            center: (lo + hi) * 0.5,
            half_size: (hi - lo) * 0.5,
        },
        cell.surface,
        shared::material::Impact {
            point: hit.point,
            normal: hit.normal,
            entry: clusters[index].center,
            entry_normal: clusters[index].normal,
            direction: hit.direction,
            diameter: projectile.weapon.spec().bore_diameter_mm * 0.001,
            damage: projectile.damage,
            penetration: projectile.penetration,
            dose: clusters[index].dose,
            seed: projectile.id as u32,
        },
    );
    let Some(mut next) = add_cuts(
        &state.cover_cuts,
        &cells,
        cell.structure,
        &cutters,
        &state.destroyed_cells,
    ) else {
        return;
    };
    if next == state.cover_cuts {
        return;
    }
    let mut chipped = Vec::new();
    if clusters[index].dose > material.degradation {
        for definition in cells
            .iter()
            .filter(|c| c.structure == cell.structure && !state.destroyed_cells.contains(&c.id))
        {
            let mut remaining = shared::solids(definition, &state.cover_cuts);
            for cutter in &cutters {
                let local = cutter.translated(-Vec3::from(definition.center));
                let cut = shared::Cut {
                    cell: definition.id,
                    center: local.center.into(),
                    half_size: local.half_size.into(),
                };
                let mut next = Vec::new();
                for solid in remaining {
                    if let Some(lost) = solid.intersection(local) {
                        chipped.push(shared::support::Node {
                            cell: definition.id,
                            structure: definition.structure,
                            surface: definition.surface,
                            solid: lost.translated(definition.center.into()),
                        });
                    }
                    next.extend(shared::subtract(solid, &cut));
                }
                shared::compact(&mut next);
                remaining = next;
            }
        }
    }
    let mut chipped_boxes: Vec<_> = chipped.iter().map(|node| node.solid).collect();
    shared::compact(&mut chipped_boxes);
    chipped = chipped_boxes
        .into_iter()
        .filter(|solid| shared::material::physical_debris(cell.surface, *solid))
        .map(|solid| shared::support::Node {
            cell: cell.id,
            structure: cell.structure,
            surface: cell.surface,
            solid,
        })
        .collect();
    let chipped_volume: f32 = chipped.iter().map(|n| n.solid.volume()).sum();
    let groups = if chipped_volume > 0.000008 {
        shared::support::fragments(&chipped)
    } else {
        vec![]
    };
    let reserved_boxes: usize = state.cover_pending.iter().map(|f| f.boxes.len()).sum();
    if live_capacity.0 + state.cover_pending.len() + groups.len() > shared::MAX_RUBBLE
        || live_capacity.1 + reserved_boxes + if groups.is_empty() { 0 } else { chipped.len() }
            > shared::MAX_RUBBLE_SOLIDS
    {
        return;
    }
    // Entry-face chips are released into the free side of the impact surface.
    // Driving them along the incoming ray wedges shallow concrete spall into
    // its preserved backstop. Keep a small bounded tangential response; neither
    // the exact initial fragment shape/pose nor support-driven release changes.
    let normal = clusters[index].normal;
    let tangent = hit.direction - normal * hit.direction.dot(normal);
    let ejection = (normal + tangent * 0.15).normalize_or_zero();
    for group in &groups {
        state.cover_pending.push(PendingFragment {
            cell: chipped[group[0]].cell,
            boxes: group.iter().map(|&i| chipped[i].solid).collect(),
            point: hit.point,
            direction: ejection,
            impulse: clusters[index].impulse / group.len() as f32,
        });
    }
    if !groups.is_empty() {
        state.cover_fines -= chipped_volume;
    }
    for (entity, mut voxel, _) in voxels.iter_mut() {
        let definition = &cells[voxel.id as usize];
        if definition.structure != cell.structure || state.destroyed_cells.contains(&voxel.id) {
            continue;
        }
        let before: f32 = shared::solids(definition, &state.cover_cuts)
            .iter()
            .map(|s| s.volume())
            .sum();
        let solids = shared::solids(definition, &next);
        let after: f32 = solids.iter().map(|s| s.volume()).sum();
        if (before - after).abs() < 1e-9 {
            continue;
        }
        state.cover_fines += (before - after).max(0.0);
        state.remove_cell_marks(voxel.id);
        let original = Vec3::from(definition.half_size).element_product() * 8.0;
        voxel.health = voxel.max_health * (after / original).clamp(0.0, 1.0);
        if solids.is_empty() {
            state.destroyed_cells.push(voxel.id);
            voxel.health = 0.0;
            commands.entity(entity).insert(ColliderDisabled);
        } else {
            voxel.shape = shared::collider(&solids);
            commands.entity(entity).insert(voxel.shape.clone());
        }
    }
    next.retain(|cut| !state.destroyed_cells.contains(&cut.cell));
    state.cover_cuts = next;
    state.cover_clusters = clusters;
    state.cover_dirty = true;
    state.support_dirty = true;
    state.support_structures.insert(cell.structure);
}

pub(super) fn release(
    mut commands: Commands,
    mut state: ResMut<WorldState>,
    structure: Res<Structure>,
    mut voxels: Voxels,
    rubble: Query<&Rubble>,
) {
    let pending = std::mem::take(&mut state.cover_pending);
    let pending_bodies = pending.len();
    let pending_boxes: usize = pending.iter().map(|p| p.boxes.len()).sum();
    for fragment in pending {
        spawn_fragment(&mut commands, &mut state, fragment);
    }
    if !state.support_dirty {
        return;
    }
    state.support_dirty = false;
    let affected = std::mem::take(&mut state.support_structures);
    let mut nodes = Vec::new();
    for cell in &structure.cells {
        if state.destroyed_cells.contains(&cell.id)
            || (!affected.is_empty() && !affected.contains(&cell.structure))
        {
            continue;
        }
        for solid in shared::solids(cell, &state.cover_cuts) {
            nodes.push(shared::support::Node {
                cell: cell.id,
                structure: cell.structure,
                surface: cell.surface,
                solid: solid.translated(cell.center.into()),
            });
        }
    }
    let unsupported = shared::support::unsupported(&nodes);
    let mut body_count = rubble.iter().count() + pending_bodies;
    let mut box_count: usize = rubble.iter().map(|r| r.boxes.len()).sum::<usize>() + pending_boxes;
    for structure_id in structure
        .cells
        .iter()
        .map(|c| c.structure)
        .collect::<std::collections::BTreeSet<_>>()
    {
        let selected: Vec<_> = nodes
            .iter()
            .zip(&unsupported)
            .filter(|(node, loose)| **loose && node.structure == structure_id)
            .map(|(node, _)| *node)
            .collect();
        if selected.is_empty() {
            continue;
        }
        let mut next_cuts = state.cover_cuts.clone();
        let mut removed = state.destroyed_cells.clone();
        for cell in structure
            .cells
            .iter()
            .filter(|c| c.structure == structure_id)
        {
            let count = nodes.iter().filter(|n| n.cell == cell.id).count();
            let lost: Vec<_> = selected.iter().filter(|n| n.cell == cell.id).collect();
            if lost.is_empty() {
                continue;
            }
            if lost.len() == count {
                removed.push(cell.id);
            } else {
                for node in lost {
                    next_cuts.push(shared::Cut {
                        cell: cell.id,
                        center: (node.solid.center - Vec3::from(cell.center)).into(),
                        half_size: node.solid.half_size.into(),
                    });
                }
            }
        }
        // Remove old cell/decomposition seams before choosing fracture detail.
        // Eagerly splitting every old cell could require >256 boxes even for
        // one pristine concrete wall, permanently deferring a valid collapse.
        let mut exact: Vec<_> = selected.iter().map(|node| node.solid).collect();
        shared::compact(&mut exact);
        let mut refined: Vec<_> = exact
            .into_iter()
            .map(|solid| shared::support::Node {
                solid,
                ..selected[0]
            })
            .collect();
        let available = shared::MAX_RUBBLE_SOLIDS.saturating_sub(box_count);
        let mut index = 0;
        while index < refined.len() {
            let node = refined[index];
            if node.solid.half_size.x.max(node.solid.half_size.y) > 0.18 {
                let split =
                    shared::material::fracture_boxes(node.solid, node.cell as u32 + index as u32);
                if split.len() > 1 && refined.len() + split.len() - 1 <= available {
                    refined.swap_remove(index);
                    refined.extend(
                        split
                            .into_iter()
                            .map(|solid| shared::support::Node { solid, ..node }),
                    );
                    continue;
                }
            }
            index += 1;
        }
        // Defer whole affected structures when the frozen-geometry/body budget
        // is full. Static material and collision remain until slots are free.
        let fragments = shared::support::fragments(&refined);
        // Released shapes are frozen in their fragment descriptors. Their old
        // static cuts have no remaining consumer and must free detail capacity.
        next_cuts.retain(|cut| !removed.contains(&cut.cell));
        if next_cuts.len() > shared::MAX_CUTS
            || box_count + refined.len() > shared::MAX_RUBBLE_SOLIDS
            || body_count + fragments.len() > shared::MAX_RUBBLE
            || fragments
                .iter()
                .any(|f| f.len() > shared::MAX_FRAGMENT_SOLIDS)
        {
            // Capacity release or a new hit will schedule the next attempt.
            state.deferred_structures.insert(structure_id);
            continue;
        }
        for fragment in fragments {
            let boxes: Vec<_> = fragment.iter().map(|&i| refined[i].solid).collect();
            let cell = refined[fragment[0]].cell;
            let center = boxes.iter().map(|b| b.center).sum::<Vec3>() / boxes.len() as f32;
            let impact = state
                .cover_clusters
                .iter()
                .filter(|c| c.structure == structure_id)
                .min_by(|a, b| {
                    a.center
                        .distance_squared(center)
                        .total_cmp(&b.center.distance_squared(center))
                });
            let (point, direction, impulse) = impact.map_or((center, Vec3::ZERO, 0.0), |c| {
                (
                    c.center,
                    c.direction,
                    c.impulse * (-c.center.distance(center) * 3.0).exp(),
                )
            });
            spawn_fragment(
                &mut commands,
                &mut state,
                PendingFragment {
                    cell,
                    boxes: boxes.clone(),
                    point,
                    direction,
                    impulse,
                },
            );
            body_count += 1;
            box_count += boxes.len();
        }
        state.deferred_structures.remove(&structure_id);
        state.cover_cuts = next_cuts;
        state.destroyed_cells = removed;
        for (entity, mut voxel, _) in &mut voxels {
            let cell = &structure.cells[voxel.id as usize];
            if cell.structure != structure_id {
                continue;
            }
            state.remove_cell_marks(cell.id);
            if state.destroyed_cells.contains(&cell.id) {
                voxel.health = 0.0;
                voxel.released = true;
                commands.entity(entity).insert(ColliderDisabled);
            } else {
                let remaining = shared::solids(cell, &state.cover_cuts);
                voxel.shape = shared::collider(&remaining);
                voxel.health = voxel.max_health * remaining.iter().map(|s| s.volume()).sum::<f32>()
                    / (Vec3::from(cell.half_size).element_product() * 8.0);
                commands.entity(entity).insert(voxel.shape.clone());
            }
        }
        state.cover_dirty = true;
    }
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
            state.support_dirty = true;
            let deferred = state.deferred_structures.clone();
            state.support_structures.extend(deferred);
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
                id: piece.id,
                cell: piece.cell,
                position: transform.translation.into(),
                rotation: transform.rotation.to_array(),
                boxes: piece.boxes.clone(),
            })
            .collect(),
    };
    server.broadcast_message(shared::CHANNEL, data::encode(&snapshot));
}
