import csv
import math
from pathlib import Path
import tempfile
import unittest

from scripts.crouched_lean import analyze, read_trace


def trace():
    rows = []
    # Presentation catches up to both confirmed extremes on repeated snapshots.
    for i, (snapshot, roll, presented, angle, sign, foot) in enumerate([
        (10, -.30, -.20, 0, 1, 0),
        (10, -.30, -.28, 0, 1, 0),
        (12, 0, -.10, .2, 1, .014),
        (13, .30, .20, .2, -1, .009),
        (13, .30, .28, .2, -1, .008),
        (15, 0, .010, .1, 1, .012),
        (15, 0, .009, .1, 1, .009),
    ]):
        row = dict(seconds=i * .01, snapshot=snapshot, actor_id=7, roll=roll,
                   presented_roll=presented, yaw=0, speed=0, lateral_velocity=0,
                   standing=1, grounded=1, crouched=1, left_weight=.99, right_weight=.99,
                   chest_qx=0, chest_qy=sign * math.sin(angle / 2), chest_qz=0,
                   chest_qw=sign * math.cos(angle / 2))
        for side in ("left", "right"):
            for suffix, x in (("", foot), ("_anchor", foot / 2), ("_confirmed", 0)):
                row.update({f"{side}{suffix}_x": x, f"{side}{suffix}_y": 0,
                            f"{side}{suffix}_z": 0})
        rows.append(row)
    return rows


class CrouchedLeanTests(unittest.TestCase):
    def test_measures_both_windows_with_matched_endpoints(self):
        report = analyze(trace())
        self.assertEqual(report["coverage"], "measured")
        self.assertEqual(report["common_feet"], ["left", "right"])
        reversal = report["windows"]["reversal"]
        returned = report["windows"]["return"]
        self.assertEqual(reversal["from"]["seconds"], .01)
        self.assertEqual(reversal["to"]["seconds"], .04)
        self.assertEqual(returned["to"]["seconds"], .06)
        self.assertEqual(reversal["max_snapshot_advance"], 2)
        self.assertAlmostEqual(reversal["chest"]["peak_pair_rad"], .2)
        self.assertAlmostEqual(returned["chest"]["peak_pair_rad"], .1)
        foot = reversal["loaded_feet"]["left"]
        self.assertEqual(foot["peak_at"]["snapshot"], 12)
        self.assertAlmostEqual(foot["foot_m"], .014)
        self.assertAlmostEqual(foot["presented_anchor_m"], .007)
        self.assertEqual(foot["confirmed_anchor_m"], 0)
        self.assertAlmostEqual(returned["loaded_feet"]["left"]["foot_m"], .004)

    def test_missing_extremes_and_return_are_not_coverage(self):
        rows = trace()
        rows[1]["presented_roll"] = -.27
        self.assertEqual(analyze(rows)["reason"], "missing_left")
        rows = trace()
        rows[4]["presented_roll"] = .27
        self.assertEqual(analyze(rows)["reason"], "missing_right")
        self.assertEqual(analyze(trace()[:-1])["reason"], "missing_neutral")

    def test_gapped_and_ineligible_windows_cannot_be_bridged(self):
        rows = trace()
        for row in rows[5:]:
            row["seconds"] += .04
        report = analyze(rows)
        self.assertEqual(report["reason"], "return:frame_gap")
        self.assertEqual(report["windows"]["reversal"]["coverage"], "measured")
        self.assertAlmostEqual(report["windows"]["return"]["frame_gaps"][0]["seconds"], .05)
        for key, value in (("speed", .08), ("grounded", 0), ("standing", 0), ("crouched", 0)):
            with self.subTest(key=key):
                rows = trace()
                rows[2][key] = value
                self.assertEqual(analyze(rows)["reason"], "reversal:not_stationary_grounded_crouch")

    def test_requires_continuous_support_on_same_foot(self):
        rows = trace()
        rows[2]["right_weight"] = .95
        rows[5]["left_weight"] = .95
        self.assertEqual(analyze(rows)["reason"], "no_common_loaded_foot")
        rows[2]["left_weight"] = .95
        self.assertEqual(analyze(rows)["reason"], "reversal:no_continuously_loaded_foot")

    def test_rejects_mixed_actor_unordered_or_changed_snapshot(self):
        for index, key, value, message in (
            (2, "actor_id", 8, "one observer"),
            (2, "seconds", .01, "increasing frame"),
            (2, "snapshot", 9, "nondecreasing snapshot"),
            (1, "roll", -.31, "without a new snapshot"),
        ):
            with self.subTest(key=key):
                rows = trace()
                rows[index][key] = value
                with self.assertRaisesRegex(ValueError, message):
                    analyze(rows)

    def test_reader_tolerates_extra_gait_fields_and_requires_presented_roll(self):
        rows = trace()
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "lean.csv"
            with path.open("w", newline="") as stream:
                writer = csv.DictWriter(stream, fieldnames=[*rows[0], "phase", "pelvis_y"])
                writer.writeheader()
                writer.writerows(rows)
            self.assertEqual(analyze(read_trace(path))["coverage"], "measured")
            path.write_text(path.read_text().replace("presented_roll", "other_roll"))
            with self.assertRaisesRegex(ValueError, "Missing columns: presented_roll"):
                read_trace(path)
