//! Pure evidence gate for one remote actor approaching a step. No route or map dependency.
use std::time::Duration;

#[derive(Clone, Copy)]
pub(super) struct Geometry {
    pub floor_y: f32,
    pub top_y: f32,
    pub x: [f32; 2],
    pub z: [f32; 2],
    pub foot_center_offset: f32,
    pub tolerance: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Stamp {
    pub time: Duration,
    pub snapshot: u64,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Contact {
    pub weight: f32,
    pub anchor: [f32; 3],
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Confirmed {
    /// Standing, grounded, and not crouched or prone.
    pub eligible: bool,
    /// Left, then right; both anchors are in world space.
    pub feet: [Contact; 2],
}

#[derive(Clone, Copy)]
pub(super) struct Presented {
    pub contact: Contact,
    pub foot_center: [f32; 3],
}

pub(super) struct Sample {
    pub actor: u64,
    pub stamp: Stamp,
    pub confirmed: Confirmed,
    /// Left, then right, in world space. An unavailable cache entry is not evidence.
    pub presented: [Option<Presented>; 2],
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Proof {
    pub actor: u64,
    pub foot: usize,
    pub floor: Stamp,
    pub confirmed_top: Stamp,
    pub presented_top: Stamp,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum InvalidSample {
    Order,
    ChangedSnapshot,
}

#[derive(Clone, Copy, Default)]
enum Progress {
    #[default]
    Empty,
    Floor(Stamp),
    Top {
        floor: Stamp,
        top: Stamp,
    },
}

pub(super) struct Gate {
    actor: u64,
    geometry: Geometry,
    /// Upper bound on both the frame interval and the time without a new snapshot.
    max_gap: Duration,
    last: Option<(Stamp, Confirmed)>,
    fresh_at: Duration,
    feet: [Progress; 2],
}

impl Gate {
    pub(super) fn new(actor: u64, geometry: Geometry, max_gap: Duration) -> Self {
        assert!(
            [
                geometry.floor_y,
                geometry.top_y,
                geometry.foot_center_offset,
                geometry.tolerance,
                geometry.x[0],
                geometry.x[1],
                geometry.z[0],
                geometry.z[1],
            ]
            .into_iter()
            .all(f32::is_finite)
        );
        assert!(
            geometry.tolerance > 0.0
                && geometry.top_y - geometry.floor_y > 2.0 * geometry.tolerance
        );
        assert!(geometry.x[0] < geometry.x[1] && geometry.z[0] < geometry.z[1]);
        assert!(!max_gap.is_zero());
        Self {
            actor,
            geometry,
            max_gap,
            last: None,
            fresh_at: Duration::ZERO,
            feet: [Progress::Empty; 2],
        }
    }

    /// Feed every target frame, including unloaded/invalid contacts. Repeated
    /// snapshot IDs may update presentation only. Same-frame top evidence is valid.
    pub(super) fn observe(&mut self, sample: Sample) -> Result<Option<Proof>, InvalidSample> {
        if sample.actor != self.actor {
            return Ok(None);
        }
        let stamp = sample.stamp;
        let fresh = if let Some((last, confirmed)) = self.last {
            if stamp.time <= last.time || stamp.snapshot < last.snapshot {
                self.feet = [Progress::Empty; 2];
                return Err(InvalidSample::Order);
            }
            if stamp.snapshot == last.snapshot && sample.confirmed != confirmed {
                self.feet = [Progress::Empty; 2];
                return Err(InvalidSample::ChangedSnapshot);
            }
            if stamp.time - last.time >= self.max_gap || stamp.time - self.fresh_at >= self.max_gap
            {
                self.feet = [Progress::Empty; 2];
            }
            stamp.snapshot > last.snapshot
        } else {
            true
        };
        self.last = Some((stamp, sample.confirmed));
        if fresh {
            self.fresh_at = stamp.time;
        }
        if !sample.confirmed.eligible || stamp.time - self.fresh_at >= self.max_gap {
            self.feet = [Progress::Empty; 2];
            return Ok(None);
        }

        let g = self.geometry;
        let inside =
            |[x, _, z]: [f32; 3]| (g.x[0]..=g.x[1]).contains(&x) && (g.z[0]..=g.z[1]).contains(&z);
        let near = |point: [f32; 3], surface: f32| {
            point.into_iter().all(f32::is_finite)
                && (point[1] - (surface + g.foot_center_offset)).abs() <= g.tolerance
        };
        let on_top = |point| inside(point) && near(point, g.top_y);
        let loaded = |weight: f32| weight > 0.95 && weight <= 1.0;

        for (foot, progress) in self.feet.iter_mut().enumerate() {
            let contact = sample.confirmed.feet[foot];
            let top = loaded(contact.weight) && on_top(contact.anchor);
            let floor = loaded(contact.weight)
                && !inside(contact.anchor)
                && near(contact.anchor, g.floor_y);
            *progress = match *progress {
                _ if fresh && floor => Progress::Floor(stamp),
                Progress::Floor(floor) if top && stamp.snapshot > floor.snapshot => {
                    Progress::Top { floor, top: stamp }
                }
                Progress::Top { .. } if !top => Progress::Empty,
                previous => previous,
            };
            if let Progress::Top { floor, top } = *progress
                && sample.presented[foot].is_some_and(|p| {
                    loaded(p.contact.weight) && on_top(p.contact.anchor) && on_top(p.foot_center)
                })
            {
                self.feet = [Progress::Empty; 2];
                return Ok(Some(Proof {
                    actor: self.actor,
                    foot,
                    floor,
                    confirmed_top: top,
                    presented_top: stamp,
                }));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLOOR: [f32; 3] = [0.0, 0.055, 2.0];
    const TOP: [f32; 3] = [0.0, 0.255, 0.0];

    fn gate() -> Gate {
        Gate::new(
            7,
            Geometry {
                floor_y: 0.0,
                top_y: 0.2,
                x: [-1.0, 1.0],
                z: [-1.0, 1.0],
                foot_center_offset: 0.055,
                tolerance: 0.02,
            },
            Duration::from_millis(50),
        )
    }

    fn contact(anchor: [f32; 3]) -> Contact {
        Contact {
            weight: 0.99,
            anchor,
        }
    }

    fn row(ms: u64, snapshot: u64, feet: [[f32; 3]; 2], presented: [bool; 2]) -> Sample {
        Sample {
            actor: 7,
            stamp: Stamp {
                time: Duration::from_millis(ms),
                snapshot,
            },
            confirmed: Confirmed {
                eligible: true,
                feet: feet.map(contact),
            },
            presented: presented.map(|top| {
                top.then_some(Presented {
                    contact: contact(TOP),
                    foot_center: TOP,
                })
            }),
        }
    }

    #[test]
    fn west_step_observer_accepts_the_same_foot_with_presentation_lag() {
        let mut gate = gate();
        let floor = row(0, 1, [FLOOR; 2], [false; 2]);
        let top = row(16, 3, [TOP, FLOOR], [false; 2]);
        let presented = row(32, 3, [TOP, FLOOR], [true, false]);
        let expected = Proof {
            actor: 7,
            foot: 0,
            floor: floor.stamp,
            confirmed_top: top.stamp,
            presented_top: presented.stamp,
        };
        assert_eq!(gate.observe(floor), Ok(None));
        assert_eq!(gate.observe(top), Ok(None));
        assert_eq!(gate.observe(presented), Ok(Some(expected)));
    }

    #[test]
    fn west_step_observer_requires_a_complete_loaded_same_actor_and_side_chain() {
        let mut gate = gate();
        assert_eq!(
            gate.observe(row(0, 1, [TOP, FLOOR], [true, false])),
            Ok(None)
        );
        let mut other = row(16, 2, [FLOOR; 2], [false; 2]);
        other.actor = 8;
        assert_eq!(gate.observe(other), Ok(None));
        assert_eq!(gate.observe(row(32, 3, [TOP; 2], [true, false])), Ok(None));
        let mut unloaded = row(48, 3, [TOP; 2], [false, true]);
        unloaded.presented[1].as_mut().unwrap().contact.weight = 0.95;
        assert_eq!(gate.observe(unloaded), Ok(None));
        let mut wrong_center = row(64, 4, [TOP; 2], [false, true]);
        wrong_center.presented[1].as_mut().unwrap().foot_center = FLOOR;
        assert_eq!(gate.observe(wrong_center), Ok(None));
    }

    #[test]
    fn west_step_observer_does_not_reuse_lost_or_expired_top_contact() {
        let mut lost = gate();
        assert_eq!(lost.observe(row(0, 1, [FLOOR; 2], [false; 2])), Ok(None));
        assert_eq!(lost.observe(row(16, 2, [TOP, FLOOR], [false; 2])), Ok(None));
        let mut air = row(32, 3, [TOP, FLOOR], [true, false]);
        air.confirmed.feet[0].weight = 0.0;
        assert_eq!(lost.observe(air), Ok(None));
        assert_eq!(
            lost.observe(row(48, 4, [TOP, FLOOR], [true, false])),
            Ok(None)
        );

        let mut stale = gate();
        assert_eq!(stale.observe(row(0, 1, [FLOOR; 2], [false; 2])), Ok(None));
        assert_eq!(
            stale.observe(row(16, 2, [TOP, FLOOR], [false; 2])),
            Ok(None)
        );
        for ms in [32, 48, 64] {
            assert_eq!(
                stale.observe(row(ms, 2, [TOP, FLOOR], [false; 2])),
                Ok(None)
            );
        }
        assert_eq!(
            stale.observe(row(66, 2, [TOP, FLOOR], [true, false])),
            Ok(None)
        );
        assert_eq!(
            stale.observe(row(80, 3, [TOP, FLOOR], [true, false])),
            Ok(None)
        );
    }

    #[test]
    fn west_step_observer_rejects_disordered_or_mutated_snapshots() {
        let mut gate = gate();
        assert_eq!(gate.observe(row(16, 2, [FLOOR; 2], [false; 2])), Ok(None));
        assert_eq!(
            gate.observe(row(16, 3, [TOP; 2], [true; 2])),
            Err(InvalidSample::Order)
        );
        assert_eq!(
            gate.observe(row(32, 1, [TOP; 2], [true; 2])),
            Err(InvalidSample::Order)
        );
        assert_eq!(
            gate.observe(row(32, 2, [TOP; 2], [true; 2])),
            Err(InvalidSample::ChangedSnapshot)
        );
        assert_eq!(gate.observe(row(48, 3, [TOP; 2], [true; 2])), Ok(None));
    }

    #[test]
    fn west_step_observer_rejects_off_tread_and_resets_on_a_gap() {
        let mut gate = gate();
        let outside = [2.0, TOP[1], TOP[2]];
        assert_eq!(gate.observe(row(0, 1, [FLOOR; 2], [false; 2])), Ok(None));
        assert_eq!(
            gate.observe(row(16, 2, [outside, FLOOR], [true, false])),
            Ok(None)
        );
        assert_eq!(
            gate.observe(row(66, 3, [TOP, FLOOR], [true, false])),
            Ok(None)
        );
        let mut invalid = row(82, 4, [FLOOR; 2], [false; 2]);
        invalid.confirmed.eligible = false;
        assert_eq!(gate.observe(invalid), Ok(None));
        assert_eq!(gate.observe(row(98, 5, [TOP; 2], [true; 2])), Ok(None));
        assert_eq!(gate.observe(row(114, 6, [FLOOR; 2], [false; 2])), Ok(None));
        let proof = gate
            .observe(row(130, 7, [TOP; 2], [true; 2]))
            .unwrap()
            .unwrap();
        assert_eq!(proof.confirmed_top, proof.presented_top);
    }
}
