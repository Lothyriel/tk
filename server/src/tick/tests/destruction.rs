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
    assert_eq!(
        app.world().get::<BarrierVoxel>(entity).unwrap().health,
        100.0
    );
    assert!(app.world().get::<ColliderDisabled>(entity).is_none());
    let state = app.world().resource::<WorldState>();
    assert_eq!(state.cover_cuts.len(), 2);
    assert!(
        state
            .cover_cuts
            .iter()
            .all(|cut| cut.half_size[0] * 2.0 == 0.006 && cut.half_size[1] * 2.0 == 0.006)
    );
    assert!(
        state.impact_marks.is_empty(),
        "an opened bore must not be covered by a floating quad"
    );
}

#[test]
fn destruction_concrete_chips_stop_the_round_and_leave_material_behind() {
    let (mut app, owner, _) = physical_fixture();
    let cell = common::map::barrier_cells().remove(2);
    voxel_shot(&mut app, owner, &cell, 26.0);
    steps(&mut app, 2);
    let entity = voxel_entity(&mut app, cell.id);
    assert_eq!(
        app.world().get::<BarrierVoxel>(entity).unwrap().health,
        74.0
    );
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

fn break_left_panel_base(app: &mut App, owner: Entity) {
    for cell in common::map::barrier_cells().into_iter().filter(|cell| {
        matches!(cell.surface, SurfaceKind::Wood)
            && cell.center[0] < 0.0
            && cell.center[1] - cell.half_size[1] < 0.1001
    }) {
        voxel_shot(app, owner, &cell, 1000.0);
    }
    steps(app, 2);
}

#[test]
fn destruction_unsupported_sections_fall_settle_and_keep_stable_ids_without_player_support() {
    let (mut app, owner, _) = physical_fixture();
    break_left_panel_base(&mut app, owner);
    let pieces: std::collections::BTreeMap<_, _> = app
        .world_mut()
        .query::<(Entity, &super::super::destruction::Rubble)>()
        .iter(app.world())
        .map(|(entity, piece)| (piece.cell, entity))
        .collect();
    assert_eq!(pieces.len(), 32);
    assert_eq!(
        pieces.keys().copied().collect::<Vec<_>>(),
        (72..104).collect::<Vec<_>>()
    );
    let other = voxel_entity(&mut app, 104);
    assert_eq!(
        app.world().get::<BarrierVoxel>(other).unwrap().health,
        160.0
    );
    assert!(app.world().get::<ColliderDisabled>(other).is_none());
    let upper = pieces[&75];
    let start_y = app.world().get::<Transform>(upper).unwrap().translation.y;
    steps(&mut app, 64);
    assert!(
        app.world().get::<Transform>(upper).unwrap().translation.y < start_y - 0.15,
        "unsupported upper section must actually fall"
    );
    let mut reference = std::collections::BTreeMap::new();
    let mut maximum_late_drift = 0.0_f32;
    let mut minimum_y = f32::MAX;
    let cells = common::map::barrier_cells();
    for tick in 0..768 {
        steps(&mut app, 1);
        for (entity, piece, transform, velocity, groups) in app
            .world_mut()
            .query::<(
                Entity,
                &super::super::destruction::Rubble,
                &Transform,
                &Velocity,
                &CollisionGroups,
            )>()
            .iter(app.world())
        {
            assert!(transform.translation.is_finite() && transform.rotation.is_finite());
            assert!(velocity.linear.is_finite() && velocity.angular.is_finite());
            assert_eq!(pieces[&piece.cell], entity);
            assert_eq!(groups.memberships, Group::GROUP_4);
            assert!((groups.filters & Group::GROUP_3).is_empty());
            let half = Vec3::from(cells[piece.cell as usize].half_size) * transform.scale;
            for x in [-1.0, 1.0] {
                for y in [-1.0, 1.0] {
                    for z in [-1.0, 1.0] {
                        let corner = transform.translation
                            + transform.rotation * (half * Vec3::new(x, y, z));
                        minimum_y = minimum_y.min(corner.y);
                        assert!(corner.y > 0.075, "rubble escaped the floor: {}", corner.y);
                    }
                }
            }
            if tick == 512 {
                reference.insert(entity, transform.translation);
            }
            if tick > 512 {
                maximum_late_drift =
                    maximum_late_drift.max(transform.translation.distance(reference[&entity]));
            }
        }
    }
    eprintln!("rubble min_corner_y={minimum_y:.6} late_drift_m={maximum_late_drift:.6}");
    assert!(
        maximum_late_drift < 0.03,
        "late rubble drift: {maximum_late_drift}"
    );
    assert_eq!(
        app.world_mut()
            .query::<&super::super::destruction::Rubble>()
            .iter(app.world())
            .count(),
        32
    );
    app.world_mut()
        .get_mut::<Transform>(owner)
        .unwrap()
        .translation = Vec3::new(-7.0, 1.05, -5.0);
    held_steps(&mut app, owner, 128);
    let height = app.world().get::<Transform>(owner).unwrap().translation.y;
    assert!(
        (0.95..1.08).contains(&height),
        "rubble must not become controller support: {height}"
    );
}

#[test]
fn destruction_rubble_is_bounded_and_expires_without_restoring_broken_cover() {
    let (mut app, _, _) = physical_fixture();
    for mut voxel in app
        .world_mut()
        .query::<&mut BarrierVoxel>()
        .iter_mut(app.world_mut())
    {
        voxel.health = 0.0;
    }
    {
        let mut state = app.world_mut().resource_mut::<WorldState>();
        state.destroyed_cells = (0..176).collect();
        state.support_dirty = true;
    }
    steps(&mut app, 1);
    assert_eq!(
        app.world_mut()
            .query::<&super::super::destruction::Rubble>()
            .iter(app.world())
            .count(),
        64
    );
    for _ in 0..2561 {
        steps(&mut app, 1);
        assert!(
            app.world_mut()
                .query::<&super::super::destruction::Rubble>()
                .iter(app.world())
                .count()
                <= 64
        );
    }
    assert_eq!(
        app.world_mut()
            .query::<&super::super::destruction::Rubble>()
            .iter(app.world())
            .count(),
        0
    );
    assert_eq!(
        app.world().resource::<WorldState>().destroyed_cells.len(),
        176
    );
    assert!(
        app.world_mut()
            .query::<&BarrierVoxel>()
            .iter(app.world())
            .all(|voxel| voxel.released && voxel.health == 0.0)
    );
}

#[test]
fn destruction_rubble_late_join_reconstructs_actual_poses_and_reset_restores_cover() {
    use common::map::destruction::{CHANNEL, CoverSnapshot};
    let (mut app, owner, _) = physical_fixture();
    let cell = common::map::barrier_cells().remove(75);
    voxel_shot(&mut app, owner, &cell, 26.0);
    steps(&mut app, 2);
    break_left_panel_base(&mut app, owner);
    let mut peer = bevy_renet2::prelude::RenetClient::new(data::renet_config(), false);
    peer.set_connected();
    app.world_mut()
        .resource_mut::<RenetServer>()
        .add_connection(999, false);
    let mut seen = 0;
    for tick in 0..24 {
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
            assert_eq!(snapshot.rubble.len(), 32);
            assert_eq!(
                snapshot.cuts,
                app.world().resource::<WorldState>().cover_cuts
            );
            assert_eq!(
                snapshot.removed,
                common::map::destroyed_mask(&app.world().resource::<WorldState>().destroyed_cells)
            );
            let actual: std::collections::BTreeMap<_, _> = app
                .world_mut()
                .query::<(&super::super::destruction::Rubble, &Transform)>()
                .iter(app.world())
                .map(|(piece, t)| (piece.cell, *t))
                .collect();
            for piece in snapshot.rubble {
                let transform = actual[&piece.cell];
                assert_eq!(piece.position, transform.translation.to_array());
                assert_eq!(piece.rotation, transform.rotation.to_array());
                assert_eq!(piece.scale, transform.scale.x);
            }
            seen += 1;
        }
    }
    assert!(
        seen >= 3,
        "periodic full state must recover the discarded updates"
    );
    app.world_mut().resource_mut::<RaidState>().phase = RaidPhase::Debrief;
    ready(&mut app, owner);
    steps(&mut app, 3);
    assert_eq!(app.world().resource::<RaidState>().phase, RaidPhase::Lobby);
    assert_eq!(
        app.world_mut()
            .query::<&super::super::destruction::Rubble>()
            .iter(app.world())
            .count(),
        0
    );
    assert!(app.world().resource::<WorldState>().cover_cuts.is_empty());
    assert!(
        app.world()
            .resource::<WorldState>()
            .destroyed_cells
            .is_empty()
    );
    for (entity, voxel) in app
        .world_mut()
        .query::<(Entity, &BarrierVoxel)>()
        .iter(app.world())
    {
        assert_eq!(voxel.health, voxel.max_health);
        assert!(!voxel.released);
        assert!(app.world().get::<ColliderDisabled>(entity).is_none());
    }
}

#[test]
fn destruction_physical_rubble_stops_rounds_without_floating_marks() {
    let (mut app, owner, _) = physical_fixture();
    let cell = common::map::barrier_cells().remove(2);
    voxel_shot(&mut app, owner, &cell, 1000.0);
    steps(&mut app, 2);
    let neighbor = voxel_entity(&mut app, 1);
    voxel_shot(&mut app, owner, &cell, 40.0);
    steps(&mut app, 2);
    assert_eq!(
        app.world().get::<BarrierVoxel>(neighbor).unwrap().health,
        100.0
    );
    assert_eq!(
        app.world_mut()
            .query::<&Projectile>()
            .iter(app.world())
            .count(),
        0
    );
    assert!(app.world().resource::<WorldState>().impact_marks.is_empty());
    let rubble: Vec<_> = app
        .world_mut()
        .query_filtered::<Entity, With<super::super::destruction::Rubble>>()
        .iter(app.world())
        .collect();
    assert_eq!(rubble.len(), 1);
    for entity in rubble {
        app.world_mut().despawn(entity);
    }
    steps(&mut app, 2);
    voxel_shot(&mut app, owner, &cell, 40.0);
    steps(&mut app, 2);
    assert_eq!(
        app.world().get::<BarrierVoxel>(neighbor).unwrap().health,
        60.0
    );
}

fn damage_at(
    In((owner, id, point)): In<(Entity, u16, Vec3)>,
    mut commands: Commands,
    mut state: ResMut<WorldState>,
    mut voxels: Query<(Entity, &mut BarrierVoxel)>,
) {
    let (entity, mut voxel) = voxels.iter_mut().find(|(_, cell)| cell.id == id).unwrap();
    let projectile = Projectile {
        id: 9001,
        weapon: WeaponId::M4A1,
        velocity: -Vec3::Z * 100.0,
        damage: 0.001,
        penetration: 30.0,
        lifetime: 3.0,
        owner_entity: owner,
        penetrated: Vec::new(),
    };
    super::super::destruction::damage(
        &mut commands,
        &mut state,
        &mut voxel,
        super::super::destruction::Hit {
            entity,
            point,
            normal: Vec3::Z,
        },
        &projectile,
    );
}

#[test]
fn destruction_sparse_geometry_budgets_fracture_instead_of_restoring_old_damage() {
    use bevy::ecs::system::RunSystemOnce;
    use common::map::destruction::{MAX_CUTS, MAX_SOLIDS, solids};
    let (mut app, owner, _) = physical_fixture();
    let cell = common::map::barrier_cells().remove(72);
    let entity = voxel_entity(&mut app, cell.id);
    for index in 0..128 {
        let point = Vec3::from(cell.center)
            + Vec3::new(
                -0.2 + (index % 16) as f32 * 0.025,
                -0.15 + (index / 16) as f32 * 0.035,
                cell.half_size[2],
            );
        app.world_mut()
            .run_system_once_with(damage_at, (owner, cell.id, point))
            .unwrap();
        if app.world().get::<BarrierVoxel>(entity).unwrap().health == 0.0 {
            break;
        }
    }
    assert_eq!(app.world().get::<BarrierVoxel>(entity).unwrap().health, 0.0);
    assert!(app.world().get::<ColliderDisabled>(entity).is_some());
    let cuts = &app.world().resource::<WorldState>().cover_cuts;
    assert!(
        cuts.len() < MAX_CUTS,
        "exercise the per-cell solids bound before the global bound"
    );
    assert!(solids(&cell, cuts).len() <= MAX_SOLIDS);
    assert!(!cuts.is_empty(), "existing damage must remain recorded");

    let (mut app, owner, _) = physical_fixture();
    let cells = common::map::barrier_cells();
    for cell in &cells[..MAX_CUTS] {
        let point = Vec3::from(cell.center) + Vec3::Z * cell.half_size[2];
        app.world_mut()
            .run_system_once_with(damage_at, (owner, cell.id, point))
            .unwrap();
    }
    let original = app.world().resource::<WorldState>().cover_cuts.clone();
    assert_eq!(original.len(), 128);
    let next = &cells[144];
    app.world_mut()
        .run_system_once_with(
            damage_at,
            (
                owner,
                next.id,
                Vec3::from(next.center) + Vec3::Z * next.half_size[2],
            ),
        )
        .unwrap();
    assert_eq!(app.world().resource::<WorldState>().cover_cuts, original);
    assert!(
        app.world()
            .resource::<WorldState>()
            .destroyed_cells
            .contains(&next.id)
    );
}
