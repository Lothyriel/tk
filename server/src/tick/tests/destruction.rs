use super::*;

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
    }
    voxel_shot(&mut app, owner, &cell, 10.0);
    steps(&mut app, 2);
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
    voxel_shot(&mut app, owner, &cell, 10.0);
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
