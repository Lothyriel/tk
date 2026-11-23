//! Obstacle stumbles come from a blocking KCC sweep, not input or proximity.
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;
use common::{
    ClientInput, MovementState, PLAYER_COLLIDER_HALF_HEIGHT, PLAYER_COLLIDER_RADIUS,
    PLAYER_STEP_HEIGHT, PLAYER_WALK_SPEED,
    combat::{BodyHealth, SurfaceKind},
    humanoid::{PhysicalMode, PoseState},
    raid::{Participant, RaidPhase, RaidState, RaidStatus},
};

#[derive(Component, Default)]
pub(super) struct Contact {
    obstacle: Option<Entity>,
    reacting: bool,
}

const MAX_OBSTACLE_HEIGHT: f32 = 0.65;
// The KCC stops short of the collider by its skin offset. Require a gap
// beyond that skin (plus settlement jitter) before rearming.
const CONTACT_MARGIN: f32 = 0.06;

pub(super) fn limit_stride(
    mut actors: Query<(&PoseState, &Contact, &mut KinematicCharacterController)>,
) {
    for (pose, contact, mut controller) in &mut actors {
        if contact.reacting
            && pose.mode == PhysicalMode::Stumbling
            && let Some(travel) = &mut controller.translation
        {
            travel.x *= 0.4;
            travel.z *= 0.4;
        }
    }
}

pub(super) fn detect(
    context: ReadRapierContext,
    time: Res<Time>,
    raid: Res<RaidState>,
    surfaces: Query<&SurfaceKind>,
    mut actors: Query<(
        &ClientInput,
        &Participant,
        &BodyHealth,
        &MovementState,
        &Transform,
        &KinematicCharacterControllerOutput,
        &mut Contact,
        &mut PoseState,
    )>,
) {
    let Ok(context) = context.single() else {
        return;
    };
    for (input, player, body, movement, root, sweep, mut contact, mut pose) in &mut actors {
        if raid.phase != RaidPhase::Active || player.status != RaidStatus::Active || !body.alive() {
            *contact = Contact::default();
            continue;
        }
        contact.reacting &= pose.mode == PhysicalMode::Stumbling;
        if let Some(obstacle) = contact.obstacle {
            // A stationary controller has no forward sweep. Check real separation
            // from the same collider instead of interpreting that as a new contact.
            let predicate = |hit, _: &_| obstacle == hit;
            let lower_half = if movement.crouched {
                common::PLAYER_CROUCH_COLLIDER_HALF_HEIGHT
            } else {
                PLAYER_COLLIDER_HALF_HEIGHT
            };
            let lower_center = root.translation - Vec3::Y * lower_half;
            if sweep.collisions.iter().any(|hit| hit.entity == obstacle)
                || context
                    .project_point(
                        lower_center,
                        PLAYER_COLLIDER_RADIUS + CONTACT_MARGIN,
                        true,
                        QueryFilter::new().predicate(&predicate),
                    )
                    .is_some()
            {
                continue;
            }
            contact.obstacle = None;
        }
        let desired = sweep.desired_translation.with_y(0.0);
        let forward = Quat::from_rotation_y(input.camera.yaw) * -Vec3::Z;
        if pose.mode != PhysicalMode::Standing
            || !pose.grounded
            || !sweep.grounded
            || pose.vault != 0
            || movement.crouched
            || !movement.sprinting
            || !input.forward
            || desired.length() < (PLAYER_WALK_SPEED + 0.25) * time.delta_secs()
        {
            continue;
        }
        let direction = desired.normalize_or_zero();
        let progress = sweep.effective_translation.dot(direction);
        if progress >= desired.length() * 0.7 {
            continue; // A graze or successful autostep is not a blocked stride.
        }
        let foot_y = root.translation.y - PLAYER_COLLIDER_HALF_HEIGHT - PLAYER_COLLIDER_RADIUS;
        for hit in &sweep.collisions {
            let Some(details) = hit.hit.details else {
                continue;
            };
            let normal = details.normal1;
            if surfaces.get(hit.entity).is_err()
                || normal.y.abs() > 0.35
                || normal.dot(direction) > -0.7
                || normal.dot(forward) > -0.7
            {
                continue;
            }
            // Find the top of this exact blocking collider, just inside the
            // hit face. A high wall contains the ray origin and is rejected.
            let inside = details.witness1 - normal * 0.04;
            let origin = inside.with_y(foot_y + MAX_OBSTACLE_HEIGHT);
            // Include adjacent solid cells in the probe so a wall assembled
            // from stacked voxels cannot masquerade as its bottom cell.
            let predicate = |entity, _: &_| surfaces.contains(entity);
            let Some((surface, top)) = context.cast_ray_and_get_normal(
                origin,
                -Vec3::Y,
                MAX_OBSTACLE_HEIGHT,
                true,
                QueryFilter::new().predicate(&predicate),
            ) else {
                continue;
            };
            let height = MAX_OBSTACLE_HEIGHT - top.time_of_impact;
            if surface != hit.entity
                || top.time_of_impact <= 0.0
                || top.normal.y < 0.8
                || height <= PLAYER_STEP_HEIGHT
            {
                continue;
            }
            contact.obstacle = Some(hit.entity);
            contact.reacting = true;
            pose.mode = PhysicalMode::Stumbling;
            pose.mode_time = 0.0;
            break;
        }
    }
}
