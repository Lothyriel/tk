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
