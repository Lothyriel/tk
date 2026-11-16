//! Authoritative sparse cover damage and its independent bounded snapshot.
use super::*;
use common::map::destruction::{self as shared, CoverSnapshot};

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
        if !state.cover_cuts.contains(&cut) {
            state.cover_cuts.push(cut);
        }
        let solids = shared::solids(cell, &state.cover_cuts);
        if solids.len() <= shared::MAX_SOLIDS && !solids.is_empty() {
            voxel.shape = shared::collider(&solids);
            commands.entity(hit.entity).insert(voxel.shape.clone());
        } else {
            voxel.health = 0.0;
        }
    } else {
        voxel.health = 0.0;
    }
    if voxel.health == 0.0 {
        if !state.destroyed_cells.contains(&voxel.id) {
            state.destroyed_cells.push(voxel.id);
        }
        commands.entity(hit.entity).insert(ColliderDisabled);
    }
    state.cover_dirty = true;
}

pub(super) fn broadcast(
    time: Res<Time>,
    raid: Res<RaidState>,
    mut state: ResMut<WorldState>,
    mut server: ResMut<RenetServer>,
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
        rubble: Vec::new(),
    };
    server.broadcast_message(shared::CHANNEL, data::encode(&snapshot));
}
