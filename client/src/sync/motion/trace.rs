//! Opt-in root presentation telemetry. No transport or camera control.
use std::{
    ffi::OsString,
    fs::{File, OpenOptions},
    io::{BufWriter, Write},
};

use super::*;

const HEADER: &str = "seconds,frame_seconds,player,raid,sequence,snapshot_age_seconds,record_gap_seconds,reset,mode,accepted_x,accepted_y,accepted_z,root_x,root_y,root_z";

#[derive(Resource)]
pub(in crate::sync) struct CameraGapTrace {
    output: BufWriter<File>,
    last: Option<(u64, u32, u64)>,
}

impl CameraGapTrace {
    pub(in crate::sync) fn open(path: OsString) -> Self {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .expect("TK_CAMERA_GAP_TRACE needs a new file in an existing directory");
        let mut output = BufWriter::new(file);
        writeln!(output, "{HEADER}").expect("write camera trace header");
        Self { output, last: None }
    }

    pub(super) fn frame(
        &mut self,
        session: &ClientSession,
        player: u64,
        time: &Time<Real>,
        root: Vec3,
        mode: &str,
    ) {
        let motion = &session.local_motion;
        let Some(accepted) = motion.samples.back() else {
            return;
        };
        let key = (player, session.raid.raid_id, session.last_sequence);
        if key.2 == 0 {
            return;
        }
        let new = self.last.replace(key) != Some(key);
        let age = (time.elapsed_secs_f64() - session.last_received).max(0.0);
        let [x, y, z] = accepted.pose.position.to_array();
        let [rx, ry, rz] = root.to_array();
        let gap = new
            .then_some(motion.record_gap)
            .flatten()
            .unwrap_or(f64::NAN);
        let reset = if new { motion.reset_reason } else { "none" };
        writeln!(self.output,
            "{:.6},{:.6},{player},{},{},{age:.6},{gap:.6},{reset},{mode},{x:.6},{y:.6},{z:.6},{rx:.6},{ry:.6},{rz:.6}",
            time.elapsed_secs_f64(), time.delta_secs_f64(), key.1, key.2,
        ).expect("write camera gap trace");
    }
}
