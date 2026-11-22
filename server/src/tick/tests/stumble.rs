use super::*;
use common::humanoid::{PhysicalMode, PoseState};

fn course(height: Option<f32>, run: bool) -> (App, Entity, Vec3) {
    let (mut app, actor, other) = physical_fixture();
    app.world_mut()
        .get_mut::<Transform>(other)
        .unwrap()
        .translation
        .x += 3.0;
    let origin = app.world().get::<Transform>(actor).unwrap().translation;
    if let Some(height) = height {
        app.world_mut().spawn((
            SurfaceKind::Concrete,
            Collider::cuboid(1.0, height / 2.0, 0.45),
            Transform::from_xyz(origin.x, height / 2.0, origin.z - 2.0),
        ));
    }
    steps(&mut app, 4);
    let mut input = app.world_mut().get_mut::<ClientInput>(actor).unwrap();
    input.forward = true;
    input.run = run;
    (app, actor, origin)
}

fn sample(app: &mut App, actor: Entity, count: usize) -> (usize, usize) {
    let mut episodes = 0;
    let mut reaction_ticks = 0;
    for _ in 0..count {
        held_steps(app, actor, 1);
        let pose = app.world().get::<PoseState>(actor).unwrap();
        assert!(matches!(
            pose.mode,
            PhysicalMode::Standing | PhysicalMode::Stumbling
        ));
        if pose.mode == PhysicalMode::Stumbling {
            episodes += usize::from(pose.mode_time == 0.0);
            reaction_ticks += 1;
        }
    }
    (episodes, reaction_ticks)
}

#[test]
fn obstacle_stumble_course_distinguishes_sprint_contact_from_step_walk_and_wall() {
    for (height, run, expected, crosses) in [
        (None, true, 0, true),
        (Some(0.18), false, 0, true),
        (Some(0.18), true, 0, true),
        (Some(0.44), false, 0, false),
        (Some(0.44), true, 1, false),
        (Some(1.8), true, 0, false),
    ] {
        let (mut app, actor, origin) = course(height, run);
        let (episodes, reaction_ticks) = sample(&mut app, actor, 150);
        assert_eq!(episodes, expected, "height={height:?}, run={run}");
        if expected == 1 {
            assert!((80..=86).contains(&reaction_ticks), "{reaction_ticks}");
        }
        let travelled = origin.z - app.world().get::<Transform>(actor).unwrap().translation.z;
        if crosses {
            assert!(travelled > 2.5, "height={height:?}, run={run}: {travelled}");
        } else {
            assert!(
                (0.9..1.55).contains(&travelled),
                "height={height:?}, run={run}: {travelled}"
            );
        }
        assert_eq!(
            app.world().get::<PoseState>(actor).unwrap().mode,
            PhysicalMode::Standing
        );
    }
}

#[test]
fn obstacle_stumble_rearms_only_after_separating_and_can_resume_running() {
    let (mut app, actor, origin) = course(Some(0.44), true);
    assert_eq!(sample(&mut app, actor, 150).0, 1);
    {
        let mut input = app.world_mut().get_mut::<ClientInput>(actor).unwrap();
        input.forward = false;
        input.run = false;
    }
    assert_eq!(sample(&mut app, actor, 90).0, 0);
    {
        let mut input = app.world_mut().get_mut::<ClientInput>(actor).unwrap();
        input.forward = true;
        input.run = true;
    }
    assert_eq!(sample(&mut app, actor, 135).0, 0);
    {
        let mut input = app.world_mut().get_mut::<ClientInput>(actor).unwrap();
        input.forward = false;
        input.backward = true;
    }
    assert_eq!(sample(&mut app, actor, 45).0, 0);
    assert!(app.world().get::<Transform>(actor).unwrap().translation.z > origin.z - 0.5);
    {
        let mut input = app.world_mut().get_mut::<ClientInput>(actor).unwrap();
        input.forward = true;
        input.backward = false;
    }
    assert_eq!(sample(&mut app, actor, 145).0, 1);
}

#[test]
fn obstacle_stumble_pose_roundtrip_keeps_the_confirmed_reaction_and_recovers() {
    use common::humanoid::{Segment, evaluate_pose};
    let (mut app, actor, _) = course(Some(0.44), true);
    assert_eq!(sample(&mut app, actor, 82).0, 1);
    let pose = app.world().get::<PoseState>(actor).unwrap();
    assert_eq!(pose.mode, PhysicalMode::Stumbling);
    let replicated = data::decode::<PoseState>(&data::encode(pose));
    let reacting = evaluate_pose(&replicated, 1.0, 1.0);
    let standing = evaluate_pose(
        &PoseState {
            mode: PhysicalMode::Standing,
            ..replicated
        },
        1.0,
        1.0,
    );
    assert!(
        reacting[Segment::Thorax.index()].position[2]
            < standing[Segment::Thorax.index()].position[2] - 0.045
    );
    sample(&mut app, actor, 110);
    assert_eq!(
        app.world().get::<PoseState>(actor).unwrap().mode,
        PhysicalMode::Standing
    );
}
