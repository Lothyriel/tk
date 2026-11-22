//! Server-owned support controller and bounded articulated debris.
use super::{Bot, WorldState, can_occupy_character};
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;
use common::{combat::*, humanoid::*, raid::*, *};

pub const MAX_PIECES: usize = 320;
pub const PIECE_LIFETIME: f32 = 35.0;

#[derive(Component)]
pub(super) struct PhysicsPiece {
    pub id: u64,
    pub owner: u64,
    pub segment: Segment,
    pub detached: bool,
    pub bot: bool,
    pub remaining: f32,
}
#[derive(Component)]
pub(super) struct Corpsed;

#[derive(Clone, Copy)]
pub(super) struct PieceActor<'a> {
    pub root: &'a Transform,
    pub owner: u64,
    pub bot: bool,
}

fn spawn_piece(
    commands: &mut Commands,
    state: &mut WorldState,
    segment: Segment,
    pose: SegmentPose,
    actor: PieceActor<'_>,
    velocity: Velocity,
    detached: bool,
) -> Entity {
    let PieceActor { root, owner, bot } = actor;
    let half = segment.half_size() * 0.88;
    let t = root.mul_transform(pose.transform());
    let id = state.next_piece_id;
    state.next_piece_id = state.next_piece_id.wrapping_add(1);
    commands
        .spawn((
            PhysicsPiece {
                id,
                owner,
                segment,
                detached,
                bot,
                remaining: PIECE_LIFETIME,
            },
            RigidBody::Dynamic,
            Collider::cuboid(half.x, half.y, half.z),
            t,
            velocity,
            Damping {
                linear_damping: 0.25,
                angular_damping: 1.5,
            },
            Friction::coefficient(0.72),
            Restitution::coefficient(0.0),
            // Adjacent joints disable their contacts; non-adjacent limbs can rest on the torso.
            CollisionGroups::new(Group::GROUP_2, Group::GROUP_1 | Group::GROUP_2),
            ColliderMassProperties::Mass(super::ragdoll::mass(segment)),
            Ccd::enabled(),
            AdditionalSolverIterations(6),
        ))
        .id()
}

pub(super) fn spawn_fragments(
    commands: &mut Commands,
    state: &mut WorldState,
    mask: u32,
    rig: &[SegmentPose; SEGMENT_COUNT],
    actor: PieceActor<'_>,
    velocity: Vec3,
) {
    for segment in SEGMENTS {
        if mask & (1 << segment.index()) != 0 {
            spawn_piece(
                commands,
                state,
                segment,
                rig[segment.index()],
                actor,
                Velocity {
                    linear: velocity,
                    angular: Vec3::new(2.0, 1.0, 0.5),
                },
                true,
            );
        }
    }
}

pub(super) fn corpse(
    commands: &mut Commands,
    state: &mut WorldState,
    rig: &[SegmentPose; SEGMENT_COUNT],
    actor: PieceActor<'_>,
    velocities: &[(Vec3, Vec3); SEGMENT_COUNT],
    body: &BodyHealth,
) -> [Option<Entity>; SEGMENT_COUNT] {
    let mut entities = [None; SEGMENT_COUNT];
    for segment in SEGMENTS {
        if body.anatomy.missing & (1 << segment.index()) != 0 {
            continue;
        }
        entities[segment.index()] = Some(spawn_piece(
            commands,
            state,
            segment,
            rig[segment.index()],
            actor,
            Velocity {
                linear: velocities[segment.index()].0,
                angular: velocities[segment.index()].1,
            },
            false,
        ));
    }
    for segment in SEGMENTS {
        let Some(parent) = segment.parent() else {
            continue;
        };
        let (Some(a), Some(b)) = (entities[parent.index()], entities[segment.index()]) else {
            continue;
        };
        let pa = rig[parent.index()].transform();
        let pb = rig[segment.index()].transform();
        let anchor = match segment {
            Segment::Abdomen => pa.transform_point(Vec3::Y * 0.10),
            Segment::Thorax => pb.transform_point(-Vec3::Y * 0.20),
            Segment::Neck | Segment::Head => {
                (pa.transform_point(Vec3::Y * parent.half_size().y)
                    + pb.transform_point(-Vec3::Y * segment.half_size().y))
                    * 0.5
            }
            Segment::LeftHand | Segment::RightHand => pb.translation,
            Segment::LeftFoot | Segment::RightFoot => {
                pb.transform_point(Vec3::new(0.0, 0.005, 0.06))
            }
            _ => pb.transform_point(Vec3::Y * segment.half_size().y),
        };
        let mut joint = GenericJoint::new(JointAxesMask::LIN_AXES);
        joint.set_local_anchor1(pa.rotation.inverse() * (anchor - pa.translation));
        joint.set_local_anchor2(pb.rotation.inverse() * (anchor - pb.translation));
        // Knee limits are anatomical, not an extra 120 degrees on top of the
        // captured bend. Using the sampled calf as the neutral frame lets a
        // crouched/falling knee fold both shafts through one another.
        if !matches!(segment, Segment::LeftShin | Segment::RightShin) {
            joint.set_local_basis1(pa.rotation.inverse() * pb.rotation);
        }
        for axis in [JointAxis::AngX, JointAxis::AngY, JointAxis::AngZ] {
            let limits = match (segment, axis) {
                (Segment::LeftShin | Segment::RightShin, JointAxis::AngX) => [-2.1, 0.12],
                (Segment::LeftShin | Segment::RightShin, _) => [-0.12, 0.12],
                (Segment::LeftForearm | Segment::RightForearm, JointAxis::AngX) => [-1.9, 0.22],
                (Segment::LeftForearm | Segment::RightForearm, _) => [-0.22, 0.22],
                (Segment::Neck | Segment::Head, _) => [-0.40, 0.40],
                (Segment::Abdomen | Segment::Thorax, JointAxis::AngX) => [-0.38, 0.55],
                (Segment::Abdomen | Segment::Thorax, _) => [-0.28, 0.28],
                (
                    Segment::LeftHand | Segment::RightHand | Segment::LeftFoot | Segment::RightFoot,
                    _,
                ) => [-0.38, 0.38],
                (Segment::LeftThigh | Segment::RightThigh, JointAxis::AngX) => [-1.3, 0.9],
                (Segment::LeftThigh | Segment::RightThigh, _) => [-0.55, 0.55],
                _ => [-1.4, 1.4],
            };
            joint.set_limits(axis, limits);
            // Passive joint drag is torque per angular speed (N m s/rad).
            // AccelerationBased scales drag by effective inertia: light distal
            // segments then keep oscillating against a loaded joint limit after
            // the torso has settled. Use physical torque damping, with no spring
            // target, so a configured handoff can still fall to a new rest pose.
            joint.set_motor_model(axis, MotorModel::ForceBased);
            // A foot loaded against a step has a longer contact lever than a
            // wrist. Extra passive ankle drag prevents late limit oscillation
            // after the torso settles, without a positional spring or velocity
            // clamp (and still permits the captured running momentum).
            let drag = if matches!(segment, Segment::LeftFoot | Segment::RightFoot) {
                4.0
            } else {
                2.5
            };
            joint.set_motor_velocity(axis, 0.0, drag);
        }
        joint.set_contacts_enabled(false);
        commands.entity(b).insert(ImpulseJoint::new(a, joint));
    }
    entities
}

pub(super) fn physical_tick(
    time: Res<Time>,
    raid: Res<RaidState>,
    context: ReadRapierContext,
    mut actors: Query<(
        Entity,
        &Client,
        &ClientInput,
        &Participant,
        &mut BodyHealth,
        &mut PoseState,
        &mut MovementState,
        &mut Collider,
        &mut Transform,
        Option<&KinematicCharacterControllerOutput>,
        &Arsenal,
        Has<super::live_body::Body>,
        &mut super::ragdoll::Motion,
    )>,
    pieces: Query<(), With<PhysicsPiece>>,
) {
    let Ok(context) = context.single() else {
        return;
    };
    let dt = time.delta_secs();
    for (
        entity,
        client,
        input,
        p,
        mut body,
        mut pose,
        mut movement,
        mut collider,
        mut t,
        output,
        arsenal,
        articulated,
        mut motion,
    ) in &mut actors
    {
        let aiming = raid.phase == RaidPhase::Active
            && p.status == RaidStatus::Active
            && body.alive()
            && input.aim
            && matches!(pose.mode, PhysicalMode::Standing | PhysicalMode::Stumbling)
            && arsenal.current().mechanics().action().is_none()
            && arsenal.current().mechanics().equipped()
            && arsenal.active == input.weapon
            && body.anatomy.capacity(Region::RightArm) > 0.0
            && !(input.run
                && !movement.prone
                && Vec2::new(pose.velocity[0], pose.velocity[2]).length() > 0.5);
        pose.ads += ((if aiming { 1.0 } else { 0.0 }) - pose.ads) * (1.0 - (-dt * 14.0).exp());
        // Objectives may enter debrief in the same tick as the last fatal hit.
        // Dead actors must still transfer their captured pose, even after active control ends.
        if raid.phase != RaidPhase::Active && !(raid.phase == RaidPhase::Debrief && !body.alive()) {
            continue;
        }
        let previous_mode = pose.mode;
        if p.status != RaidStatus::Active && body.alive() {
            continue;
        }
        if !body.alive() {
            pose.mode_time += dt;
            continue;
        }
        body.anatomy.tick(
            dt,
            input.run && !movement.prone && Vec3::from(pose.velocity).length() > 1.0,
        );
        if articulated {
            pose.mode_time += dt;
            pose.aim_pitch = arsenal.recoil.camera(input.camera).pitch;
            pose.reload_fraction = 0.0;
            pose.lean = pose.lean.map(|x| x * (-dt * 9.0).exp());
            pose.impulse = (Vec3::from(pose.impulse) * (-dt * 8.0).exp()).into();
            continue;
        }
        // Deployment/correction can invalidate last tick's KCC support and motion.
        // Never transfer a pose from a different root after such a discontinuity.
        let relocated = motion.valid && motion.root.translation.distance(t.translation) > 2.0;
        if relocated {
            motion.valid = false;
            movement.grounded = false;
        }
        let effective = output
            .filter(|_| !relocated)
            .map_or(movement.velocity, |o| {
                o.effective_translation / dt.max(0.001)
            });
        let local = t.rotation.inverse() * effective;
        pose.aim_pitch = arsenal.recoil.camera(input.camera).pitch;
        pose.weapon = arsenal.current().spec().class;
        pose.arm_weakness = [
            1.0 - body.anatomy.capacity(Region::LeftArm),
            1.0 - body.anatomy.capacity(Region::RightArm),
        ];
        let mechanics = arsenal.current().mechanics().snapshot();
        pose.reload_fraction = if mechanics.reloading() {
            mechanics.fraction
        } else {
            0.0
        };
        pose.reload_empty = mechanics.kind == Some(common::catalog::ManipulationKind::EmptyReload);
        pose.velocity = local.into();
        pose.grounded = movement.grounded;
        pose.crouch = if movement.crouched { 1.0 } else { 0.0 };
        pose.prone = movement.prone;
        let speed = Vec2::new(local.x, local.z).length();
        if movement.grounded {
            pose.phase =
                (pose.phase + stride_phase_rate(speed, pose.crouch) * dt) % std::f32::consts::TAU;
        }
        pose.mode_time += dt;
        pose.landing = (pose.landing - dt * 4.0).max(0.0);
        let (l, r) = body.anatomy.leg_support();
        let predicate = |hit: Entity, _: &_| hit != entity && pieces.get(hit).is_err();
        let filter = QueryFilter::new()
            .exclude_collider(entity)
            .exclude_sensors()
            .predicate(&predicate);
        let mut supports = [0.0; 2];
        for (i, x) in [-0.13, 0.13].into_iter().enumerate() {
            let origin = t.translation
                + t.rotation
                    * Vec3::new(
                        x,
                        if pose.prone {
                            0.0
                        } else {
                            -0.45 + pose.crouch * 0.3
                        },
                        0.0,
                    );
            if let Some((_, hit)) =
                context.cast_ray_and_get_normal(origin, -Vec3::Y, 0.60, true, filter)
            {
                supports[i] = hit.normal.y.max(0.0) * if i == 0 { l } else { r };
            }
        }
        pose.left_support = supports[0];
        pose.right_support = supports[1];
        let impulse = Vec3::from(pose.impulse);
        let horizontal = Vec2::new(impulse.x, impulse.z).length();
        pose.lean[0] = (pose.lean[0]
            + (local.x * 0.06
                + pose.acceleration[0] * 0.025
                + pose.turn_rate * speed * 0.012
                + (r - l) * 0.5
                + impulse.x * 0.1
                - pose.lean[0])
                * dt
                * 9.0)
            .clamp(-1.0, 1.0);
        pose.lean[1] = (pose.lean[1]
            + (-local.z * 0.045 - pose.acceleration[1] * 0.025 + impulse.z * 0.1 - pose.lean[1])
                * dt
                * 9.0)
            .clamp(-1.0, 1.0);
        if matches!(pose.mode, PhysicalMode::Standing | PhysicalMode::Stumbling) {
            let support_loss = movement.grounded
                && (supports[0] + supports[1] < 0.24 || l.min(r) < 0.15 && speed > 0.25);
            let trunk = (body.anatomy.muscle[Region::Thorax as usize]
                .min(body.anatomy.muscle[Region::Abdomen as usize])
                / 100.0)
                .clamp(0.0, 1.0);
            if horizontal > 2.1 * (0.45 + trunk * 0.55)
                || support_loss
                || body.anatomy.blood < BLOOD_COLLAPSE
            {
                pose.mode = PhysicalMode::Fallen;
                pose.mode_time = 0.0;
                let travel = effective + impulse * 0.6;
                let direction = (t.rotation.inverse() * Vec3::new(travel.x, 0.0, travel.z))
                    .normalize_or(Vec3::new((r - l) * 0.5, 0.0, -1.0).normalize());
                pose.fall_direction = [direction.x, direction.z];
                pose.fall_phase = pose.phase;
                pose.fall_velocity = pose.velocity;
                movement.velocity.x += impulse.x * 0.35;
                movement.velocity.z += impulse.z * 0.35;
                pose.root_drop = 0.60 - pose.crouch * 0.3;
                if movement.prone {
                    use common::stance::Stance;
                    // Preserve the floor plane while handing the voluntary
                    // envelope over to the standard injury fallback.
                    t.translation.y += 0.30 - Stance::Prone.half_height();
                    movement.prone = false;
                    pose.prone = false;
                } else {
                    t.translation.y -= pose.root_drop;
                }
                *collider = Collider::compound(vec![(
                    Vec3::ZERO,
                    Quat::from_rotation_arc(-Vec3::Z, direction),
                    Collider::cuboid(0.35, 0.3, 0.75),
                )]);
            } else if horizontal > 0.3 {
                pose.mode = PhysicalMode::Stumbling;
                pose.mode_time = 0.0;
            } else if pose.mode == PhysicalMode::Stumbling && pose.mode_time > 0.65 {
                pose.mode = PhysicalMode::Standing;
                pose.mode_time = 0.0;
            }
        } else if pose.mode == PhysicalMode::Fallen
            && pose.mode_time > 1.8
            && movement.grounded
            && l + r > 0.7
            && body.anatomy.muscle[Region::Abdomen as usize] > 20.0
            && body.anatomy.blood > BLOOD_COLLAPSE
            && can_occupy_character(
                entity,
                &context,
                t.translation + Vec3::Y * pose.root_drop,
                movement.crouched,
            )
        {
            pose.mode = PhysicalMode::Recovering;
            pose.mode_time = 0.0;
            t.translation.y += pose.root_drop;
            pose.root_drop = 0.0;
            *collider = Collider::capsule_y(
                if movement.crouched {
                    PLAYER_CROUCH_COLLIDER_HALF_HEIGHT
                } else {
                    PLAYER_COLLIDER_HALF_HEIGHT
                },
                PLAYER_COLLIDER_RADIUS,
            );
        } else if pose.mode == PhysicalMode::Recovering && pose.mode_time > 1.0 {
            pose.mode = PhysicalMode::Standing;
            pose.mode_time = 0.0;
            *collider = Collider::capsule_y(
                if movement.crouched {
                    PLAYER_CROUCH_COLLIDER_HALF_HEIGHT
                } else {
                    PLAYER_COLLIDER_HALF_HEIGHT
                },
                PLAYER_COLLIDER_RADIUS,
            );
        }
        pose.impulse = (impulse * (-dt * 8.0).exp()).into();
        if previous_mode != pose.mode {
            info!(
                "PHYSICAL owner={} {:?}->{:?} position={:?} grounded={} support={:.2}/{:.2}",
                client.id,
                previous_mode,
                pose.mode,
                t.translation,
                movement.grounded,
                pose.left_support,
                pose.right_support
            );
        }
    }
}

/// Transfer after impacts and before the snapshot. A fatal-hit snapshot must never
/// hide the controlled actor while omitting its articulated replacement.
pub(super) fn spawn_dead(
    mut commands: Commands,
    raid: Res<RaidState>,
    mut state: ResMut<WorldState>,
    mut actors: Query<
        (
            Entity,
            &Client,
            &BodyHealth,
            &mut PoseState,
            &Transform,
            Has<Bot>,
            &super::ragdoll::Motion,
            &super::ragdoll::Impact,
            Option<&mut super::live_body::Body>,
        ),
        Without<Corpsed>,
    >,
) {
    if !matches!(raid.phase, RaidPhase::Active | RaidPhase::Debrief) {
        return;
    }
    for (entity, client, body, mut pose, root, bot, motion, impact, articulated) in &mut actors {
        if body.alive() {
            continue;
        }
        if let Some(mut articulated) = articulated {
            articulated.resume_dynamic(&mut commands, motion);
            commands.entity(entity).insert(Corpsed);
            pose.mode = PhysicalMode::Dead;
            pose.mode_time = 1.0;
            info!(
                "RAGDOLL LIVE TO DEAD owner={} preserved_segment_ids=true",
                client.id
            );
            continue;
        }
        let mut rig = evaluate_pose(
            &pose,
            body.anatomy.capacity(Region::LeftLeg),
            body.anatomy.capacity(Region::RightLeg),
        );
        if motion.valid {
            for segment in SEGMENTS {
                let world = motion.world[segment.index()];
                rig[segment.index()] = SegmentPose {
                    position: (root.rotation.inverse() * (world.translation - root.translation))
                        .into(),
                    rotation: (root.rotation.inverse() * world.rotation).to_array(),
                };
            }
        }
        let velocities = super::ragdoll::transfer(
            &rig,
            root,
            motion,
            impact,
            body.anatomy.missing,
            root.rotation * Vec3::from(pose.velocity),
        );
        corpse(
            &mut commands,
            &mut state,
            &rig,
            PieceActor {
                root,
                owner: client.id,
                bot,
            },
            &velocities,
            body,
        );
        commands.entity(entity).insert(Corpsed);
        let already_down = matches!(pose.mode, PhysicalMode::Fallen | PhysicalMode::Recovering);
        pose.mode = PhysicalMode::Dead;
        pose.prone = false;
        pose.mode_time = if already_down { 1.0 } else { 0.0 };
        info!(
            "RAGDOLL HANDOFF owner={} root={:?} inherited_pelvis={:?} angular_thigh={:?} impulse={:?} contact={:?}",
            client.id,
            root.translation,
            velocities[0].0,
            velocities[Segment::LeftThigh.index()].1,
            impact.impulse,
            impact.point
        );
    }
}

pub(super) fn expire_pieces(
    mut commands: Commands,
    time: Res<Time>,
    mut pieces: Query<(Entity, &mut PhysicsPiece)>,
    actors: Query<(&Client, &BodyHealth), With<super::live_body::Body>>,
) {
    let living: Vec<_> = actors
        .iter()
        .filter(|(_, body)| body.alive())
        .map(|(client, _)| client.id)
        .collect();
    let mut ordered = Vec::new();
    for (entity, mut p) in &mut pieces {
        p.remaining -= time.delta_secs();
        if p.remaining <= 0.0 {
            commands.entity(entity).despawn();
        } else {
            ordered.push((living.contains(&p.owner) && !p.detached, p.id, entity));
        }
    }
    ordered.sort_by_key(|p| (p.0, p.1));
    for (_, _, entity) in ordered
        .iter()
        .take(ordered.len().saturating_sub(MAX_PIECES))
    {
        commands.entity(*entity).despawn();
    }
}

#[cfg(test)]
mod handoff_tests {
    use super::*;
    #[derive(Resource)]
    struct Fixture {
        rig: [SegmentPose; SEGMENT_COUNT],
        root: Transform,
        velocities: [(Vec3, Vec3); SEGMENT_COUNT],
    }
    #[test]
    fn each_body_starts_at_the_last_pose_with_its_own_inherited_momentum() {
        let rig = evaluate_pose(
            &PoseState {
                grounded: true,
                velocity: [0.0, 0.0, -5.5],
                phase: 1.2,
                ..default()
            },
            1.0,
            1.0,
        );
        let root = Transform::from_xyz(-3.0, 1.018, 7.0).with_rotation(Quat::from_rotation_y(1.1));
        let velocities = std::array::from_fn(|i| {
            (
                root.rotation * -Vec3::Z * 5.5 + Vec3::Y * i as f32 * 0.03,
                Vec3::X * i as f32 * 0.07,
            )
        });
        let mut app = App::new();
        app.init_resource::<WorldState>()
            .insert_resource(Fixture {
                rig,
                root,
                velocities,
            })
            .add_systems(
                Update,
                |mut commands: Commands, mut state: ResMut<WorldState>, f: Res<Fixture>| {
                    corpse(
                        &mut commands,
                        &mut state,
                        &f.rig,
                        PieceActor {
                            root: &f.root,
                            owner: 7,
                            bot: false,
                        },
                        &f.velocities,
                        &BodyHealth::default(),
                    );
                },
            );
        app.update();
        let mut count = 0;
        for (piece, t, velocity) in app
            .world_mut()
            .query::<(&PhysicsPiece, &Transform, &Velocity)>()
            .iter(app.world())
        {
            let i = piece.segment.index();
            let expected = root.mul_transform(rig[i].transform());
            assert!(t.translation.distance(expected.translation) < 0.0001);
            assert!(t.rotation.angle_between(expected.rotation) < 0.001);
            assert!(velocity.linear.distance(velocities[i].0) < 0.0001);
            assert!(velocity.angular.distance(velocities[i].1) < 0.0001);
            count += 1;
        }
        assert_eq!(count, SEGMENT_COUNT);
        assert_eq!(
            app.world_mut()
                .query::<&ImpulseJoint>()
                .iter(app.world())
                .count(),
            SEGMENT_COUNT - 1
        );
    }
}
