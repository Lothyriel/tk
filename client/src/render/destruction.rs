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
        app.add_systems(Startup, rubble_materials).add_systems(
            Update,
            (sync_meshes, sync_rubble)
                .chain()
                .in_set(ClientSystems::Present),
        );
    }
}

#[derive(Resource)]
struct RubbleMaterials {
    wood: Handle<StandardMaterial>,
    concrete: Handle<StandardMaterial>,
}

#[derive(Component)]
struct RubbleVisual {
    cell: u16,
    raid: u32,
}

fn rubble_materials(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(RubbleMaterials {
        wood: materials.add(Color::srgb(0.48, 0.27, 0.1)),
        concrete: materials.add(Color::srgb(0.43, 0.49, 0.53)),
    });
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
    let active = cover.current(&session.raid);
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

#[derive(bevy::ecs::system::SystemParam)]
struct RubblePresentation<'w, 's> {
    materials: Res<'w, RubbleMaterials>,
    meshes: ResMut<'w, Assets<Mesh>>,
    pieces: Query<
        'w,
        's,
        (
            Entity,
            &'static RubbleVisual,
            &'static mut Transform,
            &'static Mesh3d,
        ),
    >,
}

fn sync_rubble(
    mut commands: Commands,
    session: Res<ClientSession>,
    cover: Res<cover::State>,
    time: Res<Time<Real>>,
    presentation: RubblePresentation,
) {
    let RubblePresentation {
        materials,
        mut meshes,
        mut pieces,
    } = presentation;
    let snapshot = cover.current(&session.raid);
    let targets = snapshot.map_or(&[][..], |snapshot| snapshot.rubble.as_slice());
    let alpha = 1.0 - (-time.delta_secs() * 24.0).exp();
    for (entity, visual, mut transform, mesh) in &mut pieces {
        let target = targets
            .iter()
            .find(|target| visual.raid == session.raid.raid_id && target.cell == visual.cell);
        if let Some(target) = target {
            transform.translation = transform.translation.lerp(target.position.into(), alpha);
            transform.rotation = transform
                .rotation
                .slerp(Quat::from_array(target.rotation), alpha);
            transform.scale = Vec3::splat(target.scale);
        } else {
            commands.entity(entity).despawn();
            meshes.remove(mesh.0.id());
        }
    }
    let definitions = barrier_cells();
    for target in targets {
        if pieces.iter().any(|(_, visual, _, _)| {
            visual.raid == session.raid.raid_id && visual.cell == target.cell
        }) {
            continue;
        }
        let Some(cell) = definitions.get(target.cell as usize) else {
            continue;
        };
        let cuts = snapshot.map_or(&[][..], |snapshot| snapshot.cuts.as_slice());
        commands.spawn((
            RubbleVisual {
                cell: target.cell,
                raid: session.raid.raid_id,
            },
            Mesh3d(meshes.add(mesh_for(cell, cuts))),
            MeshMaterial3d(
                if matches!(cell.surface, common::combat::SurfaceKind::Wood) {
                    materials.wood.clone()
                } else {
                    materials.concrete.clone()
                },
            ),
            Transform {
                translation: target.position.into(),
                rotation: Quat::from_array(target.rotation),
                scale: Vec3::splat(target.scale),
            },
        ));
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

    #[test]
    fn destruction_rubble_preserves_identity_interpolates_and_cleans_meshes_on_reset() {
        use bevy::time::TimeUpdateStrategy;
        use common::{
            map::destruction::{CoverSnapshot, RubblePose},
            raid::RaidPhase,
        };
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(
                std::time::Duration::from_secs_f64(1.0 / 165.0),
            ))
            .init_resource::<ClientSession>()
            .init_resource::<cover::State>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_plugins(Plugin);
        {
            let mut session = app.world_mut().resource_mut::<ClientSession>();
            session.raid.raid_id = 5;
            session.raid.phase = RaidPhase::Active;
        }
        let cell = barrier_cells().remove(75);
        let cut = shared::impact_cut(
            &cell,
            Vec3::from(cell.center) + Vec3::Z * cell.half_size[2],
            Vec3::Z,
            common::WeaponKind::Rifle,
        );
        app.world_mut().resource_mut::<cover::State>().snapshot = Some(CoverSnapshot {
            sequence: 4,
            raid_id: 5,
            phase: RaidPhase::Active,
            removed: common::map::destroyed_mask(&[75]),
            cuts: vec![cut],
            rubble: vec![RubblePose {
                cell: 75,
                position: [4.0, 1.0, 2.0],
                rotation: Quat::from_rotation_x(0.3).to_array(),
                scale: 0.45,
            }],
        });
        app.update();
        let entity = app
            .world_mut()
            .query_filtered::<Entity, With<RubbleVisual>>()
            .single(app.world())
            .unwrap();
        let transform = app.world().get::<Transform>(entity).unwrap();
        assert_eq!(transform.translation, Vec3::new(4.0, 1.0, 2.0));
        assert_eq!(transform.rotation, Quat::from_rotation_x(0.3));
        assert_eq!(transform.scale, Vec3::splat(0.45));
        let mesh = app.world().get::<Mesh3d>(entity).unwrap();
        assert!(!ray_hits(
            app.world().resource::<Assets<Mesh>>().get(&mesh.0).unwrap(),
            Vec3::Z
        ));
        app.update();
        app.world_mut()
            .resource_mut::<cover::State>()
            .snapshot
            .as_mut()
            .unwrap()
            .rubble[0]
            .position[0] = 6.0;
        app.update();
        let x = app.world().get::<Transform>(entity).unwrap().translation.x;
        assert!(
            x > 4.0 && x < 6.0,
            "render-frame interpolation must move toward the replicated target: {x}"
        );
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<RubbleVisual>>()
                .single(app.world())
                .unwrap(),
            entity
        );
        app.world_mut().resource_mut::<ClientSession>().raid.phase = RaidPhase::Debrief;
        app.update();
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<RubbleVisual>>()
                .single(app.world())
                .unwrap(),
            entity
        );
        app.world_mut().resource_mut::<ClientSession>().raid.phase = RaidPhase::Lobby;
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&RubbleVisual>()
                .iter(app.world())
                .count(),
            0
        );
        assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 0);
        {
            let mut session = app.world_mut().resource_mut::<ClientSession>();
            session.raid.phase = RaidPhase::Active;
            session.raid.raid_id = 6;
        }
        {
            let mut state = app.world_mut().resource_mut::<cover::State>();
            let snapshot = state.snapshot.as_mut().unwrap();
            snapshot.raid_id = 6;
            snapshot.rubble[0].position = [9.0, 0.3, 2.0];
        }
        app.update();
        let replacement = app
            .world_mut()
            .query_filtered::<Entity, With<RubbleVisual>>()
            .single(app.world())
            .unwrap();
        assert_ne!(replacement, entity);
        assert_eq!(
            app.world()
                .get::<Transform>(replacement)
                .unwrap()
                .translation,
            Vec3::new(9.0, 0.3, 2.0)
        );
        app.world_mut()
            .resource_mut::<cover::State>()
            .snapshot
            .as_mut()
            .unwrap()
            .rubble
            .clear();
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&RubbleVisual>()
                .iter(app.world())
                .count(),
            0
        );
        assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 0);
    }
}
