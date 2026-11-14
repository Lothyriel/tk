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

fn spawn_piece(
    commands: &mut Commands,
    state: &mut WorldState,
    segment: Segment,
    pose: SegmentPose,
    root: &Transform,
    velocity: Vec3,
    owner: u64,
    bot: bool,
    detached: bool,
) -> Entity {
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
            Velocity {
                linear: velocity,
                angular: if detached {
                    Vec3::new(2.0, 1.0, 0.5)
                } else {
                    Vec3::ZERO
                },
            },
            Damping {
                linear_damping: 0.8,
                angular_damping: 2.5,
            },
            Friction::coefficient(0.8),
            Restitution::coefficient(0.05),
            // Debris hits the map but not actors or other pieces. Avoid self-collision explosions.
            CollisionGroups::new(Group::GROUP_2, Group::GROUP_1),
            ColliderMassProperties::Density(400.0),
            Ccd::enabled(),
        ))
        .id()
}

pub(super) fn spawn_fragments(
    commands: &mut Commands,
    state: &mut WorldState,
    mask: u32,
    rig: &[SegmentPose; SEGMENT_COUNT],
    root: &Transform,
    velocity: Vec3,
    owner: u64,
    bot: bool,
) {
    for segment in SEGMENTS {
        if mask & (1 << segment.index()) != 0 {
            spawn_piece(
                commands,
                state,
                segment,
                rig[segment.index()],
                root,
                velocity,
                owner,
                bot,
                true,
            );
        }
    }
}

fn corpse(
    commands: &mut Commands,
    state: &mut WorldState,
    rig: &[SegmentPose; SEGMENT_COUNT],
    root: &Transform,
    velocity: Vec3,
    body: &BodyHealth,
    owner: u64,
    bot: bool,
) {
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
            root,
            velocity,
            owner,
            bot,
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
        let anchor = (pa.translation + pb.translation) * 0.5;
        let mut joint = GenericJoint::new(JointAxesMask::LIN_AXES);
        joint.set_local_anchor1(pa.rotation.inverse() * (anchor - pa.translation));
        joint.set_local_anchor2(pb.rotation.inverse() * (anchor - pb.translation));
        joint.set_local_basis1(pa.rotation.inverse() * pb.rotation);
        for axis in [JointAxis::AngX, JointAxis::AngY, JointAxis::AngZ] {
            let limit = if matches!(segment, Segment::Neck | Segment::Head) {
                0.45
            } else {
                1.1
            };
            joint.set_limits(axis, [-limit, limit]);
            joint.set_motor_velocity(axis, 0.0, 1.5);
        }
        joint.set_contacts_enabled(false);
        commands.entity(b).insert(ImpulseJoint::new(a, joint));
    }
}

pub(super) fn physical_tick(
    mut commands: Commands,
    time: Res<Time>,
    raid: Res<RaidState>,
    context: ReadRapierContext,
    mut state: ResMut<WorldState>,
    mut actors: Query<(
        Entity,
        &Client,
        &ClientInput,
        &Participant,
        &mut BodyHealth,
        &mut PoseState,
        &MovementState,
        &mut Collider,
        &mut Transform,
        Has<Bot>,
        Has<Corpsed>,
        Option<&KinematicCharacterControllerOutput>,
        &Arsenal,
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
        movement,
        mut collider,
        mut t,
        bot,
        corpsed,
        output,
        arsenal,
    ) in &mut actors
    {
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
            if !corpsed {
                let rig = evaluate_pose(
                    &pose,
                    body.anatomy.capacity(Region::LeftLeg),
                    body.anatomy.capacity(Region::RightLeg),
                );
                corpse(
                    &mut commands,
                    &mut state,
                    &rig,
                    &t,
                    t.rotation * Vec3::from(pose.velocity) + Vec3::from(pose.impulse),
                    &body,
                    client.id,
                    bot,
                );
                commands.entity(entity).insert(Corpsed);
                let already_down =
                    matches!(pose.mode, PhysicalMode::Fallen | PhysicalMode::Recovering);
                pose.mode = PhysicalMode::Dead;
                pose.mode_time = if already_down { 1.0 } else { 0.0 };
            }
            continue;
        }
        body.anatomy
            .tick(dt, input.run && Vec3::from(pose.velocity).length() > 1.0);
        let effective = output.map_or(movement.velocity, |o| {
            o.effective_translation / dt.max(0.001)
        });
        let local = t.rotation.inverse() * effective;
        pose.aim_pitch = input.camera.pitch;
        pose.weapon = arsenal.active;
        pose.arm_weakness = [
            1.0 - body.anatomy.capacity(Region::LeftArm),
            1.0 - body.anatomy.capacity(Region::RightArm),
        ];
        pose.reload_fraction = if arsenal.reload_remaining > 0.0 {
            1.0 - arsenal.reload_remaining / arsenal.active.spec().reload_seconds
        } else {
            0.0
        };
        pose.velocity = local.into();
        pose.grounded = movement.grounded;
        pose.crouch = if movement.crouched { 1.0 } else { 0.0 };
        let speed = Vec2::new(local.x, local.z).length();
        if movement.grounded {
            let stride = 0.28 - pose.crouch * 0.10;
            pose.phase = (pose.phase + speed * dt * std::f32::consts::PI / (2.0 * stride))
                % std::f32::consts::TAU;
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
            let origin = t.translation + t.rotation * Vec3::new(x, -0.45 + pose.crouch * 0.3, 0.0);
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
            + (local.x * 0.08 + (r - l) * 0.5 + impulse.x * 0.1 - pose.lean[0]) * dt * 9.0)
            .clamp(-1.0, 1.0);
        pose.lean[1] = (pose.lean[1]
            + (-local.z * 0.06 + impulse.z * 0.1 - pose.lean[1]) * dt * 9.0)
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
                pose.root_drop = 0.60 - pose.crouch * 0.3;
                t.translation.y -= pose.root_drop;
                *collider = Collider::cuboid(0.35, 0.3, 0.75);
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

pub(super) fn expire_pieces(
    mut commands: Commands,
    time: Res<Time>,
    mut pieces: Query<(Entity, &mut PhysicsPiece)>,
) {
    let mut ordered = Vec::new();
    for (entity, mut p) in &mut pieces {
        p.remaining -= time.delta_secs();
        if p.remaining <= 0.0 {
            commands.entity(entity).despawn();
        } else {
            ordered.push((p.id, entity));
        }
    }
    ordered.sort_by_key(|p| p.0);
    for (_, entity) in ordered
        .iter()
        .take(ordered.len().saturating_sub(MAX_PIECES))
    {
        commands.entity(*entity).despawn();
    }
}
