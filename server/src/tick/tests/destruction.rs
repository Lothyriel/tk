use super::*;

#[test]
fn destruction_same_tick_bore_passes_but_adjacent_wood_still_costs_penetration() {
    let (mut app, owner, _) = physical_fixture();
    let cell = common::map::barrier_cells()
        .into_iter()
        .find(|c| c.id == 72)
        .unwrap();
    voxel_shot(&mut app, owner, &cell, 30.0);
    voxel_shot(&mut app, owner, &cell, 30.0);
    let mut adjacent = cell.clone();
    adjacent.center[0] += 0.03;
    voxel_shot(&mut app, owner, &adjacent, 30.0);
    steps(&mut app, 1);
    let mut energies: Vec<_> = app
        .world_mut()
        .query::<&Projectile>()
        .iter(app.world())
        .map(|p| p.penetration)
        .collect();
    energies.sort_by(f32::total_cmp);
    assert_eq!(energies.len(), 3);
    assert!((energies[0] - 25.2).abs() < 0.001);
    assert!((energies[1] - 25.2).abs() < 0.001);
    assert_eq!(energies[2], 30.0);
    let entity = voxel_entity(&mut app, cell.id);
    assert!(app.world().get::<BarrierVoxel>(entity).unwrap().health > 159.0);
    assert!(app.world().get::<ColliderDisabled>(entity).is_none());
    let state = app.world().resource::<WorldState>();
    assert_eq!(state.cover_cuts.len(), 2);
    assert!(
        state
            .cover_cuts
            .iter()
            .all(|cut| (cut.half_size[0] * 2.0 - 0.006).abs() < 0.001
                && (cut.half_size[1] * 2.0 - 0.006).abs() < 0.001)
    );
    assert!(
        state.impact_marks.is_empty(),
        "an opened bore must not be covered by a floating quad"
    );
}

#[test]
fn destruction_oblique_shot_crossing_cell_seam_pays_preimpact_full_thickness_once() {
    for clearance in [false, true] {
        let (mut app, owner, _) = physical_fixture();
        let direction = Vec3::new(0.6, 0.0, -0.8);
        let contact = Vec3::new(-7.501, 1.35, -4.88);
        for id in [9001, 9002] {
            let mut projectile = app.world_mut().spawn((
                Projectile {
                    id,
                    weapon: WeaponId::M4A1,
                    velocity: direction * 100.0,
                    damage: 34.0,
                    penetration: 30.0,
                    lifetime: 3.0,
                    owner_entity: owner,
                    penetrated: vec![],
                },
                Transform::from_translation(if clearance {
                    contact + direction * 0.005
                } else {
                    contact - direction * 0.60
                }),
            ));
            if clearance {
                // The muzzle is beyond the first cell's side seam, inside the
                // next wood cell. Flight then starts in already-charged material.
                projectile.insert(super::super::MuzzleClearance(contact - direction * 0.60));
            }
        }
        steps(&mut app, 1);
        let energies: std::collections::BTreeMap<_, _> = app
            .world_mut()
            .query::<&Projectile>()
            .iter(app.world())
            .map(|p| (p.id, p.penetration))
            .collect();
        assert_eq!(energies.len(), 2);
        // 0.24m normal thickness / cos(angle)=0.30m; wood costs20 units/m.
        // Allow only the resolver's 2mm traversal epsilon at the internal seam.
        assert!(
            (energies[&9001] - 24.0).abs() < 0.05,
            "new bore cannot grant its creating shot free neighbor material or double-charge: clearance={clearance}, {energies:?}"
        );
        assert_eq!(
            energies[&9002], 30.0,
            "next same-tick shot uses the now-open bore: clearance={clearance}"
        );
    }
}

#[test]
fn destruction_concrete_chips_stop_the_round_and_leave_material_behind() {
    let (mut app, owner, _) = physical_fixture();
    let cell = common::map::barrier_cells().remove(2);
    voxel_shot(&mut app, owner, &cell, 26.0);
    steps(&mut app, 2);
    let entity = voxel_entity(&mut app, cell.id);
    assert!(app.world().get::<BarrierVoxel>(entity).unwrap().health > 99.0);
    assert_eq!(app.world().resource::<WorldState>().cover_cuts.len(), 1);
    assert_eq!(
        app.world_mut()
            .query::<&Projectile>()
            .iter(app.world())
            .count(),
        0
    );
    let center = Vec3::from(cell.center);
    let shape = &app.world().get::<Collider>(entity).unwrap();
    assert!(
        shape
            .cast_ray(
                center,
                Quat::IDENTITY,
                center + Vec3::Z,
                -Vec3::Z,
                2.0,
                true
            )
            .is_some()
    );
    assert!(app.world().get::<ColliderDisabled>(entity).is_none());
}

#[test]
fn destruction_marks_expire_with_support_but_preserve_other_surfaces_and_reset() {
    let (mut app, owner, _) = physical_fixture();
    let cell = common::map::barrier_cells()
        .into_iter()
        .find(|c| c.id == 72)
        .unwrap();
    {
        let mut state = app.world_mut().resource_mut::<WorldState>();
        state.impact_on_cell(Vec3::ZERO, Vec3::Y, 25, Some(0));
        state.impact(Vec3::new(2.0, 0.1, 2.0), Vec3::Y, 25);
        state.impact_on_cell(Vec3::from(cell.center), Vec3::Z, 50, Some(cell.id));
        state.impact_on_cell(Vec3::from(cell.center), -Vec3::Z, 50, Some(cell.id));
    }
    {
        let state = app.world().resource::<WorldState>();
        assert_eq!(
            state.impact_marks.len(),
            4,
            "wood entry/exit plus two unrelated marks"
        );
        assert_eq!(
            state
                .impact_cells
                .values()
                .filter(|&&id| id == cell.id)
                .count(),
            2
        );
    }
    voxel_shot(&mut app, owner, &cell, 1000.0);
    steps(&mut app, 2);
    {
        let state = app.world().resource::<WorldState>();
        assert_eq!(state.impact_marks.len(), 2);
        assert!(
            state
                .impact_marks
                .iter()
                .any(|mark| mark.pos == [2.0, 0.1, 2.0])
        );
        assert_eq!(
            state.impact_cells.values().copied().collect::<Vec<_>>(),
            vec![0]
        );
        assert!(!state.impact_cells.values().any(|&id| id == cell.id));
    }
    app.world_mut().resource_mut::<RaidState>().phase = RaidPhase::Debrief;
    ready(&mut app, owner);
    steps(&mut app, 3);
    let state = app.world().resource::<WorldState>();
    assert!(state.impact_marks.is_empty());
    assert!(state.impact_cells.is_empty());
}

/// Remove a narrow ligament across the panel, not entire cells. This fixture
/// isolates the real support/release solver; native input is checked separately.
fn undercut_left_panel(app: &mut App) -> f32 {
    undercut_structure(app, 6)
}

fn undercut_structure(app: &mut App, structure: u16) -> f32 {
    let cells = common::map::barrier_cells();
    let mut volume = 0.0;
    {
        let mut state = app.world_mut().resource_mut::<WorldState>();
        for cell in cells.iter().filter(|cell| {
            cell.structure == structure && cell.center[1] - cell.half_size[1] <= 0.1001
        }) {
            let cut = common::map::destruction::Cut {
                cell: cell.id,
                center: [0.0, 0.16 - cell.center[1], 0.0],
                half_size: [cell.half_size[0], 0.015, cell.half_size[2]],
            };
            let before: f32 = common::map::destruction::solids(cell, &state.cover_cuts)
                .iter()
                .map(|s| s.volume())
                .sum();
            state.cover_cuts.push(cut);
            let after: f32 = common::map::destruction::solids(cell, &state.cover_cuts)
                .iter()
                .map(|s| s.volume())
                .sum();
            volume += before - after;
        }
        state.support_dirty = true;
        state.cover_dirty = true;
    }
    steps(app, 2);
    volume
}

#[test]
fn destruction_whole_concrete_section_collapses_within_frozen_geometry_budget() {
    let (mut app, _, _) = physical_fixture();
    let original: f32 = common::map::barrier_cells()
        .iter()
        .filter(|c| c.structure == 5)
        .map(|c| Vec3::from(c.half_size).element_product() * 8.0)
        .sum();
    let fines = undercut_structure(&mut app, 5);
    let (standing, detached) = material_volume(&mut app, 5);
    assert!(
        detached > original * 0.8,
        "a fully undercut concrete section must not float forever"
    );
    assert!((standing + detached + fines - original).abs() < original * 0.0001);
    let pieces: Vec<_> = app
        .world_mut()
        .query::<&super::super::destruction::Rubble>()
        .iter(app.world())
        .collect();
    assert!(pieces.len() > 1 && pieces.len() <= common::map::destruction::MAX_RUBBLE);
    assert!(
        pieces.iter().map(|p| p.boxes.len()).sum::<usize>()
            <= common::map::destruction::MAX_RUBBLE_SOLIDS
    );
    assert!(
        app.world()
            .resource::<WorldState>()
            .deferred_structures
            .is_empty()
    );
    steps(&mut app, 256);
    for pose in app
        .world_mut()
        .query_filtered::<&Transform, With<super::super::destruction::Rubble>>()
        .iter(app.world())
    {
        assert!(pose.translation.is_finite() && pose.rotation.is_finite());
        assert_eq!(pose.scale, Vec3::ONE);
    }
}

#[test]
fn destruction_detached_cell_frees_cut_budget_without_changing_frozen_fragments() {
    use bevy::ecs::system::RunSystemOnce;
    use common::map::destruction::{self as shared, SolidBox};
    let (mut app, owner, _) = physical_fixture();
    let cells = common::map::barrier_cells();
    let source = &cells[75];
    for i in 0..12 {
        app.world_mut()
            .resource_mut::<WorldState>()
            .cover_cuts
            .push(shared::Cut {
                cell: source.id,
                center: [0.0, -0.20 + i as f32 * 0.035, 0.0],
                half_size: [0.07, 0.001, source.half_size[2]],
            });
    }
    undercut_left_panel(&mut app);
    let state = app.world().resource::<WorldState>();
    assert!(state.destroyed_cells.contains(&source.id));
    assert!(
        state.cover_cuts.iter().all(|cut| cut.cell != source.id),
        "released source detail must be pruned"
    );
    let frozen: std::collections::BTreeMap<_, _> = app
        .world_mut()
        .query::<&super::super::destruction::Rubble>()
        .iter(app.world())
        .map(|r| (r.id, r.boxes.clone()))
        .collect();
    assert!(!frozen.is_empty());
    let existing = app.world().resource::<WorldState>().cover_cuts.clone();
    // Even a propagated cutter covering the removed source must not recreate
    // its unused cut records or its collision behind the removal mask.
    let next = super::super::destruction::add_cuts(
        &existing,
        &cells,
        source.structure,
        &[SolidBox {
            center: source.center.into(),
            half_size: source.half_size.into(),
        }],
        &app.world().resource::<WorldState>().destroyed_cells,
    )
    .unwrap();
    assert_eq!(next, existing);
    let other = &cells[0];
    app.world_mut()
        .run_system_once_with(
            damage_at,
            (
                owner,
                other.id,
                Vec3::from(other.center) + Vec3::Z * other.half_size[2],
                1.0,
            ),
        )
        .unwrap();
    steps(&mut app, 1);
    assert!(
        app.world()
            .resource::<WorldState>()
            .cover_cuts
            .iter()
            .any(|cut| cut.cell == other.id)
    );
    for piece in app
        .world_mut()
        .query::<&super::super::destruction::Rubble>()
        .iter(app.world())
    {
        if let Some(shape) = frozen.get(&piece.id) {
            assert_eq!(&piece.boxes, shape);
        }
    }
    steps(&mut app, 2561);
    assert!(
        app.world()
            .resource::<WorldState>()
            .cover_cuts
            .iter()
            .all(|cut| cut.cell != source.id)
    );
}

fn material_volume(app: &mut App, structure: u16) -> (f32, f32) {
    let cells = common::map::barrier_cells();
    let state = app.world().resource::<WorldState>();
    let static_volume = cells
        .iter()
        .filter(|c| c.structure == structure && !state.destroyed_cells.contains(&c.id))
        .flat_map(|cell| common::map::destruction::solids(cell, &state.cover_cuts))
        .map(|s| s.volume())
        .sum();
    let rubble_volume = app
        .world_mut()
        .query::<&super::super::destruction::Rubble>()
        .iter(app.world())
        .filter(|piece| cells[piece.cell as usize].structure == structure)
        .flat_map(|piece| &piece.boxes)
        .map(|b| Vec3::from(b.half_size).element_product() * 8.0)
        .sum();
    (static_volume, rubble_volume)
}

#[test]
fn destruction_undercut_releases_irregular_volume_preserving_fragments_and_settles() {
    let (mut app, _, _) = physical_fixture();
    let original: f32 = common::map::barrier_cells()
        .iter()
        .filter(|c| c.structure == 6)
        .map(|c| Vec3::from(c.half_size).element_product() * 8.0)
        .sum();
    let fines = undercut_left_panel(&mut app);
    let (static_volume, rubble_volume) = material_volume(&mut app, 6);
    assert!(rubble_volume > original * 0.8);
    assert!(
        static_volume > 0.0,
        "the narrow intact floor-side material must remain"
    );
    assert!((static_volume + rubble_volume + fines - original).abs() < 1e-5);
    let initial: std::collections::BTreeMap<_, _> = app
        .world_mut()
        .query::<(Entity, &super::super::destruction::Rubble, &Transform)>()
        .iter(app.world())
        .map(|(entity, piece, pose)| (piece.id, (entity, pose.translation, piece.boxes.clone())))
        .collect();
    assert!(!initial.is_empty() && initial.len() <= common::map::destruction::MAX_RUBBLE);
    assert!(initial.values().any(|(_, _, boxes)| boxes.len() > 1));
    let mut reference = std::collections::BTreeMap::new();
    let mut maximum_drift = 0.0_f32;
    for tick in 0..1024 {
        steps(&mut app, 1);
        for (entity, piece, pose, velocity) in app
            .world_mut()
            .query::<(
                Entity,
                &super::super::destruction::Rubble,
                &Transform,
                &Velocity,
            )>()
            .iter(app.world())
        {
            assert_eq!(initial[&piece.id].0, entity);
            assert_eq!(
                initial[&piece.id].2, piece.boxes,
                "detached shape is frozen"
            );
            assert_eq!(pose.scale, Vec3::ONE);
            assert!(pose.translation.is_finite() && pose.rotation.is_finite());
            assert!(velocity.linear.is_finite() && velocity.angular.is_finite());
            for b in &piece.boxes {
                for bits in 0..8 {
                    let sign = Vec3::new(
                        if bits & 1 == 0 { -1.0 } else { 1.0 },
                        if bits & 2 == 0 { -1.0 } else { 1.0 },
                        if bits & 4 == 0 { -1.0 } else { 1.0 },
                    );
                    let corner =
                        pose.transform_point(Vec3::from(b.center) + Vec3::from(b.half_size) * sign);
                    assert!(corner.y > 0.065, "fragment escaped floor: {corner:?}");
                }
            }
            if tick == 768 {
                reference.insert(piece.id, pose.translation);
            }
            if tick > 768 {
                maximum_drift = maximum_drift.max(pose.translation.distance(reference[&piece.id]));
            }
        }
    }
    assert!(maximum_drift < 0.03, "late fragment drift={maximum_drift}");
    assert!(
        app.world_mut()
            .query::<&Transform>()
            .iter(app.world())
            .any(|t| t.translation.y < 0.5)
    );
}

#[test]
fn destruction_rubble_late_join_reconstructs_frozen_geometry_and_reset_restores_cover() {
    use common::map::destruction::{CHANNEL, CoverSnapshot};
    let (mut app, owner, _) = physical_fixture();
    undercut_left_panel(&mut app);
    let mut peer = bevy_renet2::prelude::RenetClient::new(data::renet_config(), false);
    peer.set_connected();
    app.world_mut()
        .resource_mut::<RenetServer>()
        .add_connection(999, false);
    let mut seen = 0;
    for tick in 0..32 {
        steps(&mut app, 1);
        for packet in app
            .world_mut()
            .resource_mut::<RenetServer>()
            .get_packets_to_send(999)
            .unwrap()
        {
            if tick >= 8 {
                peer.process_packet(packet);
            }
        }
        while let Some(bytes) = peer.receive_message(CHANNEL) {
            let snapshot = data::decode::<CoverSnapshot>(&bytes);
            assert!(snapshot.valid());
            assert!(bytes.len() < common::map::destruction::MAX_SNAPSHOT_BYTES);
            assert!(!snapshot.rubble.is_empty());
            let actual: std::collections::BTreeMap<_, _> = app
                .world_mut()
                .query::<(&super::super::destruction::Rubble, &Transform)>()
                .iter(app.world())
                .map(|(piece, t)| (piece.id, (piece.boxes.clone(), *t)))
                .collect();
            for piece in snapshot.rubble {
                assert_eq!(piece.boxes, actual[&piece.id].0);
                assert_eq!(piece.position, actual[&piece.id].1.translation.to_array());
            }
            seen += 1;
        }
    }
    assert!(seen >= 3);
    app.world_mut().resource_mut::<RaidState>().phase = RaidPhase::Debrief;
    ready(&mut app, owner);
    steps(&mut app, 3);
    assert_eq!(
        app.world_mut()
            .query::<&super::super::destruction::Rubble>()
            .iter(app.world())
            .count(),
        0
    );
    let state = app.world().resource::<WorldState>();
    assert!(state.cover_cuts.is_empty() && state.destroyed_cells.is_empty());
    assert!(state.cover_clusters.is_empty());
    assert!(
        app.world_mut()
            .query::<&BarrierVoxel>()
            .iter(app.world())
            .all(|v| v.health == v.max_health && !v.released)
    );
}

fn damage_at(
    In((owner, id, point, damage)): In<(Entity, u16, Vec3, f32)>,
    mut commands: Commands,
    mut state: ResMut<WorldState>,
    mut voxels: super::super::destruction::Voxels,
    rubble: Query<&super::super::destruction::Rubble>,
) {
    let entity = voxels.iter().find(|(_, cell, _)| cell.id == id).unwrap().0;
    let projectile = Projectile {
        id: 9001,
        weapon: WeaponId::M4A1,
        velocity: -Vec3::Z * 100.0,
        damage,
        penetration: 30.0,
        lifetime: 3.0,
        owner_entity: owner,
        penetrated: Vec::new(),
    };
    super::super::destruction::damage(
        &mut commands,
        &mut state,
        &mut voxels,
        super::super::destruction::Hit {
            entity,
            point,
            normal: Vec3::Z,
            direction: Vec3::NEG_Z,
        },
        &projectile,
        (
            rubble.iter().count(),
            rubble.iter().map(|r| r.boxes.len()).sum(),
        ),
    );
}

#[test]
fn destruction_tiny_dispersed_hits_and_budget_exhaustion_never_delete_healthy_material() {
    use bevy::ecs::system::RunSystemOnce;
    use common::map::destruction::{MAX_CUTS, MAX_STATIC_SOLIDS, solids};
    let (mut app, owner, _) = physical_fixture();
    let cells = common::map::barrier_cells();
    for cell in &cells[..MAX_CUTS] {
        let point = Vec3::from(cell.center) + Vec3::Z * cell.half_size[2];
        app.world_mut()
            .run_system_once_with(damage_at, (owner, cell.id, point, 0.001))
            .unwrap();
    }
    let original = app.world().resource::<WorldState>().cover_cuts.clone();
    let next = &cells[144];
    app.world_mut()
        .run_system_once_with(
            damage_at,
            (
                owner,
                next.id,
                Vec3::from(next.center) + Vec3::Z * next.half_size[2],
                0.001,
            ),
        )
        .unwrap();
    let state = app.world().resource::<WorldState>();
    assert_eq!(
        state.cover_cuts, original,
        "saturation must preserve every old hole"
    );
    assert!(
        state.destroyed_cells.is_empty(),
        "detail pressure must not remove healthy cells"
    );
    let count: usize = cells
        .iter()
        .map(|c| solids(c, &state.cover_cuts).len())
        .sum();
    assert!(count <= MAX_STATIC_SOLIDS);
    assert!(
        app.world_mut()
            .query::<&BarrierVoxel>()
            .iter(app.world())
            .all(|v| v.health > v.max_health * 0.99)
    );
}

#[test]
fn destruction_crossing_slits_enforce_global_geometry_and_support_work_bounds() {
    use common::map::destruction::{self as shared, SolidBox};
    let cells = common::map::barrier_cells();
    let mut cuts = Vec::new();
    let mut rejected = false;
    let mut maximum = 0;
    let mut max_rebuild = std::time::Duration::ZERO;
    let mut max_solve = std::time::Duration::ZERO;
    let began = std::time::Instant::now();
    // Five disjoint old cells, each approaching a13×14 grid. Every local cell
    // stays below384 solids; this reaches the GLOBAL1024 bound before128 cuts.
    'pressure: for id in [72, 76, 80, 84, 88] {
        let cell = &cells[id];
        for index in 0..25 {
            let (axis, slot, count) = if index < 13 {
                (0, index, 13)
            } else {
                (1, index - 13, 12)
            };
            let mut half = Vec3::from(cell.half_size);
            half[axis] = 0.001;
            let mut center = Vec3::from(cell.center);
            center[axis] += -0.22 + slot as f32 * 0.44 / (count - 1) as f32;
            let cutter = SolidBox {
                center,
                half_size: half,
            };
            let rebuild = std::time::Instant::now();
            match super::super::destruction::add_cuts(&cuts, &cells, cell.structure, &[cutter], &[])
            {
                Some(next) => cuts = next,
                None => {
                    rejected = true;
                    break 'pressure;
                }
            }
            max_rebuild = max_rebuild.max(rebuild.elapsed());
            let nodes: Vec<_> = cells
                .iter()
                .flat_map(|c| {
                    shared::solids(c, &cuts)
                        .into_iter()
                        .map(|s| shared::support::Node {
                            cell: c.id,
                            structure: c.structure,
                            surface: c.surface,
                            solid: s.translated(c.center.into()),
                        })
                })
                .collect();
            maximum = maximum.max(nodes.len());
            assert!(nodes.len() <= shared::MAX_STATIC_SOLIDS);
            let solve = std::time::Instant::now();
            let _ = shared::support::unsupported(&nodes);
            max_solve = max_solve.max(solve.elapsed());
        }
    }
    assert!(
        rejected && cuts.len() < shared::MAX_CUTS,
        "global geometry must saturate before the cut counter"
    );
    assert!(
        maximum > shared::MAX_STATIC_SOLIDS * 9 / 10,
        "exercise a near-saturated graph, not just pristine cells"
    );
    eprintln!(
        "crossing slit nodes_max={maximum}, cuts={}, rebuild_max_us={}, support_max_us={}, total_ms={}",
        cuts.len(),
        max_rebuild.as_micros(),
        max_solve.as_micros(),
        began.elapsed().as_millis()
    );
}

#[test]
fn destruction_fragments_expire_without_restoring_static_material() {
    let (mut app, _, _) = physical_fixture();
    undercut_left_panel(&mut app);
    let removed = app.world().resource::<WorldState>().destroyed_cells.clone();
    assert!(!removed.is_empty());
    steps(&mut app, 2561);
    assert_eq!(
        app.world_mut()
            .query::<&super::super::destruction::Rubble>()
            .iter(app.world())
            .count(),
        0
    );
    assert_eq!(
        app.world().resource::<WorldState>().destroyed_cells,
        removed
    );
}

#[test]
fn destruction_normal_clustered_input_releases_material_specific_chunks() {
    for (position, target, structure) in [
        (Vec3::new(-7.0, 1.05, -3.0), Vec3::new(-7.2, 1.35, -4.88), 6),
        (Vec3::new(-1.5, 1.05, -0.8), Vec3::new(-1.85, 1.1, -2.4), 5),
    ] {
        let (mut app, owner, _) = physical_fixture();
        app.world_mut()
            .get_mut::<Transform>(owner)
            .unwrap()
            .translation = position;
        steps(&mut app, 3);
        let original: f32 = common::map::barrier_cells()
            .iter()
            .filter(|c| c.structure == structure)
            .map(|c| Vec3::from(c.half_size).element_product() * 8.0)
            .sum();
        for shot in 0..12 {
            // Edge hits stay near the previous cavity without firing down its
            // empty centre; actual production input and ammunition are used.
            let offset =
                Vec3::X * ((shot % 3) as f32 - 1.0) * 0.025 + Vec3::Y * ((shot / 3) as f32) * 0.012;
            normal_shot(&mut app, owner, target + offset);
            let fragments: Vec<_> = app
                .world_mut()
                .query::<&super::super::destruction::Rubble>()
                .iter(app.world())
                .filter(|r| common::map::barrier_cells()[r.cell as usize].structure == structure)
                .collect();
            if !fragments.is_empty() {
                break;
            }
        }
        let (standing, detached) = material_volume(&mut app, structure);
        assert!(
            detached > 0.0,
            "normal clustered shots must produce breakoff in structure={structure}"
        );
        assert!(
            standing > original * 0.9,
            "local damage must preserve the surrounding wall"
        );
        let fines = app.world().resource::<WorldState>().cover_fines;
        assert!(
            (standing + detached + fines - original).abs() < original * 0.001,
            "material must be conserved: static={standing}, fragments={detached}, fines={fines}, original={original}"
        );
    }
}

#[test]
fn destruction_correct_native_wood_corner_rays_progress_within_existing_detail_caps() {
    let (mut app, owner, _) = physical_fixture();
    let eye = Vec3::new(-8.61139, 1.6520071, -3.3320434);
    let focus = Vec3::new(-8.93, 2.03, -4.88);
    let original: f32 = common::map::barrier_cells()
        .iter()
        .filter(|cell| cell.structure == 6)
        .map(|cell| Vec3::from(cell.half_size).element_product() * 8.0)
        .sum();
    let mut observed = Vec::new();
    for (index, offset) in [
        Vec3::ZERO,
        Vec3::new(-0.012, 0.003, 0.0),
        Vec3::new(0.012, -0.003, 0.0),
        Vec3::new(0.0, 0.012, 0.0),
    ]
    .into_iter()
    .enumerate()
    {
        let contact = focus + offset;
        let direction = (contact - eye).normalize();
        app.world_mut().spawn((
            Projectile {
                id: 9001 + index as u64,
                weapon: WeaponId::M4A1,
                velocity: direction * 100.0,
                damage: 34.0,
                penetration: 36.0,
                lifetime: 3.0,
                owner_entity: owner,
                penetrated: vec![],
            },
            Transform::from_translation(contact - direction * 0.60),
        ));
        steps(&mut app, 2);
        observed.push(app.world().resource::<WorldState>().cover_cuts.len());
        if app
            .world_mut()
            .query::<&super::super::destruction::Rubble>()
            .iter(app.world())
            .next()
            .is_some()
        {
            break;
        }
    }
    let (standing, physical) = material_volume(&mut app, 6);
    let state = app.world().resource::<WorldState>();
    assert!(
        physical > 0.0,
        "correct wood-v3 rays must progress to physical fragments under fixed caps; cuts={observed:?}, clusters={:?}",
        state.cover_clusters
    );
    assert!(
        standing > original * 0.9,
        "a local corner hit cannot delete surrounding healthy wall"
    );
    assert!(state.cover_fines >= -1e-6);
    assert!(
        (standing + physical + state.cover_fines - original).abs() < original * 0.0001,
        "all removed material must remain physical or recorded fines: standing={standing}, physical={physical}, fines={}",
        state.cover_fines
    );
    assert!(state.cover_cuts.len() <= common::map::destruction::MAX_CUTS);
    let pieces: Vec<_> = app
        .world_mut()
        .query::<&super::super::destruction::Rubble>()
        .iter(app.world())
        .collect();
    assert!(pieces.len() <= common::map::destruction::MAX_RUBBLE);
    assert!(
        pieces.iter().map(|p| p.boxes.len()).sum::<usize>()
            <= common::map::destruction::MAX_RUBBLE_SOLIDS
    );
}

#[test]
fn destruction_native_concrete_spall_escapes_parent_cavity_and_settles() {
    let root = Vec3::new(-1.6414638, 1.1417292, -1.2345289);
    for eye in [
        root + Vec3::Y * PLAYER_VIEW_HEIGHT,
        Vec3::new(-1.6580098, 1.738023, -1.3489486),
    ] {
        let (mut app, owner, _) = physical_fixture();
        let spec = WeaponKind::Rifle.spec();
        let offset = common::weapon_animation::resolved_muzzle_offset(spec, 0.0);
        for (index, target) in [
            Vec3::new(-1.96, 1.16, -2.4),
            Vec3::new(-1.9719898, 1.1604958, -2.3999999),
            Vec3::new(-1.948041, 1.1590091, -2.3999999),
            Vec3::new(-1.9719079, 1.1614841, -2.4099834),
            Vec3::new(-1.9481636, 1.1580251, -2.4100506),
        ]
        .into_iter()
        .enumerate()
        {
            // Reproduce native-v5 AP shots through the actual configured outlet,
            // including production eye-to-muzzle clearance before ordinary flight.
            let delta = target - eye;
            let forward =
                (delta.length_squared() - offset.x * offset.x - offset.y * offset.y).sqrt();
            let pitch = (delta.y / (forward * forward + offset.y * offset.y).sqrt()).asin()
                - offset.y.atan2(forward);
            let local_z = offset.y * pitch.sin() - forward * pitch.cos();
            let yaw = (-delta.x).atan2(-delta.z) + offset.x.atan2(-local_z);
            let rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
            let muzzle = eye + rotation * offset;
            let direction = rotation * Vec3::NEG_Z;
            assert!((target - muzzle).cross(direction).length() < 0.001);
            app.world_mut().spawn((
                Projectile {
                    id: index as u64,
                    weapon: WeaponId::M4A1,
                    velocity: direction * spec.muzzle_speed,
                    damage: AmmoKind::Ap.damage(spec),
                    penetration: AmmoKind::Ap.penetration(spec),
                    lifetime: 3.0,
                    owner_entity: owner,
                    penetrated: vec![],
                },
                Transform::from_translation(muzzle),
                super::super::MuzzleClearance(eye),
            ));
            steps(&mut app, 2);
        }
        let (id, first_entity, initial) = app
            .world_mut()
            .query::<(Entity, &super::super::destruction::Rubble, &Transform)>()
            .iter(app.world())
            .min_by_key(|(_, piece, _)| piece.id)
            .map(|(entity, piece, pose)| (piece.id, entity, *pose))
            .expect("the native concrete-angle hits must create physical spall");
        let centers: Vec<_> = app
            .world_mut()
            .query::<(&super::super::destruction::Rubble, &Transform)>()
            .iter(app.world())
            .flat_map(|(piece, pose)| {
                piece
                    .boxes
                    .iter()
                    .map(|b| pose.transform_point(Vec3::from(b.center)))
                    .collect::<Vec<_>>()
            })
            .collect();
        let blocked = centers
            .iter()
            .filter(|point| {
                app.world_mut()
                    .query::<(&BarrierVoxel, &Transform)>()
                    .iter(app.world())
                    .any(|(cell, pose)| {
                        cell.health > 0.0
                            && cell
                                .shape
                                .cast_ray(
                                    pose.translation,
                                    pose.rotation,
                                    **point,
                                    Vec3::Z,
                                    0.15,
                                    true,
                                )
                                .is_some()
                    })
            })
            .count();
        assert_eq!(
            blocked, 0,
            "progressive concrete spall must not be sealed behind an old narrow front mouth"
        );
        let neighbor = Vec3::new(-1.80, 1.05, -2.30);
        assert!(
            app.world_mut()
                .query::<(&BarrierVoxel, &Transform)>()
                .iter(app.world())
                .any(|(cell, pose)| cell.health > 0.0
                    && cell
                        .shape
                        .cast_ray(
                            pose.translation,
                            pose.rotation,
                            neighbor,
                            Vec3::NEG_Z,
                            0.30,
                            true
                        )
                        .is_some()),
            "opening the local spall rim must preserve neighboring concrete"
        );
        let births: std::collections::BTreeMap<_, _> = app
            .world_mut()
            .query::<(&super::super::destruction::Rubble, &Transform)>()
            .iter(app.world())
            .map(|(piece, pose)| (piece.id, pose.translation))
            .collect();
        let mut motions = std::collections::BTreeMap::<u32, f32>::new();
        let mut escaped = std::collections::BTreeSet::new();
        let mut late_positions = std::collections::BTreeMap::new();
        let mut late_drifts = std::collections::BTreeMap::<u32, f32>::new();
        for tick in 0..1024 {
            steps(&mut app, 1);
            for (piece, pose, velocity) in app
                .world_mut()
                .query::<(&super::super::destruction::Rubble, &Transform, &Velocity)>()
                .iter(app.world())
            {
                assert!(pose.translation.is_finite() && pose.rotation.is_finite());
                assert!(velocity.linear.is_finite() && velocity.angular.is_finite());
                let motion = motions.entry(piece.id).or_default();
                *motion = motion.max(pose.translation.distance(births[&piece.id]));
                if pose.translation.z > -2.38 || pose.translation.x < -2.02 {
                    escaped.insert(piece.id);
                }
                if tick == 768 {
                    late_positions.insert(piece.id, pose.translation);
                }
                if tick > 768 {
                    let drift = late_drifts.entry(piece.id).or_default();
                    *drift = drift.max(pose.translation.distance(late_positions[&piece.id]));
                }
            }
        }
        for (piece, pose, velocity, sleeping) in app
            .world_mut()
            .query::<(
                &super::super::destruction::Rubble,
                &Transform,
                &Velocity,
                &Sleeping,
            )>()
            .iter(app.world())
        {
            let mut lo = Vec3::splat(f32::INFINITY);
            let mut hi = Vec3::splat(f32::NEG_INFINITY);
            for b in &piece.boxes {
                for bits in 0..8 {
                    let sign = Vec3::new(
                        if bits & 1 == 0 { -1.0 } else { 1.0 },
                        if bits & 2 == 0 { -1.0 } else { 1.0 },
                        if bits & 4 == 0 { -1.0 } else { 1.0 },
                    );
                    let corner =
                        pose.transform_point(Vec3::from(b.center) + Vec3::from(b.half_size) * sign);
                    lo = lo.min(corner);
                    hi = hi.max(corner);
                }
            }
            eprintln!(
                "native concrete final id={} born={:?} pose={pose:?} velocity={velocity:?} sleeping={} bounds={lo:?}..{hi:?} motion_m={}",
                piece.id, births[&piece.id], sleeping.sleeping, motions[&piece.id]
            );
        }
        // The native replay exposed an overconstraint in the original oracle:
        // the first chip legitimately moves12mm and rests on the remaining rim,
        // while two other chips travel >1m to the floor. Require actual ejection
        // and settling, without forcing every released shape off a valid ledge.
        assert!(
            escaped
                .iter()
                .any(|piece| motions[piece] > 0.02 && late_drifts[piece] < 0.03),
            "at least one tracked chip must leave its preserved parent and settle: motions={motions:?}, escaped={escaped:?}"
        );
        assert!(
            motions[&id] > 0.005 && late_drifts[&id] < 0.03,
            "the retained first chip must move visibly and settle, not reproduce submillimetre pinning"
        );
        if !escaped.contains(&id) {
            let mut query = app.world_mut().query::<(
                &RapierContextSimulation,
                &RapierContextColliders,
                &RapierRigidBodySet,
            )>();
            let (simulation, colliders, bodies) = query.single(app.world()).unwrap();
            let mut upward_parent_contact = false;
            let mut deepest = 0.0_f32;
            for pair in simulation.contact_pairs_with(colliders, bodies, first_entity) {
                if !pair.has_any_active_contact() {
                    continue;
                }
                let first = pair.collider1() == Some(first_entity);
                let other = if first {
                    pair.collider2()
                } else {
                    pair.collider1()
                };
                if other
                    .and_then(|entity| app.world().get::<BarrierVoxel>(entity))
                    .is_none()
                {
                    continue;
                }
                for manifold in pair.manifolds() {
                    let support = if first {
                        -manifold.normal()
                    } else {
                        manifold.normal()
                    };
                    for contact in manifold.points() {
                        deepest = deepest.max((-contact.dist()).max(0.0));
                        upward_parent_contact |= support.y > 0.2 && contact.dist() < 0.002;
                    }
                }
            }
            eprintln!(
                "native concrete first-chip ledge contact upward={upward_parent_contact} penetration_m={deepest} motion_m={} born={:?}",
                motions[&id], initial.translation
            );
            assert!(
                upward_parent_contact,
                "a rim-resting chip needs a real supporting parent contact"
            );
            assert!(
                deepest < 0.001,
                "resting contact cannot hide deeper than1mm penetration: {deepest}"
            );
        }
    }
}

#[test]
fn destruction_equal_clustered_and_dispersed_damage_has_different_local_failure() {
    use bevy::ecs::system::RunSystemOnce;
    let mut outcomes = Vec::new();
    for spacing in [0.025, 0.20] {
        let (mut app, owner, _) = physical_fixture();
        let cell = common::map::barrier_cells().remove(72);
        let original: f32 = common::map::barrier_cells()
            .iter()
            .filter(|c| c.structure == cell.structure)
            .map(|c| Vec3::from(c.half_size).element_product() * 8.0)
            .sum();
        for i in 0..3 {
            let point =
                Vec3::from(cell.center) + Vec3::new(i as f32 * spacing, 0.0, cell.half_size[2]);
            app.world_mut()
                .run_system_once_with(damage_at, (owner, cell.id, point, 34.0))
                .unwrap();
            steps(&mut app, 1);
        }
        let (standing, fragments) = material_volume(&mut app, cell.structure);
        assert!(
            standing > original * 0.99,
            "equal damage cannot erase a healthy cell pool"
        );
        outcomes.push((original - standing, fragments));
    }
    assert!(
        outcomes[0].0 > outcomes[1].0 * 5.0,
        "nearby hits must grow a localized cavity: {outcomes:?}"
    );
    assert!(
        outcomes[0].1 > 0.0 && outcomes[1].1 == 0.0,
        "only degraded local material breaks off: {outcomes:?}"
    );
}

#[test]
fn destruction_full_body_capacity_defers_without_rechecking_unchanged_support_every_tick() {
    use common::map::destruction::{MAX_RUBBLE, SolidBox};
    let (mut app, _, _) = physical_fixture();
    // Reserve the complete body envelope without introducing another source of
    // contact impulses. This isolates admission/retry policy from Rapier tuning.
    let reserved: Vec<_> = (0..MAX_RUBBLE)
        .map(|i| {
            app.world_mut()
                .spawn((
                    super::super::destruction::Rubble {
                        id: 10_000 + i as u32,
                        cell: 72,
                        boxes: vec![
                            SolidBox {
                                center: Vec3::ZERO,
                                half_size: Vec3::splat(0.01),
                            }
                            .into(),
                        ],
                        remaining: 20.0,
                    },
                    Transform::from_xyz(10.0, 1.0, 10.0),
                ))
                .id()
        })
        .collect();
    undercut_left_panel(&mut app);
    let cuts = app.world().resource::<WorldState>().cover_cuts.clone();
    assert!(
        app.world()
            .resource::<WorldState>()
            .destroyed_cells
            .is_empty()
    );
    for _ in 0..128 {
        assert!(
            !app.world().resource::<WorldState>().support_dirty,
            "unchanged capacity cannot schedule a max-flow solve every fixed tick"
        );
        steps(&mut app, 1);
        assert_eq!(app.world().resource::<WorldState>().cover_cuts, cuts);
    }
    for entity in reserved {
        app.world_mut().despawn(entity);
    }
    // expire() sets this same flag when a real fragment frees its reservation.
    app.world_mut().resource_mut::<WorldState>().support_dirty = true;
    steps(&mut app, 1);
    assert!(
        !app.world()
            .resource::<WorldState>()
            .destroyed_cells
            .is_empty()
    );
    assert!(
        app.world_mut()
            .query::<&super::super::destruction::Rubble>()
            .iter(app.world())
            .count()
            <= MAX_RUBBLE
    );
}
