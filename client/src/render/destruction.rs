//! Cover meshes share their remaining solids with the authoritative colliders.
use bevy::prelude::*;
use common::map::{barrier_cells, destruction as shared};
use std::collections::BTreeMap;

use super::BarrierVisual;
use crate::{
    ClientSystems,
    sync::{ClientSession, cover},
};

pub(super) struct Plugin;
impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sync_meshes.in_set(ClientSystems::Present));
    }
}

fn mesh_for(cell: &common::map::BarrierCell, cuts: &[shared::Cut]) -> Mesh {
    let mut merged: Option<Mesh> = None;
    for solid in shared::solids(cell, cuts) {
        let mesh = Mesh::from(Cuboid::from_size(solid.half_size * 2.0)).translated_by(solid.center);
        if let Some(combined) = &mut merged {
            combined.merge(&mesh).expect("same box attributes");
        } else {
            merged = Some(mesh);
        }
    }
    merged.unwrap_or_else(|| Mesh::from(Cuboid::from_size(Vec3::splat(0.0001))))
}

fn sync_meshes(
    session: Res<ClientSession>,
    cover: Res<cover::State>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut cells: Query<(&BarrierVisual, &mut Mesh3d)>,
    mut applied: Local<BTreeMap<u16, Vec<shared::Cut>>>,
    mut owned: Local<BTreeMap<u16, Handle<Mesh>>>,
) {
    let active = cover.snapshot.as_ref().filter(|snapshot| {
        snapshot.raid_id == session.raid.raid_id && snapshot.phase == session.raid.phase
    });
    let cuts = active.map_or(&[][..], |snapshot| snapshot.cuts.as_slice());
    let definitions = barrier_cells();
    for (cell, mut mesh) in &mut cells {
        let cuts: Vec<_> = cuts
            .iter()
            .filter(|cut| cut.cell == cell.0)
            .cloned()
            .collect();
        if applied
            .get(&cell.0)
            .is_none_or(|previous| *previous != cuts)
        {
            // Intact cells already have their shared startup mesh.
            if !cuts.is_empty() || applied.contains_key(&cell.0) {
                let handle = meshes.add(mesh_for(&definitions[cell.0 as usize], &cuts));
                if let Some(old) = owned.insert(cell.0, handle.clone()) {
                    meshes.remove(old.id());
                }
                mesh.0 = handle;
            }
            applied.insert(cell.0, cuts);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ray_hits(mesh: &Mesh, origin: Vec3) -> bool {
        let direction = -Vec3::Z;
        mesh.triangles().unwrap().any(|triangle| {
            let [a, b, c] = triangle.vertices;
            let e1 = b - a;
            let e2 = c - a;
            let h = direction.cross(e2);
            let det = e1.dot(h);
            if det.abs() < 0.00000001 {
                return false;
            }
            let s = origin - a;
            let u = s.dot(h) / det;
            let q = s.cross(e1);
            let v = direction.dot(q) / det;
            let distance = e2.dot(q) / det;
            u >= 0.0 && v >= 0.0 && u + v <= 1.0 && distance >= 0.0
        })
    }

    #[test]
    fn destruction_render_mesh_has_a_real_bore_and_keeps_adjacent_faces() {
        let cell = barrier_cells().into_iter().find(|c| c.id == 72).unwrap();
        let cut = shared::impact_cut(
            &cell,
            Vec3::from(cell.center) + Vec3::Z * cell.half_size[2],
            Vec3::Z,
            common::WeaponKind::Rifle,
        );
        let intact = mesh_for(&cell, &[]);
        assert!(ray_hits(&intact, Vec3::Z));
        let carved = mesh_for(&cell, &[cut]);
        assert!(!ray_hits(&carved, Vec3::Z));
        assert!(ray_hits(&carved, Vec3::Z + Vec3::X * 0.03));
        let reset = mesh_for(&cell, &[]);
        assert!(ray_hits(&reset, Vec3::Z));
    }
}
