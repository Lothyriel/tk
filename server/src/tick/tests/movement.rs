use super::*;
use common::humanoid::{PhysicalMode, PoseState};

fn speed(app: &App, actor: Entity) -> f32 {
    let v = app.world().get::<MovementState>(actor).unwrap().velocity;
    Vec2::new(v.x, v.z).length()
}

#[test]
fn movement_sprint_builds_and_brakes_with_measurable_momentum() {
    let (mut app, actor, other) = physical_fixture();
    app.world_mut()
        .get_mut::<Transform>(other)
        .unwrap()
        .translation
        .x += 4.0;
    {
        let mut input = app.world_mut().get_mut::<ClientInput>(actor).unwrap();
        input.forward = true;
        input.run = true;
    }
    let mut reached = None;
    for tick in 1..=128 {
        held_steps(&mut app, actor, 1);
        if speed(&app, actor) >= 4.95 {
            reached.get_or_insert(tick as f32 / 128.0);
        }
    }
    let reached = reached.expect("must reach sprint speed");
    assert!((0.45..=0.70).contains(&reached), "90% sprint at {reached}s");
    let start = app.world().get::<Transform>(actor).unwrap().translation;
    {
        let mut input = app.world_mut().get_mut::<ClientInput>(actor).unwrap();
        input.forward = false;
        input.run = false;
    }
    let mut stopped = None;
    for tick in 1..=128 {
        held_steps(&mut app, actor, 1);
        if speed(&app, actor) < 0.1 {
            stopped = Some(tick as f32 / 128.0);
            break;
        }
    }
    let stopped = stopped.expect("must stop");
    let distance = start.distance(app.world().get::<Transform>(actor).unwrap().translation);
    assert!((0.30..=0.55).contains(&stopped), "braking {stopped}s");
    assert!((0.5..=1.2).contains(&distance), "braking {distance}m");
}

#[test]
fn movement_obstacle_jumps_do_not_mistake_edge_contact_for_loss_of_balance() {
    for (height, gap, run) in [
        (0.0, 0.0, false),
        (0.18, 0.05, true),
        (0.44, 0.05, false),
        (0.44, 0.05, true),
        (0.65, 0.20, true),
        (1.8, 0.05, true),
    ] {
        let (mut app, actor, other) = physical_fixture();
        app.world_mut()
            .get_mut::<Transform>(other)
            .unwrap()
            .translation
            .x += 4.0;
        let origin = app.world().get::<Transform>(actor).unwrap().translation;
        if height > 0.0 {
            app.world_mut().spawn((
                SurfaceKind::Concrete,
                Collider::cuboid(1.0, height / 2.0, 0.3),
                Transform::from_xyz(origin.x, 0.1 + height / 2.0, origin.z - 0.65 - gap),
            ));
        }
        steps(&mut app, 4);
        {
            let mut input = app.world_mut().get_mut::<ClientInput>(actor).unwrap();
            input.jump = true;
            input.forward = false; // Jump first; adding forward intent in air cannot start a vault.
            input.run = run;
        }
        held_steps(&mut app, actor, 1);
        app.world_mut()
            .get_mut::<ClientInput>(actor)
            .unwrap()
            .forward = true;
        let mut airborne = false;
        for tick in 0..300 {
            // Release travel after the obstacle so the fixture stays away from map cover.
            if tick == 100 {
                app.world_mut()
                    .get_mut::<ClientInput>(actor)
                    .unwrap()
                    .forward = false;
            }
            held_steps(&mut app, actor, 1);
            let root = app.world().get::<Transform>(actor).unwrap();
            let movement = app.world().get::<MovementState>(actor).unwrap();
            let pose = app.world().get::<PoseState>(actor).unwrap();
            airborne |= !pose.grounded;
            assert!(root.translation.is_finite() && movement.velocity.is_finite());
            assert!(
                matches!(pose.mode, PhysicalMode::Standing | PhysicalMode::Stumbling),
                "height={height} gap={gap} run={run} tick={tick} root={:?} grounded={} support={}/{} mode={:?}",
                root.translation,
                pose.grounded,
                pose.left_support,
                pose.right_support,
                pose.mode
            );
            assert!(root.translation.y >= 0.95, "floor escape {root:?}");
        }
        assert!(airborne);
    }
}

fn vault_course(height: f32, depth: f32) -> (App, Entity, Entity, Vec3) {
    let (mut app, actor, other) = physical_fixture();
    app.world_mut()
        .get_mut::<Transform>(other)
        .unwrap()
        .translation
        .x += 4.0;
    let origin = app.world().get::<Transform>(actor).unwrap().translation;
    let obstacle = app
        .world_mut()
        .spawn((
            SurfaceKind::Concrete,
            Collider::cuboid(1.0, height / 2.0, depth / 2.0),
            Transform::from_xyz(origin.x, 0.1 + height / 2.0, origin.z - 0.45 - depth / 2.0),
        ))
        .id();
    steps(&mut app, 4);
    (app, actor, obstacle, origin)
}

fn request_vault(app: &mut App, actor: Entity) {
    let mut input = app.world_mut().get_mut::<ClientInput>(actor).unwrap();
    input.forward = true;
    input.jump = true;
}

#[test]
fn movement_jump_press_during_vault_is_consumed_until_a_fresh_grounded_press() {
    let (mut app, actor, _, _) = vault_course(0.65, 0.6);
    request_vault(&mut app, actor);
    held_steps(&mut app, actor, 8);
    app.world_mut().get_mut::<ClientInput>(actor).unwrap().jump = false;
    held_steps(&mut app, actor, 1);
    app.world_mut().get_mut::<ClientInput>(actor).unwrap().jump = true;
    app.world_mut()
        .get_mut::<ClientInput>(actor)
        .unwrap()
        .forward = false;
    let mut landed = false;
    for _ in 0..240 {
        held_steps(&mut app, actor, 1);
        let pose = app.world().get::<PoseState>(actor).unwrap();
        if pose.vault == 0 && pose.grounded {
            landed = true;
        }
        if landed {
            assert!(
                pose.grounded && pose.vault == 0,
                "rejected airborne press replayed"
            );
        }
    }
    assert!(landed);
    app.world_mut().get_mut::<ClientInput>(actor).unwrap().jump = false;
    held_steps(&mut app, actor, 1);
    app.world_mut().get_mut::<ClientInput>(actor).unwrap().jump = true;
    held_steps(&mut app, actor, 1);
    assert!(
        app.world().get::<MovementState>(actor).unwrap().velocity.y > 6.0,
        "fresh grounded press must still jump"
    );
}

#[test]
fn movement_diagonals_obey_the_directional_envelope_without_a_speed_bonus() {
    for (backward, low, high) in [(false, 3.45, 3.60), (true, 2.45, 2.60)] {
        let (mut app, actor, other) = physical_fixture();
        app.world_mut()
            .get_mut::<Transform>(other)
            .unwrap()
            .translation
            .x += 5.0;
        {
            let mut input = app.world_mut().get_mut::<ClientInput>(actor).unwrap();
            input.forward = !backward;
            input.backward = backward;
            input.right = true;
            input.run = true;
        }
        held_steps(&mut app, actor, 120);
        assert!(
            (low..=high).contains(&speed(&app, actor)),
            "diagonal speed {}",
            speed(&app, actor)
        );
    }
}

#[test]
fn movement_vault_clears_low_and_high_cover_once_and_lands_under_normal_gravity() {
    for (height, depth) in [(0.30, 0.3), (0.65, 0.6), (1.0, 1.0)] {
        let (mut app, actor, _, origin) = vault_course(height, depth);
        request_vault(&mut app, actor);
        let mut active = false;
        let mut episodes = 0;
        let mut landed = None;
        let mut previous = origin;
        for tick in 0..280 {
            held_steps(&mut app, actor, 1);
            let pose = app.world().get::<PoseState>(actor).unwrap();
            let root = app.world().get::<Transform>(actor).unwrap().translation;
            if pose.vault != 0 && !active {
                episodes += 1;
            }
            active = pose.vault != 0;
            assert!(
                root.is_finite() && root.distance(previous) < 0.10,
                "tick discontinuity {root:?} {previous:?}"
            );
            assert!(
                matches!(pose.mode, PhysicalMode::Standing | PhysicalMode::Stumbling),
                "height={height} tick={tick} {pose:?}"
            );
            assert!(root.y >= 0.99, "floor penetration {root:?}");
            if root.z < origin.z - 0.45 && root.z > origin.z - 0.45 - depth {
                assert!(
                    root.y - 0.90 >= 0.1 + height - 0.02,
                    "capsule crossed cover below its top: {root:?}"
                );
            }
            if episodes > 0 && !active && pose.grounded && root.z < origin.z - 0.45 - depth - 0.35 {
                landed.get_or_insert(tick as f32 / 128.0);
            }
            if tick == 150 {
                app.world_mut()
                    .get_mut::<ClientInput>(actor)
                    .unwrap()
                    .forward = false;
            }
            previous = root;
        }
        assert_eq!(episodes, 1, "height={height} depth={depth}");
        assert!(
            landed.is_some_and(|time| time < 1.5),
            "height={height} landing={landed:?}"
        );
        let pose = app.world().get::<PoseState>(actor).unwrap();
        assert!(pose.grounded && pose.vault == 0);
    }
}

#[test]
fn movement_directional_limits_and_shift_handling_agree() {
    for (backward, sideways, run, expected) in [
        (false, false, true, 5.5),
        (false, false, false, 3.25),
        (true, false, false, 2.3),
        (true, false, true, 2.3),
        (false, true, false, 2.8),
        (false, true, true, 2.8),
    ] {
        let (mut app, actor, other) = physical_fixture();
        app.world_mut()
            .get_mut::<Transform>(other)
            .unwrap()
            .translation
            .x += 5.0;
        {
            let mut input = app.world_mut().get_mut::<ClientInput>(actor).unwrap();
            input.forward = !backward && !sideways;
            input.backward = backward;
            input.right = sideways;
            input.run = run;
            input.aim = true;
        }
        held_steps(&mut app, actor, 120);
        let pose = app.world().get::<PoseState>(actor).unwrap();
        assert!(
            (speed(&app, actor) - expected).abs() < expected * 0.05,
            "back={backward} side={sideways} run={run} actual={}",
            speed(&app, actor)
        );
        assert_eq!(pose.sprinting, run && !backward && !sideways);
        if backward || sideways {
            assert!(
                pose.ads > 0.98,
                "Shift must not suppress ADS while backpedaling/strafing"
            );
        }
    }
}

#[test]
fn movement_reversal_yaw_and_wall_contacts_preserve_only_achieved_momentum() {
    let (mut app, actor, other) = physical_fixture();
    app.world_mut()
        .get_mut::<Transform>(other)
        .unwrap()
        .translation
        .x += 5.0;
    {
        let mut input = app.world_mut().get_mut::<ClientInput>(actor).unwrap();
        input.forward = true;
        input.run = true;
    }
    held_steps(&mut app, actor, 90);
    let before = app.world().get::<MovementState>(actor).unwrap().velocity;
    app.world_mut()
        .get_mut::<ClientInput>(actor)
        .unwrap()
        .camera
        .yaw = std::f32::consts::PI;
    held_steps(&mut app, actor, 1);
    let after = app.world().get::<MovementState>(actor).unwrap().velocity;
    assert!(
        after.z < -4.5 && after.distance(before) < 0.12,
        "instant yaw reversal {before:?}->{after:?}"
    );
    held_steps(&mut app, actor, 128);
    assert!(app.world().get::<MovementState>(actor).unwrap().velocity.z > 4.5);

    let (mut app, actor, _, _) = vault_course(1.8, 0.8);
    {
        let mut input = app.world_mut().get_mut::<ClientInput>(actor).unwrap();
        input.forward = true;
        input.run = true;
    }
    held_steps(&mut app, actor, 150);
    assert!(
        speed(&app, actor) < 0.12,
        "blocked speed {}",
        speed(&app, actor)
    );
    {
        let mut input = app.world_mut().get_mut::<ClientInput>(actor).unwrap();
        input.forward = false;
        input.run = false;
        input.backward = true;
    }
    held_steps(&mut app, actor, 1);
    assert!(speed(&app, actor) < 0.12);
}

#[test]
fn movement_vault_rejects_geometry_and_control_states_that_cannot_traverse() {
    for scenario in [
        "step", "wall", "wide", "ceiling", "landing", "pit", "dynamic", "crouch", "prone", "weak",
        "stale", "inactive", "dead",
    ] {
        let height = if scenario == "step" {
            0.18
        } else if scenario == "wall" {
            1.8
        } else {
            0.65
        };
        let depth = if scenario == "wide" { 1.3 } else { 0.6 };
        let (mut app, actor, obstacle, origin) = vault_course(height, depth);
        match scenario {
            "ceiling" => {
                app.world_mut().spawn((
                    Collider::cuboid(1.0, 0.1, 1.5),
                    Transform::from_xyz(origin.x, 2.15, origin.z - 0.5),
                ));
            }
            "landing" => {
                app.world_mut().spawn((
                    Collider::cuboid(1.0, 1.5, 0.1),
                    Transform::from_xyz(origin.x, 1.5, origin.z - 1.45),
                ));
            }
            "pit" => {
                app.world_mut()
                    .get_mut::<Transform>(actor)
                    .unwrap()
                    .translation
                    .y += 1.5;
                app.world_mut()
                    .get_mut::<Transform>(obstacle)
                    .unwrap()
                    .translation
                    .y += 1.5;
                app.world_mut().spawn((
                    Collider::cuboid(1.0, 0.75, 0.3),
                    Transform::from_xyz(origin.x, 0.85, origin.z),
                ));
            }
            "dynamic" => {
                let transform = *app.world().get::<Transform>(obstacle).unwrap();
                app.world_mut().entity_mut(obstacle).despawn();
                app.world_mut().spawn((
                    RigidBody::Dynamic,
                    GravityScale(0.0),
                    SurfaceKind::Concrete,
                    Collider::cuboid(1.0, height / 2.0, depth / 2.0),
                    transform,
                ));
            }
            "crouch" => {
                app.world_mut()
                    .get_mut::<ClientInput>(actor)
                    .unwrap()
                    .crouch = true;
            }
            "prone" => {
                // Leave room for the horizontal prone envelope; the lip is
                // still within the standing vault probe's one-metre reach.
                app.world_mut()
                    .get_mut::<Transform>(obstacle)
                    .unwrap()
                    .translation
                    .z -= 0.5;
                // Let Rapier see the moved obstacle before the posture edge.
                steps(&mut app, 2);
                app.world_mut()
                    .get_mut::<ClientInput>(actor)
                    .unwrap()
                    .prone_sequence = 1;
            }
            "weak" => {
                app.world_mut()
                    .get_mut::<BodyHealth>(actor)
                    .unwrap()
                    .anatomy
                    .vigor = 0.0;
            }
            "inactive" => {
                app.world_mut()
                    .get_mut::<Participant>(actor)
                    .unwrap()
                    .status = RaidStatus::Extracted;
            }
            "dead" => {
                app.world_mut().get_mut::<BodyHealth>(actor).unwrap().parts[0] = 0.0;
            }
            _ => {}
        }
        held_steps(&mut app, actor, 4);
        if scenario == "prone" {
            assert!(app.world().get::<PoseState>(actor).unwrap().prone);
        }
        request_vault(&mut app, actor);
        if scenario == "stale" {
            app.world_mut()
                .get_mut::<LastInput>(actor)
                .unwrap()
                .received_at = -100.0;
            steps(&mut app, 1);
        } else {
            held_steps(&mut app, actor, 1);
        }
        assert!(
            app.world().get::<PoseState>(actor).unwrap().vault == 0,
            "accepted {scenario}"
        );
        assert!(
            app.world()
                .get::<super::super::traversal::State>(actor)
                .unwrap()
                .motion
                .is_none(),
            "accepted state {scenario}"
        );
        if matches!(scenario, "weak" | "prone" | "stale" | "inactive" | "dead") {
            assert!(
                app.world().get::<MovementState>(actor).unwrap().velocity.y <= 0.1,
                "unexpected jump {scenario}"
            );
        } else {
            assert!(
                app.world().get::<MovementState>(actor).unwrap().velocity.y > 0.0,
                "must fall back to normal jump: {scenario}"
            );
        }
    }
}

#[test]
fn movement_vault_aborts_on_changed_cover_blocking_geometry_and_lifecycle() {
    for cause in ["removed", "blocked", "impact", "stale", "raid"] {
        let (mut app, actor, obstacle, origin) = vault_course(0.65, 0.6);
        request_vault(&mut app, actor);
        held_steps(&mut app, actor, 16);
        assert!(
            app.world().get::<PoseState>(actor).unwrap().vault != 0,
            "no start {cause}"
        );
        match cause {
            "removed" => {
                app.world_mut().entity_mut(obstacle).despawn();
            }
            "blocked" => {
                app.world_mut().spawn((
                    Collider::cuboid(1.0, 2.5, 0.1),
                    Transform::from_xyz(origin.x, 2.5, origin.z - 0.95),
                ));
            }
            "impact" => {
                app.world_mut().get_mut::<PoseState>(actor).unwrap().impulse = [4.0, 0.0, 0.0];
            }
            "stale" => {
                app.world_mut()
                    .get_mut::<LastInput>(actor)
                    .unwrap()
                    .received_at = -100.0;
            }
            "raid" => {
                app.world_mut().resource_mut::<RaidState>().raid_id += 1;
            }
            _ => unreachable!(),
        }
        let mut aborted = false;
        for _ in 0..180 {
            if cause == "stale" {
                steps(&mut app, 1);
            } else {
                held_steps(&mut app, actor, 1);
            }
            let root = app.world().get::<Transform>(actor).unwrap().translation;
            let pose = app.world().get::<PoseState>(actor).unwrap();
            aborted |= pose.vault == 0;
            assert!(root.is_finite());
            if cause == "blocked" {
                assert!(root.z > origin.z - 0.90, "crossed new wall: {root:?}");
            }
        }
        assert!(aborted && app.world().get::<PoseState>(actor).unwrap().vault == 0);
        if cause == "impact" {
            assert!(
                app.world()
                    .get::<PoseState>(actor)
                    .unwrap()
                    .physical
                    .is_some(),
                "impact must interrupt into physical fall"
            );
        } else {
            let pose = app.world().get::<PoseState>(actor).unwrap();
            assert!(
                matches!(pose.mode, PhysicalMode::Standing | PhysicalMode::Stumbling),
                "{cause}: {pose:?}"
            );
            assert!(pose.grounded, "{cause} did not land");
        }
    }
}
