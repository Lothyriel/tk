//! Authoritative configured-weapon pose in actor-local coordinates.
//!
//! Resolve after Rapier writeback and lean preparation, before firing and
//! snapshots. The actor root, collision queries, shot and replicated local pose
//! therefore all refer to the same completed simulation step.
use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;
use common::{
    catalog::{AnchorKind, Bounds, ResolvedWeapon},
    combat::*,
    humanoid::*,
    raid::*,
};

const WEAPON_RADIUS: f32 = 0.065;
const ARM_RADIUS: f32 = 0.085;
const HAND_RADIUS: f32 = 0.105;
// Extra clearance includes packed-position rounding and prevents a touching
// shape cast from rejecting the next stationary pose.
const CLEARANCE: f32 = 0.012;
const RETRACTION_STEP: f32 = 0.025;
const RETRACTION_STEPS: usize = 30;
const RECOVERY_METRES_PER_SECOND: f32 = 2.0;
const MAX_CANDIDATES_PER_PASS: usize = 128;
const M4A1_GAME_ZERO: f32 = 20.0;

struct AimReference {
    eye: Vec3,
    rotation: Quat,
    delta: f32,
}

#[derive(Clone)]
pub(super) struct Previous {
    raid: u32,
    life: u8,
    weapon: common::WeaponInstanceId,
    revision: u64,
    actor: Vec3,
    actor_rotation: Quat,
    aim_roll: f32,
    weapon_root: Transform,
    retraction: f32,
    yaw: f32,
    lateral: f32,
    prone: bool,
    crouched: bool,
    segments: [(Vec3, Vec3, f32); 7],
    points: Vec<(Vec3, f32)>,
}

fn filter(entity: Entity) -> QueryFilter<'static> {
    QueryFilter::new()
        .exclude_collider(entity)
        .exclude_sensors()
        // Include fragments and moving rubble as well as map and actors.
        .groups(CollisionGroups::new(Group::ALL, Group::ALL))
}

/// Candidate roots are fixed by collision search; projecting them to make a
/// hand reachable would invalidate the collision test. Solve each arm once and
/// reject unreachable contacts without running the iterative root fitter.
fn solve_fixed(
    root: Transform,
    contacts: handling::WeaponContacts,
    posture: configured::Posture,
) -> Option<handling::HandlingSolution> {
    let support = handling::solve_arm(
        handling::Hand::Support,
        posture.shoulders[0],
        root.mul_transform(contacts.support),
        posture.poles[0],
    )
    .ok()?;
    let trigger = handling::solve_arm(
        handling::Hand::Trigger,
        posture.shoulders[1],
        root.mul_transform(contacts.trigger),
        posture.poles[1],
    )
    .ok()?;
    Some(handling::HandlingSolution {
        root,
        support,
        trigger,
    })
}

/// Rotate the actual rigid assembly toward the game-zero solution, rather
/// than sending a projectile in a direction different from the visible bore.
/// Align the sight with the eye when the arm geometry permits it.
fn calibrate(
    base: handling::HandlingSolution,
    posture: configured::Posture,
    contacts: handling::WeaponContacts,
    spec: &ResolvedWeapon,
    actor: Transform,
    ads: f32,
    aim: AimReference,
) -> Option<handling::HandlingSolution> {
    let weight = common::weapon_animation::ease(ads);
    if weight <= 0.0 {
        return Some(base);
    }
    let outlet = common::weapon_animation::configured_outlet(spec);
    let bore = common::weapon_animation::configured_bore(spec);
    let original_bore = actor.rotation * base.root.rotation * bore.rotation * Vec3::NEG_Z;
    let forward = aim.rotation * Vec3::NEG_Z;
    let target = aim.eye + forward * M4A1_GAME_ZERO;
    let eye = actor.rotation.inverse() * (aim.eye - actor.translation);
    let sight = spec.assembly.anchors.get(&AnchorKind::Sight).copied();
    let sight_point = sight.map(|sight| {
        let forward = actor.rotation.inverse() * forward;
        let depth = (base.root.transform_point(sight.translation) - eye).dot(forward);
        let depth = if spec.class == common::WeaponKind::Pistol {
            0.43
        } else {
            depth.max(0.015)
        };
        eye + forward * depth
    });
    let trigger = base.root.transform_point(contacts.trigger.translation);
    let mut result = base;
    for _ in 0..4 {
        let muzzle = actor
            .mul_transform(result.root)
            .mul_transform(outlet)
            .translation;
        let to_target = target - muzzle;
        let mut offset = to_target;
        for _ in 0..3 {
            let ticks = offset.length() / spec.muzzle_speed / aim.delta;
            let whole = ticks.floor();
            let drop = common::PROJECTILE_GRAVITY
                * aim.delta
                * aim.delta
                * (whole + 1.0)
                * (whole * 0.5 + ticks.fract());
            offset = to_target + Vec3::Y * drop;
        }
        let target_bore = original_bore
            .lerp(offset.normalize_or_zero(), weight)
            .normalize_or_zero();
        if target_bore.length_squared() < 0.5 {
            return None;
        }
        let correction = Quat::from_rotation_arc(original_bore, target_bore);
        let rotation = actor.rotation.inverse() * correction * actor.rotation * base.root.rotation;
        let preferred = trigger - rotation * contacts.trigger.translation;
        let mut aligned = None;
        if let (Some(sight), Some(point)) = (sight, sight_point) {
            let sight_rotation = actor.rotation.inverse() * aim.rotation * sight.rotation.inverse();
            let rotation = rotation.slerp(
                if spec.definition == common::WeaponId::M4A1 {
                    let sight_bore = actor.rotation * sight_rotation * bore.rotation * Vec3::NEG_Z;
                    actor.rotation.inverse()
                        * Quat::from_rotation_arc(sight_bore, target_bore)
                        * actor.rotation
                        * sight_rotation
                } else {
                    sight_rotation
                },
                weight,
            );
            let braced = point - rotation * sight.translation;
            for fraction in [1.0, 0.75, 0.5, 0.25] {
                let root = Transform::from_translation(preferred.lerp(braced, weight * fraction))
                    .with_rotation(rotation);
                if let Some(solved) = solve_fixed(root, contacts, posture) {
                    aligned = Some(solved);
                    break;
                }
            }
        }
        result = if let Some(aligned) = aligned {
            aligned
        } else if spec.definition == common::WeaponId::M4A1 {
            handling::fit_weapon(
                Transform::from_translation(preferred).with_rotation(rotation),
                contacts,
                posture.shoulders,
                posture.poles,
                posture.stock_target,
                posture.max_adjustment,
            )
            .ok()?
        } else {
            return Some(base);
        };
    }
    Some(result)
}

/// Segments are actor-local. Authored parts use their individual bounds; the
/// weapon capsule covers the bore-to-outlet channel (or the legacy assembly
/// from its trigger to outlet). Remaining capsules cover both connected arms.
fn segments(
    root: Transform,
    solution: handling::HandlingSolution,
    shoulders: [Vec3; 2],
    rear: Vec3,
    outlet: Vec3,
    weapon_radius: f32,
) -> [(Vec3, Vec3, f32); 7] {
    [
        (
            root.transform_point(rear),
            root.transform_point(outlet),
            weapon_radius,
        ),
        (shoulders[0], solution.support.elbow, ARM_RADIUS),
        (
            solution.support.elbow,
            solution.support.wrist.translation,
            ARM_RADIUS,
        ),
        (shoulders[1], solution.trigger.elbow, ARM_RADIUS),
        (
            solution.trigger.elbow,
            solution.trigger.wrist.translation,
            ARM_RADIUS,
        ),
        (
            solution.support.wrist.translation,
            solution.support.wrist.translation,
            HAND_RADIUS,
        ),
        (
            solution.trigger.wrist.translation,
            solution.trigger.wrist.translation,
            HAND_RADIUS,
        ),
    ]
}

fn clear(
    context: &RapierContext<'_>,
    entity: Entity,
    actor: Transform,
    weapon_root: Transform,
    bounds: &[Bounds],
    segments: &[(Vec3, Vec3, f32); 7],
    previous: Option<(&Previous, &HashSet<Entity>)>,
) -> Option<Vec<(Vec3, f32)>> {
    let world_root = actor.mul_transform(weapon_root);
    let allow_sweep = |entity: Entity, _: &bevy_rapier3d::rapier::geometry::Collider| {
        previous.is_none_or(|(_, overlapping)| !overlapping.contains(&entity))
    };
    for bounds in bounds {
        let min = Vec3::from(bounds.min);
        let max = Vec3::from(bounds.max);
        let center = (min + max) * 0.5;
        let half = (max - min) * 0.5;
        let shape = Collider::cuboid(half.x + CLEARANCE, half.y + CLEARANCE, half.z + CLEARANCE);
        let mut overlap = false;
        context.intersect_shape(
            world_root.transform_point(center),
            world_root.rotation,
            &shape,
            filter(entity),
            |_, _| {
                overlap = true;
                false
            },
        );
        if overlap {
            return None;
        }
        if let Some((previous, _)) = previous {
            let start = previous.weapon_root.transform_point(center);
            let end = world_root.transform_point(center);
            let angular =
                (world_root.rotation * previous.weapon_root.rotation.inverse()).to_scaled_axis();
            if start.distance_squared(end) > 1e-8 || angular.length_squared() > 1e-8 {
                let shape = Collider::cuboid(half.x, half.y, half.z);
                let motion = NonlinearMotion {
                    translation: start,
                    rotation: previous.weapon_root.rotation,
                    // The authored box is centred on `center`, but the whole
                    // rigid assembly rotates around the weapon root.
                    local_center: -center,
                    linear_velocity: world_root.translation - previous.weapon_root.translation,
                    angular_velocity: angular,
                };
                if context
                    .cast_shape_nonlinear(
                        &motion,
                        &shape,
                        0.0,
                        1.0,
                        false,
                        filter(entity).predicate(&allow_sweep),
                    )
                    .is_some()
                {
                    return None;
                }
            }
        }
    }
    let mut points = Vec::with_capacity(45);
    for (index, &(a, b, radius)) in segments.iter().enumerate() {
        let a = actor.transform_point(a);
        let b = actor.transform_point(b);
        let capsule = Collider::capsule(a, b, radius + CLEARANCE);
        let mut overlap = false;
        context.intersect_shape(
            Vec3::ZERO,
            Quat::IDENTITY,
            &capsule,
            filter(entity),
            |_, _| {
                overlap = true;
                false
            },
        );
        if overlap {
            return None;
        }
        // Fixed counts preserve correspondence across a rigid configuration.
        // Adjacent balls overlap even on the longest supported assembly.
        let count = if index == 0 {
            16
        } else if index >= 5 {
            0
        } else {
            6
        };
        for i in 0..=count {
            let weight = if count == 0 {
                0.0
            } else {
                i as f32 / count as f32
            };
            points.push((a.lerp(b, weight), radius));
        }
    }
    if let Some((previous, _)) = previous {
        if previous.points.len() != points.len() {
            return None;
        }
        for (&(start, radius), &(end, _)) in previous.points.iter().zip(&points) {
            let travel = end - start;
            if travel.length_squared() < 1e-8 {
                continue;
            }
            let ball = Collider::ball(radius);
            let options = ShapeCastOptions {
                stop_at_penetration: false,
                ..ShapeCastOptions::with_max_time_of_impact(1.0)
            };
            if context
                .cast_shape(
                    start,
                    Quat::IDENTITY,
                    travel,
                    &ball,
                    options,
                    filter(entity).predicate(&allow_sweep),
                )
                .is_some()
            {
                return None;
            }
        }
    }
    Some(points)
}

fn previous_overlaps(
    context: &RapierContext<'_>,
    entity: Entity,
    previous: &Previous,
    bounds: &[Bounds],
) -> HashSet<Entity> {
    let mut overlaps = HashSet::new();
    for &(a, b, radius) in &previous.segments {
        let shape = Collider::capsule(a, b, radius);
        context.intersect_shape(
            Vec3::ZERO,
            Quat::IDENTITY,
            &shape,
            filter(entity),
            |other, _| {
                overlaps.insert(other);
                true
            },
        );
    }
    for bounds in bounds {
        let min = Vec3::from(bounds.min);
        let max = Vec3::from(bounds.max);
        let half = (max - min) * 0.5;
        let shape = Collider::cuboid(half.x, half.y, half.z);
        context.intersect_shape(
            previous.weapon_root.transform_point((min + max) * 0.5),
            previous.weapon_root.rotation,
            &shape,
            filter(entity),
            |other, _| {
                overlaps.insert(other);
                true
            },
        );
    }
    overlaps
}

fn shoulders_clear(
    context: &RapierContext<'_>,
    entity: Entity,
    actor: Transform,
    shoulders: [Vec3; 2],
) -> bool {
    let shape = Collider::ball(ARM_RADIUS + CLEARANCE);
    shoulders.into_iter().all(|shoulder| {
        let mut hit = false;
        context.intersect_shape(
            actor.transform_point(shoulder),
            Quat::IDENTITY,
            &shape,
            filter(entity),
            |_, _| {
                hit = true;
                false
            },
        );
        !hit
    })
}

pub(super) fn resolve(
    context: ReadRapierContext,
    raid: Res<RaidState>,
    time: Res<Time>,
    mut history: Local<HashMap<Entity, Previous>>,
    mut actors: Query<(
        Entity,
        &Transform,
        &Collider,
        &common::ClientInput,
        &common::MovementState,
        &mut PoseState,
        &BodyHealth,
        &ArmorState,
        &Arsenal,
        &Participant,
        &super::arena::Life,
    )>,
) {
    let Ok(context) = context.single() else {
        for (_, _, _, _, _, mut pose, ..) in &mut actors {
            pose.weapon_root = PackedWeaponRoot::default();
        }
        history.clear();
        return;
    };
    let mut seen = HashSet::new();
    for (
        entity,
        actor,
        collider,
        input,
        movement,
        mut pose,
        body,
        armor,
        arsenal,
        participant,
        life,
    ) in &mut actors
    {
        seen.insert(entity);
        pose.weapon_root = PackedWeaponRoot::default();
        if raid.phase != RaidPhase::Active
            || participant.status != RaidStatus::Active
            || !body.alive()
            || body.anatomy.capacity(Region::RightArm) <= 0.0
            || !matches!(pose.mode, PhysicalMode::Standing | PhysicalMode::Stumbling)
        {
            history.remove(&entity);
            continue;
        }
        let legs = [Region::LeftLeg, Region::RightLeg].map(|r| body.anatomy.capacity(r));
        let weapon = arsenal.current();
        let instance = weapon.instance();
        if history.get(&entity).is_some_and(|old| {
            old.raid != raid.raid_id
                || old.life != life.generation
                || old.weapon != instance.id
                || old.revision != instance.revision
                || old.actor.distance(actor.translation) >= 2.0
        }) {
            history.remove(&entity);
        }
        let previous = history.get(&entity);
        // A collider may enter the previous volume between physics steps. Its
        // initial overlap may be escaped, but it must not disable sweeping
        // against unrelated obstacles and the replacement must be fully clear.
        let spec = weapon.presentation_spec();
        let overlaps = previous
            .map(|old| previous_overlaps(&context, entity, old, &spec.assembly.obstruction))
            .unwrap_or_default();
        let mechanics = weapon.mechanics().snapshot();
        let action = common::weapon_animation::manipulation::action(spec.class, &mechanics);
        let Ok(rig) = configured::evaluate_with_carry(
            &pose,
            legs,
            spec,
            &mechanics,
            Some(action),
            Some(configured::shoulder_cover(armor)),
        ) else {
            continue;
        };
        let (Some(base), Some(posture), Ok(contacts)) = (
            rig.weapon,
            rig.posture,
            handling::configured_contacts(spec, action, 0.0, &mechanics),
        ) else {
            continue;
        };
        let camera = arsenal.recoil.camera(common::CameraInput {
            yaw: if movement.prone {
                actor.rotation.to_euler(EulerRot::YXZ).0
            } else {
                input.camera.yaw
            },
            roll: pose.aim_roll,
            ..input.camera
        });
        let aim = AimReference {
            eye: super::simulation::neutral_eye(actor, movement)
                + actor.rotation * aim_lean_offset(pose.aim_roll),
            rotation: Quat::from(&camera),
            delta: time.delta_secs(),
        };
        let Some(base) = calibrate(base, posture, contacts, spec, *actor, pose.ads, aim) else {
            continue;
        };
        // These endpoints cannot move with the weapon. If either is already
        // inside an obstacle, no choice of root can produce clear arm capsules.
        if !shoulders_clear(&context, entity, *actor, posture.shoulders) {
            continue;
        }
        let outlet = common::weapon_animation::configured_outlet(spec);
        let bore = spec.assembly.anchors.get(&AnchorKind::Bore);
        let rear = bore.unwrap_or(&contacts.trigger).translation;
        let weapon_radius = if bore.is_some() { WEAPON_RADIUS } else { 0.05 };
        let min_retraction = previous.map_or(0.0, |old| {
            (old.retraction - RECOVERY_METRES_PER_SECOND * time.delta_secs()).max(0.0)
        });
        let prior_yaw = previous.map_or(0.0, |old| {
            old.yaw.signum() * (old.yaw.abs() - 3.0 * time.delta_secs()).max(0.0)
        });
        let prior_lateral = previous.map_or(0.0, |old| {
            old.lateral.signum() * (old.lateral.abs() - 2.0 * time.delta_secs()).max(0.0)
        });
        let trigger = base.root.transform_point(contacts.trigger.translation);
        let mut travel = actor.rotation * Vec3::from(pose.velocity);
        travel.y = 0.0;
        travel *= 0.04;
        if travel.length_squared() > 1e-6 {
            let options = ShapeCastOptions {
                stop_at_penetration: false,
                ..ShapeCastOptions::with_max_time_of_impact(1.0)
            };
            if let Some((_, hit)) = context.cast_shape(
                actor.translation,
                actor.rotation,
                travel,
                collider,
                options,
                filter(entity),
            ) {
                let distance = (hit.time_of_impact * travel.length() - 0.02).max(0.0);
                travel *= (distance / travel.length()).clamp(0.0, 1.0);
            }
        }
        let mut future_actor = *actor;
        future_actor.translation += travel;
        let can_forecast = travel.length_squared() > 1e-6
            && shoulders_clear(&context, entity, future_actor, posture.shoulders);
        'candidates: for lookahead in [true, false] {
            if lookahead && !can_forecast {
                continue;
            }
            for pure_retraction in [true, false] {
                let mut attempts = 0;
                'search: for step in 0..=RETRACTION_STEPS * 2 {
                    let offset = if step == 0 {
                        0.0
                    } else {
                        let direction = if step % 2 == 1 { 1.0 } else { -1.0 };
                        direction * step.div_ceil(2) as f32 * RETRACTION_STEP
                    };
                    let retraction = min_retraction + offset;
                    if !(0.0..=RETRACTION_STEPS as f32 * RETRACTION_STEP).contains(&retraction) {
                        continue;
                    }
                    let yaw_offsets = if min_retraction > 0.30 {
                        [0.0, -0.6, 0.6, -0.15, 0.15]
                    } else {
                        [0.0, -0.15, 0.15, -0.6, 0.6]
                    };
                    for yaw_offset in yaw_offsets {
                        let yaw = prior_yaw + yaw_offset;
                        if yaw.abs() > 1.3 {
                            continue;
                        }
                        for side_offset in [0.0, -0.05, 0.05, -0.15, 0.15] {
                            if pure_retraction != (yaw_offset == 0.0 && side_offset == 0.0) {
                                continue;
                            }
                            let lateral = prior_lateral + side_offset;
                            if lateral.abs() > 0.25 {
                                continue;
                            }
                            attempts += 1;
                            let budget = if overlaps.is_empty() {
                                MAX_CANDIDATES_PER_PASS
                            } else {
                                MAX_CANDIDATES_PER_PASS * 4
                            };
                            if attempts > budget {
                                break 'search;
                            }
                            let rotation = base.root.rotation * Quat::from_rotation_y(yaw);
                            let candidate = Transform::from_translation(
                                trigger - rotation * contacts.trigger.translation
                                    + rotation * Vec3::Z * retraction
                                    + Vec3::X * lateral,
                            )
                            .with_rotation(rotation);
                            let solved = if retraction == 0.0 && yaw == 0.0 && lateral == 0.0 {
                                Some(base)
                            } else {
                                // Release the stock pocket while retracting or turning aside;
                                // both grip contacts and anatomical reach remain exact.
                                solve_fixed(candidate, contacts, posture)
                            };
                            let Some(mut solved) = solved else {
                                continue;
                            };
                            let mut shape = segments(
                                solved.root,
                                solved,
                                posture.shoulders,
                                rear,
                                outlet.translation,
                                weapon_radius,
                            );
                            let Some(mut points) = clear(
                                &context,
                                entity,
                                *actor,
                                solved.root,
                                &spec.assembly.obstruction,
                                &shape,
                                previous.map(|old| (old, &overlaps)),
                            ) else {
                                continue;
                            };
                            if lookahead
                                && clear(
                                    &context,
                                    entity,
                                    future_actor,
                                    solved.root,
                                    &spec.assembly.obstruction,
                                    &shape,
                                    None,
                                )
                                .is_none()
                            {
                                continue;
                            }
                            let mut accepted_retraction = retraction;
                            let mut accepted_yaw = yaw;
                            let mut accepted_lateral = lateral;
                            if let Some(old) = previous.filter(|old| {
                                overlaps.is_empty()
                                    && old.prone == pose.prone
                                    && old.crouched == (pose.crouch > 0.5)
                            }) {
                                let target = actor.mul_transform(solved.root);
                                let distance =
                                    old.weapon_root.translation.distance(target.translation);
                                let angle = old.weapon_root.rotation.angle_between(target.rotation);
                                let moving = Vec3::from(pose.velocity).length() > 0.5;
                                // Follow actual vertical as well as horizontal body
                                // displacement. Otherwise jumping or landing can
                                // leave both arms unable to reach a stale world pose.
                                let translation_limit = (if moving { 6.0 } else { 2.0 }
                                    * time.delta_secs())
                                .max(old.actor.distance(actor.translation) + 0.004)
                                .min(0.08);
                                let rotation_limit =
                                    if moving { 7.0 } else { 2.0 } * time.delta_secs();
                                let factor = (translation_limit / distance.max(1e-6))
                                    .min(rotation_limit / angle.max(1e-6))
                                    .min(1.0);
                                // A clear, fully raised weapon follows the aim input.
                                // The candidate's entire path was already swept above.
                                let clear_ads = pose.ads > 0.98
                                    && [
                                        retraction,
                                        yaw,
                                        lateral,
                                        old.retraction,
                                        old.yaw,
                                        old.lateral,
                                    ]
                                    .into_iter()
                                    .all(|offset| offset == 0.0);
                                if factor < 1.0 && !clear_ads {
                                    let mut accepted = None;
                                    for scale in [1.0, 0.75, 0.5, 0.25, 0.0] {
                                        let weight = factor * scale;
                                        let world_position = old
                                            .weapon_root
                                            .translation
                                            .lerp(target.translation, weight);
                                        let world_rotation =
                                            old.weapon_root.rotation.slerp(target.rotation, weight);
                                        let local = Transform::from_translation(
                                            actor.rotation.inverse()
                                                * (world_position - actor.translation),
                                        )
                                        .with_rotation(actor.rotation.inverse() * world_rotation);
                                        let Some(intermediate) =
                                            solve_fixed(local, contacts, posture)
                                        else {
                                            continue;
                                        };
                                        let intermediate_shape = segments(
                                            intermediate.root,
                                            intermediate,
                                            posture.shoulders,
                                            rear,
                                            outlet.translation,
                                            weapon_radius,
                                        );
                                        let Some(intermediate_points) = clear(
                                            &context,
                                            entity,
                                            *actor,
                                            intermediate.root,
                                            &spec.assembly.obstruction,
                                            &intermediate_shape,
                                            Some((old, &overlaps)),
                                        ) else {
                                            continue;
                                        };
                                        accepted = Some((
                                            weight,
                                            intermediate,
                                            intermediate_shape,
                                            intermediate_points,
                                        ));
                                        break;
                                    }
                                    if let Some((
                                        weight,
                                        intermediate,
                                        intermediate_shape,
                                        intermediate_points,
                                    )) = accepted
                                    {
                                        solved = intermediate;
                                        shape = intermediate_shape;
                                        points = intermediate_points;
                                        accepted_retraction =
                                            old.retraction + (retraction - old.retraction) * weight;
                                        accepted_yaw = old.yaw + (yaw - old.yaw) * weight;
                                        accepted_lateral =
                                            old.lateral + (lateral - old.lateral) * weight;
                                    } else if old.actor_rotation.angle_between(actor.rotation)
                                        <= 0.10
                                        && (old.aim_roll - pose.aim_roll).abs() <= 0.10
                                    {
                                        continue;
                                    }
                                    // If body motion makes every interpolated grip unreachable,
                                    // the fully swept and reachable candidate remains valid. Holding
                                    // the stale root would permanently disable the weapon.
                                }
                            }
                            let Some(packed) = PackedWeaponRoot::pack(solved.root) else {
                                continue;
                            };
                            pose.weapon_root = packed;
                            history.insert(
                                entity,
                                Previous {
                                    raid: raid.raid_id,
                                    life: life.generation,
                                    weapon: instance.id,
                                    revision: instance.revision,
                                    actor: actor.translation,
                                    actor_rotation: actor.rotation,
                                    aim_roll: pose.aim_roll,
                                    weapon_root: actor.mul_transform(solved.root),
                                    retraction: accepted_retraction,
                                    yaw: accepted_yaw,
                                    lateral: accepted_lateral,
                                    prone: pose.prone,
                                    crouched: pose.crouch > 0.5,
                                    segments: shape.map(|(a, b, radius)| {
                                        (actor.transform_point(a), actor.transform_point(b), radius)
                                    }),
                                    points,
                                },
                            );
                            break 'candidates;
                        }
                    }
                }
            }
        }
    }
    history.retain(|entity, _| seen.contains(entity));
}
