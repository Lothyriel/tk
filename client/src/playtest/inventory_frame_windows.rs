//! Standalone TAR-77 sampler. The route owns input, the timeout, and checkpoint
//! validation. Feed it one observation per frame starting at confirmed loot:
//! the real elapsed interval and the panel state of the frame that just ended,
//! not the state after this frame's toggle. Validate focus, revision, viewport,
//! and the rest of the checkpoint through the end of that interval.
//!
//! Drive the normal panel action only when `requested_panel()` differs from
//! the observed state; wait for the observed transition before sampling. On
//! completion, emit `summaries()` via `Display` at column one, without a logger
//! prefix. Do not emit anything for an interrupted or invalid run.

use std::fmt;

const VIEWPORT: [u32; 2] = [1280, 800];
const DISCARD_FRAMES: usize = 120;
const SAMPLES: usize = 300;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    Closed,
    Open,
}

use Panel::{Closed, Open};

const WARMUP: [Panel; 2] = [Closed, Open];
const ORDER: [Panel; 12] = [
    Closed, Open, Open, Closed, // ABBA
    Open, Closed, Closed, Open, // BAAB
    Closed, Open, Open, Closed, // ABBA
];

pub struct Observation {
    pub panel: Panel,
    /// False if fixed checkpoint conditions (raid, view, pose, cursor, UI
    /// scale, no pending inventory operation, etc.) no longer hold.
    pub checkpoint_valid: bool,
    pub focused: bool,
    pub viewport: [u32; 2],
    pub revision: u64,
    pub frame_ms: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidRun {
    Checkpoint,
    Focus,
    Viewport,
    Revision,
    PanelChanged,
    FrameTime,
}

pub struct WindowSummary {
    index: usize,
    panel: Panel,
    revision: u64,
    p50_ms: f64,
    p95_ms: f64,
}

/// One complete record for `scripts/inventory_frame_windows.py`.
impl fmt::Display for WindowSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let panel = match self.panel {
            Closed => "closed",
            Open => "open",
        };
        write!(
            f,
            concat!(
                "TK INVENTORY PERF v1 ",
                "{{\"stage\":\"confirmed_loot\",\"index\":{},\"panel\":\"{}\",",
                "\"samples\":{},\"revision\":{},\"viewport\":[{},{}],",
                "\"p50_ms\":{},\"p95_ms\":{}}}"
            ),
            self.index,
            panel,
            SAMPLES,
            self.revision,
            VIEWPORT[0],
            VIEWPORT[1],
            self.p50_ms,
            self.p95_ms,
        )
    }
}

pub struct Sampler {
    revision: u64,
    step: usize,
    discard: usize,
    samples: Vec<f64>,
    windows: Vec<WindowSummary>,
    invalid: Option<InvalidRun>,
}

impl Sampler {
    pub fn new(revision: u64) -> Self {
        Self {
            revision,
            step: 0,
            discard: DISCARD_FRAMES,
            samples: Vec::with_capacity(SAMPLES),
            windows: Vec::with_capacity(ORDER.len()),
            invalid: None,
        }
    }

    /// `None` once complete or invalidated.
    pub fn requested_panel(&self) -> Option<Panel> {
        if self.invalid.is_some() {
            return None;
        }
        if self.step < WARMUP.len() {
            return Some(WARMUP[self.step]);
        }
        ORDER.get(self.step - WARMUP.len()).copied()
    }

    /// Returns a sticky error if the run is invalid. A wrong panel state is
    /// tolerated only while waiting for the first match after a state request.
    /// A bad steady interval invalidates the run instead of censoring samples.
    pub fn observe(&mut self, frame: Observation) -> Result<(), InvalidRun> {
        if let Some(reason) = self.invalid {
            return Err(reason);
        }
        let Some(panel) = self.requested_panel() else {
            return Ok(());
        };
        let invalid = if !frame.checkpoint_valid {
            Some(InvalidRun::Checkpoint)
        } else if !frame.focused {
            Some(InvalidRun::Focus)
        } else if frame.viewport != VIEWPORT {
            Some(InvalidRun::Viewport)
        } else if frame.revision != self.revision {
            Some(InvalidRun::Revision)
        } else if frame.panel != panel && self.discard < DISCARD_FRAMES {
            Some(InvalidRun::PanelChanged)
        } else if frame.panel == panel
            && self.discard == 0
            && (!frame.frame_ms.is_finite() || frame.frame_ms <= 0.0)
        {
            Some(InvalidRun::FrameTime)
        } else {
            None
        };
        if let Some(reason) = invalid {
            self.invalid = Some(reason);
            return Err(reason);
        }
        if frame.panel != panel {
            // The preceding frame still had the old state. Do not spend any
            // of the discard budget until the requested state is observed.
            return Ok(());
        }
        if self.discard != 0 {
            self.discard -= 1;
            if self.discard == 0 && self.step < WARMUP.len() {
                self.advance(panel);
            }
            return Ok(());
        }

        self.samples.push(frame.frame_ms);
        if self.samples.len() == SAMPLES {
            self.samples.sort_unstable_by(f64::total_cmp);
            self.windows.push(WindowSummary {
                index: self.windows.len(),
                panel,
                revision: self.revision,
                // Nearest rank, converted from 1-based to 0-based indices.
                p50_ms: self.samples[149],
                p95_ms: self.samples[284],
            });
            self.samples.clear();
            self.advance(panel);
        }
        Ok(())
    }

    /// Available only once all 12 windows are complete; no partial summaries.
    pub fn summaries(&self) -> Option<&[WindowSummary]> {
        (self.invalid.is_none() && self.windows.len() == ORDER.len())
            .then_some(self.windows.as_slice())
    }

    fn advance(&mut self, previous: Panel) {
        self.step += 1;
        // Consecutive windows with the same panel state have no transition.
        if self.requested_panel().is_some_and(|next| next != previous) {
            self.discard = DISCARD_FRAMES;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(panel: Panel, frame_ms: f64) -> Observation {
        Observation {
            panel,
            checkpoint_valid: true,
            focused: true,
            viewport: [1280, 800],
            revision: 42,
            frame_ms,
        }
    }

    fn feed(sampler: &mut Sampler, panel: Panel, count: usize, ms: f64) {
        for _ in 0..count {
            sampler.observe(frame(panel, ms)).unwrap();
        }
    }

    fn warm(sampler: &mut Sampler) {
        feed(sampler, Closed, 120, 1.0);
        feed(sampler, Open, 120, 1.0);
    }

    #[test]
    fn waits_for_observation_then_discards_120_frames_in_each_warmup() {
        let mut sampler = Sampler::new(42);
        feed(&mut sampler, Open, 130, 1.0);
        feed(&mut sampler, Closed, 119, 1.0);
        assert_eq!(sampler.requested_panel(), Some(Closed));
        feed(&mut sampler, Closed, 1, 1.0);
        assert_eq!(sampler.requested_panel(), Some(Open));
        feed(&mut sampler, Closed, 130, 1.0);
        feed(&mut sampler, Open, 119, 1.0);
        assert_eq!(sampler.requested_panel(), Some(Open));
        feed(&mut sampler, Open, 1, 1.0);
        assert_eq!(sampler.requested_panel(), Some(Closed));
        assert!(sampler.summaries().is_none());
    }

    #[test]
    fn summarizes_exact_order_and_nearest_rank_without_transition_frames() {
        let mut sampler = Sampler::new(42);
        warm(&mut sampler);
        let order = [
            Closed, Open, Open, Closed, Open, Closed, Closed, Open, Closed, Open, Open, Closed,
        ];
        let mut previous = Open;
        for panel in order {
            assert!(sampler.summaries().is_none());
            assert_eq!(sampler.requested_panel(), Some(panel));
            if panel != previous {
                feed(&mut sampler, previous, 3, 5000.0);
                feed(&mut sampler, panel, 120, 6000.0);
            }
            for ms in (1..=300).rev() {
                sampler.observe(frame(panel, f64::from(ms))).unwrap();
            }
            previous = panel;
        }
        assert_eq!(sampler.requested_panel(), None);
        let summaries = sampler.summaries().unwrap();
        assert_eq!(summaries.len(), 12);
        for (index, (&panel, summary)) in order.iter().zip(summaries).enumerate() {
            assert_eq!(summary.index, index);
            assert_eq!(summary.panel, panel);
            assert_eq!(summary.revision, 42);
            assert_eq!(summary.p50_ms, 150.0);
            assert_eq!(summary.p95_ms, 285.0);
        }
        assert_eq!(
            summaries[0].to_string(),
            concat!(
                "TK INVENTORY PERF v1 ",
                "{\"stage\":\"confirmed_loot\",\"index\":0,\"panel\":\"closed\",",
                "\"samples\":300,\"revision\":42,\"viewport\":[1280,800],",
                "\"p50_ms\":150,\"p95_ms\":285}",
            )
        );
    }

    #[test]
    fn bad_steady_intervals_invalidate_instead_of_silently_dropping_samples() {
        for invalid_ms in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let mut sampler = Sampler::new(42);
            warm(&mut sampler);
            feed(&mut sampler, Closed, 120, 1.0);
            assert_eq!(
                sampler.observe(frame(Closed, invalid_ms)),
                Err(InvalidRun::FrameTime)
            );
            assert_eq!(
                sampler.observe(frame(Closed, 1.0)),
                Err(InvalidRun::FrameTime)
            );
            assert_eq!(sampler.requested_panel(), None);
            assert!(sampler.summaries().is_none());
        }
    }

    #[test]
    fn wrong_state_during_settle_or_inside_adjacent_same_state_windows_fails() {
        let mut sampler = Sampler::new(42);
        warm(&mut sampler);
        feed(&mut sampler, Closed, 1, 1.0);
        assert_eq!(
            sampler.observe(frame(Open, 1.0)),
            Err(InvalidRun::PanelChanged)
        );

        let mut sampler = Sampler::new(42);
        warm(&mut sampler);
        feed(&mut sampler, Closed, 420, 1.0);
        feed(&mut sampler, Open, 420, 1.0);
        assert_eq!(sampler.requested_panel(), Some(Open));
        assert_eq!(
            sampler.observe(frame(Closed, 1.0)),
            Err(InvalidRun::PanelChanged)
        );
        assert!(sampler.summaries().is_none());
    }

    #[test]
    fn checks_caller_guards_even_while_waiting_for_panel() {
        for (bad_frame, reason) in [
            (
                Observation {
                    checkpoint_valid: false,
                    ..frame(Open, 1.0)
                },
                InvalidRun::Checkpoint,
            ),
            (
                Observation {
                    focused: false,
                    ..frame(Open, 1.0)
                },
                InvalidRun::Focus,
            ),
            (
                Observation {
                    viewport: [1280, 720],
                    ..frame(Open, 1.0)
                },
                InvalidRun::Viewport,
            ),
            (
                Observation {
                    revision: 43,
                    ..frame(Open, 1.0)
                },
                InvalidRun::Revision,
            ),
        ] {
            let mut sampler = Sampler::new(42);
            assert_eq!(sampler.observe(bad_frame), Err(reason));
            assert!(sampler.summaries().is_none());
        }
    }
}
