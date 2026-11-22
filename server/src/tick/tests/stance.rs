use super::*;
use bevy::ecs::system::RunSystemOnce;
use common::humanoid::PoseState;
use common::stance::Stance;

fn destination_clear(app: &mut App, actor: Entity, centre: Vec3, stance: Stance, yaw: f32) -> bool {
    app.world_mut()
        .run_system_once(move |context: ReadRapierContext| {
            can_occupy_stance(
                actor,
                &context.single().unwrap(),
                centre,
                Quat::from_rotation_y(yaw),
                stance,
            )
        })
        .unwrap()
}

#[test]
fn voluntary_prone_candidate_preserves_floor_and_checks_exit_headroom() {
    let (mut app, actor, _) = physical_fixture();
    let stand = app.world().get::<Transform>(actor).unwrap().translation;
    let crouch = Stance::Crouched.centre_from(Stance::Standing, stand);
    let prone = Stance::Prone.centre_from(Stance::Crouched, crouch);
    let from_stand = Stance::Prone.centre_from(Stance::Standing, stand);
    assert!(prone.distance(from_stand) < 1e-6);
    assert!((prone.y - Stance::Prone.half_height() - (stand.y - 0.90)).abs() < 1e-6);
    assert!(
        (prone.y + Stance::Prone.eye_above_root()) < crouch.y + Stance::Crouched.eye_above_root()
    );
    assert!(destination_clear(
        &mut app,
        actor,
        prone,
        Stance::Prone,
        0.0
    ));
    assert!(destination_clear(
        &mut app,
        actor,
        stand,
        Stance::Standing,
        0.0
    ));

    // A ceiling clear of the prone envelope but below either upright option.
    let ceiling = app
        .world_mut()
        .spawn((
            SurfaceKind::Concrete,
            Collider::cuboid(1.0, 0.10, 1.0),
            Transform::from_xyz(prone.x, prone.y + 0.55, prone.z),
        ))
        .id();
    steps(&mut app, 4);
    assert!(destination_clear(
        &mut app,
        actor,
        prone,
        Stance::Prone,
        0.0
    ));
    for target in [Stance::Crouched, Stance::Standing] {
        let centre = target.centre_from(Stance::Prone, prone);
        assert!(!destination_clear(&mut app, actor, centre, target, 0.0));
    }

    app.world_mut().entity_mut(ceiling).despawn();
    steps(&mut app, 4);
    for target in [Stance::Crouched, Stance::Standing] {
        let centre = target.centre_from(Stance::Prone, prone);
        assert!(destination_clear(&mut app, actor, centre, target, 0.0));
    }
}

#[test]
fn voluntary_prone_candidate_checks_length_in_facing_direction() {
    let (mut app, actor, _) = physical_fixture();
    let stand = app.world().get::<Transform>(actor).unwrap().translation;
    let prone = Stance::Prone.centre_from(Stance::Standing, stand);
    app.world_mut().spawn((
        SurfaceKind::Concrete,
        Collider::cuboid(1.0, 0.50, 0.07),
        Transform::from_xyz(prone.x, prone.y + 0.15, prone.z + 0.64),
    ));
    steps(&mut app, 4);
    assert!(destination_clear(
        &mut app,
        actor,
        stand,
        Stance::Standing,
        0.0
    ));
    assert!(!destination_clear(
        &mut app,
        actor,
        prone,
        Stance::Prone,
        0.0
    ));
    assert!(destination_clear(
        &mut app,
        actor,
        prone,
        Stance::Prone,
        std::f32::consts::FRAC_PI_2
    ));
}

#[test]
fn voluntary_prone_toggle_crawls_aims_and_exits_only_on_fresh_clear_edge() {
    let (mut app, actor, peer) = physical_fixture();
    app.world_mut()
        .get_mut::<Transform>(peer)
        .unwrap()
        .translation
        .x -= 4.0;
    let start = app.world().get::<Transform>(actor).unwrap().translation;
    let toggle = |app: &mut App, sequence| {
        app.world_mut()
            .get_mut::<ClientInput>(actor)
            .unwrap()
            .prone_sequence = sequence;
        held_steps(app, actor, 1);
    };
    let prone = |app: &App| app.world().get::<PoseState>(actor).unwrap().prone;
    toggle(&mut app, 1);
    assert!(prone(&app));
    toggle(&mut app, 2);
    assert!(
        prone(&app),
        "rapid reversal must consume the edge without exiting"
    );
    {
        let mut input = app.world_mut().get_mut::<ClientInput>(actor).unwrap();
        input.run = true;
        input.jump = true;
        input.aim = true;
        input.forward = true;
    }
    held_steps(&mut app, actor, 96);
    assert!(prone(&app), "the throttled edge must not replay");
    let position = app.world().get::<Transform>(actor).unwrap().translation;
    assert!(
        (0.35..0.66).contains(&(start.z - position.z)),
        "{start:?} -> {position:?}"
    );
    assert!((position.y - start.y + 0.67).abs() < 0.02);
    let pose = app.world().get::<PoseState>(actor).unwrap();
    assert!(pose.grounded && pose.ads > 0.99, "{pose:?}");
    {
        let mut input = app.world_mut().get_mut::<ClientInput>(actor).unwrap();
        input.forward = false;
        input.fire = true;
        input.fire_pressed_sequence = 1;
    }
    held_steps(&mut app, actor, 1);
    assert_eq!(
        app.world().get::<Arsenal>(actor).unwrap().recoil.impulses,
        1
    );
    {
        let mut input = app.world_mut().get_mut::<ClientInput>(actor).unwrap();
        input.fire = false;
        input.reload_sequence = 1;
    }
    held_steps(&mut app, actor, 8);
    assert!(
        app.world()
            .get::<Arsenal>(actor)
            .unwrap()
            .current()
            .mechanics()
            .snapshot()
            .reloading()
    );
    let position = app.world().get::<Transform>(actor).unwrap().translation;
    let ceiling = app
        .world_mut()
        .spawn((
            SurfaceKind::Concrete,
            Collider::cuboid(1.0, 0.1, 1.0),
            Transform::from_translation(position + Vec3::Y * 0.55),
        ))
        .id();
    held_steps(&mut app, actor, 4);
    toggle(&mut app, 3);
    assert!(prone(&app), "headroom blocks exit");
    app.world_mut().entity_mut(ceiling).despawn();
    held_steps(&mut app, actor, 8);
    assert!(prone(&app), "a previously rejected edge must not replay");
    toggle(&mut app, 2);
    assert!(prone(&app), "older edge cannot exit");
    app.world_mut()
        .get_mut::<ClientInput>(actor)
        .unwrap()
        .crouch = true;
    toggle(&mut app, 4);
    assert!(!prone(&app));
    assert!(app.world().get::<MovementState>(actor).unwrap().crouched);
    held_steps(&mut app, actor, 8);
    assert!(
        app.world().get::<PoseState>(actor).unwrap().grounded,
        "jump held through exit must not replay"
    );
}

#[test]
fn voluntary_prone_yields_to_involuntary_fall() {
    let (mut app, actor, _) = physical_fixture();
    app.world_mut()
        .get_mut::<ClientInput>(actor)
        .unwrap()
        .prone_sequence = 1;
    held_steps(&mut app, actor, 1);
    assert!(app.world().get::<PoseState>(actor).unwrap().prone);
    app.world_mut().get_mut::<PoseState>(actor).unwrap().impulse = [3.0, 0.0, 0.0];
    held_steps(&mut app, actor, 8);
    let pose = app.world().get::<PoseState>(actor).unwrap();
    assert!(!pose.prone);
    assert_eq!(pose.mode, common::humanoid::PhysicalMode::Fallen);
    assert!(pose.physical.is_some());
}
