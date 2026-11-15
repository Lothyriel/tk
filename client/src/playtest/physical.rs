//! Dedicated healthy-character physical review through ordinary UDP movement and shots.
use super::{aim_barrel, capture, move_toward};
use bevy::prelude::*;
use common::{humanoid::*, *};

#[derive(Default)]
pub(super) struct Review {
    pub captures: u8,
    pub previous_alive: Option<ClientData>,
    pub rest: Option<Vec<PhysicsPieceData>>,
}

pub(super) fn scenario() -> Option<String> {
    std::env::var("TK_PHYSICAL_REVIEW").ok()
}

pub(super) fn setup(input: &mut ClientInput, local: &ClientData, peer: bool, name: &str) -> f32 {
    let wall = name.ends_with("wall");
    let target = if peer {
        [-4.0, 1.0, if wall { 18.7 } else { 15.0 }]
    } else {
        [-4.0, 1.0, if wall { 15.2 } else { 18.5 }]
    };
    let distance = move_toward(input, local.pos, target);
    if distance < 0.35 {
        input.forward = false;
        input.backward = false;
        input.left = false;
        input.right = false;
    }
    distance
}

pub(super) fn action(
    commands: &mut Commands,
    review: &mut Review,
    input: &mut ClientInput,
    local: &ClientData,
    other: &ClientData,
    peer: bool,
    name: &str,
    elapsed: f64,
) -> bool {
    let target = if peer { local } else { other };
    if target.body.alive() {
        review.previous_alive = Some(target.clone());
    }
    if peer {
        input.camera = CameraInput {
            yaw: -std::f32::consts::FRAC_PI_2,
            ..default()
        };
        input.forward = name.starts_with("run") && target.body.alive();
        input.run = input.forward;
    } else {
        let rig = evaluate_pose(&target.pose, 1.0, 1.0);
        let velocity = Quat::from_rotation_y(target.rot.yaw) * Vec3::from(target.pose.velocity);
        let head = Vec3::from(target.pos)
            + Quat::from_rotation_y(target.rot.yaw)
                * Vec3::from(rig[Segment::Head.index()].position);
        aim_barrel(input, local, head + velocity * 0.05);
        input.fire = elapsed > 1.0 && target.body.alive();
    }
    if elapsed > 0.8 && review.captures & 1 == 0 {
        capture(commands, &format!("{name}-before"));
        review.captures |= 1;
        info!(
            "PHYSICAL NATIVE BEFORE scenario={name} target={} position={:?} velocity={:?} phase={:.3}",
            target.id, target.pos, target.pose.velocity, target.pose.phase
        );
        if name.starts_with("run") {
            assert!(
                Vec3::from(target.pose.velocity).length() > 4.0,
                "healthy run speed must be reached before the hit"
            );
        }
    }
    if !target.body.alive() {
        let previous = review
            .previous_alive
            .as_ref()
            .expect("Observe living character before physics handoff");
        capture(commands, &format!("{name}-handoff"));
        info!(
            "PHYSICAL NATIVE HANDOFF scenario={name} target={} pre_position={:?} pre_velocity={:?}",
            target.id, previous.pos, previous.pose.velocity
        );
        return true;
    }
    assert!(
        elapsed < 6.0,
        "normal moving anatomical head shots must produce the requested handoff"
    );
    false
}

pub(super) fn rest(
    commands: &mut Commands,
    review: &mut Review,
    input: &mut ClientInput,
    local: &ClientData,
    target: &ClientData,
    pieces: &[PhysicsPieceData],
    peer: bool,
    name: &str,
    elapsed: f64,
) -> bool {
    let owned: Vec<_> = pieces
        .iter()
        .filter(|p| p.owner == target.id && !p.detached)
        .cloned()
        .collect();
    if !peer {
        if let Some(thorax) = owned.iter().find(|p| p.segment == Segment::Thorax) {
            if elapsed > 1.5 {
                let centre = Vec3::from(thorax.pose.position);
                let mut away = Vec3::from(local.pos) - centre;
                away.y = 0.0;
                if away.length() > 3.2 {
                    let target = centre + away.normalize() * 2.8;
                    move_toward(input, local.pos, target.into());
                }
            }
            aim_barrel(input, local, thorax.pose.position.into());
        }
    }
    if elapsed > 0.25 {
        assert_eq!(
            owned.len(),
            SEGMENT_COUNT,
            "all healthy segments must replicate after death"
        );
    }
    if elapsed > 8.0 && review.rest.is_none() {
        capture(commands, &format!("{name}-rest-start"));
        review.rest = Some(owned.clone());
    }
    if elapsed > 10.0 {
        let earlier = review.rest.as_ref().expect("rest comparison sample");
        let mut max_position = 0.0_f32;
        let mut max_angle = 0.0_f32;
        for piece in &owned {
            let previous = earlier
                .iter()
                .find(|p| p.id == piece.id)
                .expect("stable physical segment IDs");
            max_position = max_position
                .max(Vec3::from(piece.pose.position).distance(previous.pose.position.into()));
            max_angle = max_angle.max(
                Quat::from_array(piece.pose.rotation)
                    .angle_between(Quat::from_array(previous.pose.rotation)),
            );
        }
        assert!(
            max_position < 0.02 && max_angle < 0.05,
            "body must remain at rest in real snapshots: {max_position}m/{max_angle}rad"
        );
        let root = std::path::PathBuf::from(std::env::var("TK_PLAYTEST_DIR").unwrap());
        let payload = owned
            .iter()
            .map(|p| {
                format!(
                    "{{\"id\":{},\"segment\":{},\"position\":{:?},\"rotation\":{:?}}}",
                    p.id,
                    p.segment.index(),
                    p.pose.position,
                    p.pose.rotation
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        std::fs::write(root.join("physical-rest.json"), format!("{{\"scenario\":\"{name}\",\"victim\":{},\"max_position_delta\":{max_position},\"max_angle_delta\":{max_angle},\"pieces\":[{payload}]}}", target.id)).unwrap();
        capture(commands, &format!("{name}-rest"));
        info!(
            "PHYSICAL REVIEW PASS scenario={name} target={} pieces={} rest_delta={max_position:.6}m/{max_angle:.6}rad",
            target.id,
            owned.len()
        );
        return true;
    }
    false
}
