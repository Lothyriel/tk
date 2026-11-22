//! Walk onto the published west step using ordinary player input.
use super::step_observer::{Confirmed, Contact, Gate, Geometry, Presented, Sample, Stamp};
use bevy::prelude::*;
use common::{
    CameraInput, ClientData, ClientInput,
    humanoid::{PhysicalMode, Segment},
    map::{BLOCKS, MapBlock},
};
use std::time::Duration;

fn step() -> &'static MapBlock {
    BLOCKS
        .iter()
        .find(|block| block.center[0] == -14.0 && block.center[2] == 2.0)
        .expect("published west step")
}

#[derive(Default)]
pub(super) struct Route {
    stage: u8,
    stopped_since: Option<f64>,
}

impl Route {
    pub(super) fn update(
        &mut self,
        elapsed: f64,
        actor: &ClientData,
        input: &mut ClientInput,
    ) -> bool {
        let [x, _, z] = actor.pos;
        let [target_x, target_y, target_z] = step().center;
        input.camera = CameraInput::default();
        match self.stage {
            0 if z <= 8.5 => self.stage = 1, // Leave the adjacent spawn lane.
            1 if x <= target_x => self.stage = 2,
            2 if z <= target_z + 0.13 => self.stage = 3,
            _ => (),
        }
        input.left = self.stage == 1;
        input.forward = matches!(self.stage, 0 | 2);
        if self.stage == 3 {
            let topo = target_y + step().half_size[1];
            assert!(
                standing(actor)
                    && actor.pos[1] > topo + 0.85
                    && (x - target_x).abs() < step().half_size[0]
                    && (z - target_z).abs() < step().half_size[2],
                "leader did not stay on the west step: {:?}",
                actor.pos,
            );
            // Keep sending snapshots while the observer presents the contact.
            elapsed - *self.stopped_since.get_or_insert(elapsed) > 2.0
        } else {
            false
        }
    }
}

fn standing(actor: &ClientData) -> bool {
    actor.alive
        && actor.pose.mode == PhysicalMode::Standing
        && actor.pose.grounded
        && !actor.crouched
        && !actor.pose.prone
}

pub(super) fn observe(
    driver: Res<super::Driver>,
    time: Res<Time<Real>>,
    session: Res<crate::sync::ClientSession>,
    id: Res<common::PlayerId>,
    poses: Res<crate::render::actors::PoseCache>,
    mut gate: Local<Option<Gate>>,
    mut exit: MessageWriter<AppExit>,
) {
    if driver.step != super::Step::Pov {
        return;
    }
    let Some(actor) = session.players.iter().find(|p| p.id != id.0 && !p.bot) else {
        // Don't bridge a missing actor with cached presentation.
        *gate = None;
        return;
    };
    let gate = gate.get_or_insert_with(|| {
        let [x, y, z] = step().center;
        let [hx, hy, hz] = step().half_size;
        Gate::new(
            actor.id,
            Geometry {
                floor_y: BLOCKS[0].center[1] + BLOCKS[0].half_size[1],
                top_y: y + hy,
                x: [x - hx, x + hx],
                z: [z - hz, z + hz],
                foot_center_offset: 0.055,
                tolerance: 0.020,
            },
            Duration::from_millis(250),
        )
    });
    let root = Transform::from_translation(actor.pos.into())
        .with_rotation(Quat::from_rotation_y(actor.rot.yaw));
    let presented = poses
        .presented_support(actor.id)
        .zip(poses.presented_foot_anchors(actor.id));
    let sample = Sample {
        actor: actor.id,
        stamp: Stamp {
            time: time.elapsed(),
            snapshot: session.last_sequence,
        },
        confirmed: Confirmed {
            eligible: standing(actor),
            feet: std::array::from_fn(|i| Contact {
                weight: actor.pose.foot_weight[i],
                anchor: root.transform_point(actor.pose.feet[i].into()).to_array(),
            }),
        },
        presented: [Segment::LeftFoot, Segment::RightFoot].map(|segment| {
            let i = usize::from(segment == Segment::RightFoot);
            let ((_, weights), anchors) = presented?;
            Some(Presented {
                contact: Contact {
                    weight: weights[i],
                    anchor: anchors[i].to_array(),
                },
                foot_center: poses
                    .presented_bone(actor.id, segment)?
                    .translation
                    .to_array(),
            })
        }),
    };
    if let Some(proof) = gate
        .observe(sample)
        .expect("ordered west step observations")
    {
        info!(
            "WEST STEP CONTACT actor={} foot={} floor={} confirmed={} presented={} floor_ms={} confirmed_ms={} presented_ms={}",
            proof.actor,
            proof.foot,
            proof.floor.snapshot,
            proof.confirmed_top.snapshot,
            proof.presented_top.snapshot,
            proof.floor.time.as_millis(),
            proof.confirmed_top.time.as_millis(),
            proof.presented_top.time.as_millis()
        );
        info!("POV OBSERVER PASS");
        exit.write(AppExit::Success);
    }
}
