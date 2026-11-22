import csv
import io
import unittest

from scripts.camera_gap import FIELDS, compare


def frame(t, dt, sequence, *, player=7, raid=1, age=0, gap=0.02,
          reset="none", mode="interpolate", x=1, root=None):
    return [t, dt, player, raid, sequence, age, gap, reset, mode,
            x, 0, 0, x if root is None else root, 0, 0]


def evidence(*, outage=False, player=7):
    rows = [
        frame(0, 0.02, 1, player=player, raid=0, gap="NaN", reset="initial"),
        frame(0.02, 0.02, 2, player=player, reset="lifecycle"),
        frame(0.04, 0.02, 3, player=player),
    ]
    if outage:
        rows.extend([
            frame(0.14, 0.10, 3, player=player, age=0.10, gap="NaN", mode="hold_latest"),
            frame(0.30, 0.16, 3, player=player, age=0.26, gap="NaN", mode="hold_latest"),
            frame(0.34, 0.04, 8, player=player, gap=0.30, reset="arrival_gap", mode="hold_oldest", x=2.25),
            frame(0.36, 0.02, 9, player=player, x=2.3),
        ])
    else:
        rows.append(frame(0.06, 0.02, 4, player=player))
    return rows


def trace(rows):
    output = io.StringIO()
    csv.writer(output).writerows([FIELDS, *rows])
    return output.getvalue().splitlines()


class CameraGapTests(unittest.TestCase):
    def test_consecutive_hold_and_first_fresh_reset(self):
        report = compare(trace(evidence()), trace(evidence(outage=True, player=8)))
        self.assertAlmostEqual(report["control"]["raid_median_fps"], 50)
        recovered = report["recovery"]
        self.assertEqual((recovered["held_frames"], recovered["held_sequence"],
                          recovered["first_fresh_sequence"]), (2, 3, 8))
        self.assertEqual(recovered["last_held_age_seconds"], 0.26)
        self.assertEqual(recovered["record_gap_seconds"], 0.30)
        self.assertEqual(recovered["recovery_displacement_m"], 1.25)

    def test_incomplete_or_mixed_evidence(self):
        for label, change in [
            ("missing frame", lambda rows: rows.pop(3)),
            ("no frames after reset", lambda rows: rows.pop()),
            ("changed player", lambda rows: rows[4].__setitem__(2, 8)),
            ("changed raid", lambda rows: rows[5].__setitem__(3, 2)),
            ("lifecycle reset within raid", lambda rows: rows[5].__setitem__(7, "lifecycle")),
            ("hold position off by one CSV unit", lambda rows: rows[4].__setitem__(12, 1.000001)),
            ("hold interrupted", lambda rows: rows[4].__setitem__(8, "interpolate")),
            ("first fresh frame has no reset", lambda rows: rows[5].__setitem__(7, "none")),
            ("reset did not land on fresh position", lambda rows: rows[5].__setitem__(12, 2.2)),
            ("truncated final row", lambda rows: rows[-1].pop()),
        ]:
            with self.subTest(label=label), self.assertRaises(ValueError):
                rows = evidence(outage=True)
                change(rows)
                compare(trace(evidence()), trace(rows))

    def test_stale_control_is_not_comparable(self):
        with self.assertRaisesRegex(ValueError, "control contains stale"):
            compare(trace(evidence(outage=True)), trace(evidence(outage=True)))
