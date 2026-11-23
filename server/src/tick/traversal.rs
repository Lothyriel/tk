//! Short, geometry-qualified vault assistance. Rapier still sweeps every tick;
//! after clearing the lip the ordinary controller owns gravity and landing.
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;
use common::{
    MovementState, PLAYER_COLLIDER_HALF_HEIGHT, PLAYER_COLLIDER_RADIUS, PLAYER_STEP_HEIGHT,
    humanoid::PoseState,
};

const MAX_HEIGHT: f32 = 1.0;
const MAX_DEPTH: f32 = 1.0;
const REACH: f32 = 1.0;
const LIFT_TIME: f32 = 0.25;

#[derive(Component, Default)]
pub(super) struct State {
    pub motion: Option<Motion>,
    pub raid: u32,
}

pub(super) struct Motion {
    obstacle: Entity,
    start: Vec3,
    raised: Vec3,
    end: Vec3,
    elapsed: f32,
    duration: f32,
    exit_velocity: Vec3,
    expected: Vec3,
}

fn solids(entity: Entity) -> QueryFilter<'static> {
    QueryFilter::new()
        .exclude_collider(entity)
        .exclude_sensors()
        .groups(CollisionGroups::new(
            Group::ALL,
            Group::GROUP_1 | Group::GROUP_3,
        ))
}

fn clear(context: &RapierContext<'_>, entity: Entity, from: Vec3, to: Vec3) -> bool {
    let capsule = Collider::capsule_y(PLAYER_COLLIDER_HALF_HEIGHT, PLAYER_COLLIDER_RADIUS);
    context
        .cast_shape(
            from,
            Quat::IDENTITY,
            to - from,
            &capsule,
            ShapeCastOptions::with_max_time_of_impact(1.0),
            solids(entity),
        )
        .is_none()
}

pub(super) fn plan(
    context: &RapierContext<'_>,
    entity: Entity,
    root: Vec3,
    direction: Vec3,
    velocity: Vec3,
) -> Option<Motion> {
    let half_height = PLAYER_COLLIDER_HALF_HEIGHT + PLAYER_COLLIDER_RADIUS;
    let foot = root.y - half_height;
    let fixed = QueryFilter::only_fixed()
        .exclude_sensors()
        .groups(CollisionGroups::new(Group::ALL, Group::GROUP_1));
    let (obstacle, face) = context.cast_ray_and_get_normal(
        root.with_y(foot + PLAYER_STEP_HEIGHT + 0.02),
        direction,
        REACH,
        true,
        fixed,
    )?;
    if face.time_of_impact <= 0.0 || face.normal.dot(direction) > -0.7 {
        return None;
    }
    let inside = face.point + direction * 0.03;
    let (_, top) = context.cast_ray_and_get_normal(
        inside.with_y(foot + MAX_HEIGHT + 0.02),
        -Vec3::Y,
        MAX_HEIGHT + 0.02,
        true,
        fixed,
    )?;
    let height = top.point.y - foot;
    if top.time_of_impact <= 0.0
        || top.normal.y < 0.8
        || !(PLAYER_STEP_HEIGHT + 0.01..=MAX_HEIGHT).contains(&height)
    {
        return None;
    }
    // Probe back from beyond the maximum accepted depth. A ray starting inside
    // a wider/stacked obstacle returns zero and cannot qualify as a thin lip.
    let far = face.point + direction * (MAX_DEPTH + 0.02);
    let (back_entity, back) =
        context.cast_ray_and_get_normal(far, -direction, MAX_DEPTH + 0.02, true, fixed)?;
    let depth = (back.point - face.point).dot(direction);
    if back_entity != obstacle
        || back.time_of_impact <= 0.0
        || !(0.02..=MAX_DEPTH).contains(&depth)
        || back.normal.dot(direction) < 0.7
    {
        return None;
    }
    let raised = root.with_y(top.point.y + half_height + 0.08);
    let end = (back.point + direction * (PLAYER_COLLIDER_RADIUS + 0.12)).with_y(raised.y);
    if !clear(context, entity, root, raised) || !clear(context, entity, raised, end) {
        return None;
    }
    // The full envelope must have a supported landing on the far side, at most
    // a small step below the takeoff plane. Never assist a vault into a pit.
    let capsule = Collider::capsule_y(PLAYER_COLLIDER_HALF_HEIGHT, PLAYER_COLLIDER_RADIUS);
    let (_, landing) = context.cast_shape(
        end,
        Quat::IDENTITY,
        -Vec3::Y,
        &capsule,
        ShapeCastOptions::with_max_time_of_impact(height + 0.30),
        solids(entity),
    )?;
    if landing.details.is_none_or(|d| d.normal1.y < 0.8)
        || end.y - landing.time_of_impact - half_height < foot - 0.20
    {
        return None;
    }
    let travel = end.distance(raised);
    Some(Motion {
        obstacle,
        start: root,
        raised,
        end,
        elapsed: 0.0,
        duration: LIFT_TIME + (travel / 4.5).clamp(0.30, 0.55),
        exit_velocity: direction * velocity.dot(direction).clamp(1.5, 5.5),
        expected: root,
    })
}

/// Returns true while assistance owns this tick's requested translation.
pub(super) fn advance(
    state: &mut State,
    context: &RapierContext<'_>,
    entity: Entity,
    root: &Transform,
    dt: f32,
    (movement, pose, controller): (
        &mut MovementState,
        &mut PoseState,
        &mut KinematicCharacterController,
    ),
) -> bool {
    let Some(motion) = &mut state.motion else {
        return false;
    };
    let obstacle_present = context
        .colliders
        .entity2collider()
        .contains_key(&motion.obstacle);
    if !obstacle_present || root.translation.distance(motion.expected) > 0.09 {
        state.motion = None;
        pose.vault = 0;
        return false;
    }
    motion.elapsed += dt;
    let progress = (motion.elapsed / motion.duration).min(1.0);
    let target = if motion.elapsed <= LIFT_TIME {
        motion.start.lerp(
            motion.raised,
            common::weapon_animation::ease(motion.elapsed / LIFT_TIME),
        )
    } else {
        motion.raised.lerp(
            motion.end,
            common::weapon_animation::ease(
                (motion.elapsed - LIFT_TIME) / (motion.duration - LIFT_TIME),
            ),
        )
    };
    if !clear(context, entity, root.translation, target) {
        state.motion = None;
        pose.vault = 0;
        return false;
    }
    movement.velocity = (target - root.translation) / dt;
    movement.grounded = false;
    movement.sprinting = false;
    pose.sprinting = false;
    pose.set_vault(Some(progress));
    pose.foot_weight = [0.0; 2];
    controller.autostep = None;
    controller.snap_to_ground = None;
    controller.translation = Some(target - root.translation);
    motion.expected = target;
    if progress >= 1.0 {
        movement.velocity = motion.exit_velocity;
        state.motion = None;
        // The final assisted tick is airborne. Clear the marker on the next
        // ordinary tick; no terminal pose survives a lost completion snapshot.
    }
    true
}
