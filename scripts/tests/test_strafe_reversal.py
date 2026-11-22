import csv
import json
import math
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from scripts.strafe_reversal import analyze


def sample(seconds, snapshot, velocity, foot, anchor, confirmed, turn=0.0, sign=1):
    row = dict(seconds=seconds, snapshot=snapshot, actor_id=7, lateral_velocity=velocity,
               yaw=0.0, standing=1, grounded=1, crouched=0, left_weight=0.99,
               right_weight=0.25, chest_qx=0, chest_qy=sign * math.sin(turn / 2),
               chest_qz=0, chest_qw=sign * math.cos(turn / 2))
    for prefix, x in (("left", foot), ("left_anchor", anchor), ("left_confirmed", confirmed),
                      ("right", 0), ("right_anchor", 0), ("right_confirmed", 0)):
        row.update({f"{prefix}_x": x, f"{prefix}_y": 0, f"{prefix}_z": 0})
    return row


def trace():
    return [
        sample(0.00, 10, 2, 0.000, 0.000, 0.000),
        sample(0.01, 10, 2, 0.003, 0.002, 0.000),
        sample(0.03, 12, 0, 0.008, 0.004, 0.001, turn=0.15),
        sample(0.04, 13, -1, 0.014, 0.007, 0.002, turn=0.15, sign=-1),
        sample(0.05, 14, -2, 0.009, 0.010, 0.003, turn=0.15),
    ]


class StrafeReversalTests(unittest.TestCase):
    def test_measures_matched_endpoints_and_sign_invariant_chest_turn(self):
        report = analyze(trace())
        self.assertEqual(report["coverage"], "measured")
        bracket = report["bracket"]
        self.assertEqual((bracket["from"]["snapshot"], bracket["to"]["snapshot"]), (10, 13))
        self.assertEqual(bracket["from"]["seconds"], 0.01)
        self.assertAlmostEqual(bracket["seconds"], 0.03)
        self.assertEqual(bracket["max_snapshot_advance"], 2)
        self.assertEqual(bracket["frame_gaps"], [])
        self.assertAlmostEqual(report["chest"]["peak_pair_rad"], 0.15)
        self.assertEqual(report["chest"]["to"]["snapshot"], 12)
        interval, = report["loaded_intervals"]
        self.assertEqual((interval["side"], interval["pairs"]), ("left", 4))
        self.assertEqual(interval["peak_at"]["snapshot"], 13)
        self.assertAlmostEqual(interval["foot_m"], 0.014)
        self.assertAlmostEqual(interval["presented_anchor_m"], 0.007)
        self.assertAlmostEqual(interval["confirmed_anchor_m"], 0.002)

    def test_missing_sign_or_eligible_support_is_not_measured(self):
        self.assertEqual(analyze(trace()[:3])["reason"], "no_positive_to_negative_bracket")
        for flag, value in (("standing", 0), ("grounded", 0), ("crouched", 1)):
            with self.subTest(flag=flag):
                rows = trace()
                rows[2][flag] = value
                self.assertEqual(analyze(rows)["reason"], "ineligible_stance_in_bracket")
        rows = trace()
        rows[2]["left_weight"] = 0.95
        self.assertEqual(analyze(rows)["reason"], "no_continuously_loaded_foot_across_bracket")
        rows[2]["yaw"] = 0.1
        self.assertEqual(analyze(rows)["reason"], "yaw_not_fixed")
        rows = trace()
        rows[2]["snapshot"] = 10
        self.assertEqual(analyze(rows)["reason"], "velocity_changed_without_new_snapshot")

    def test_frame_gap_cannot_bridge_support_but_cuts_interval_outside_bracket(self):
        rows = trace()
        for row in rows[2:]:
            row["seconds"] += 0.1
        report = analyze(rows)
        self.assertEqual(report["coverage"], "insufficient")
        self.assertEqual(report["reason"], "frame_gap_in_bracket")
        gap, = report["bracket"]["frame_gaps"]
        self.assertAlmostEqual(gap["seconds"], 0.12)
        self.assertIsNone(report["chest"])
        self.assertEqual(report["loaded_intervals"], [])
        rows = trace()
        rows[2]["seconds"], rows[3]["seconds"], rows[4]["seconds"] = 0.06, 0.07, 0.08
        self.assertEqual(analyze(rows)["reason"], "frame_gap_in_bracket")
        rows = trace()
        rows[-1]["seconds"] += 0.1
        interval, = analyze(rows)["loaded_intervals"]
        self.assertEqual(interval["to"]["snapshot"], 13)

    def test_cli_distinguishes_measured_missing_and_invalid_csv(self):
        script = Path(__file__).resolve().parents[1] / "strafe_reversal.py"
        rows = trace()
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "gait.csv"
            for count, status in ((5, 0), (3, 1), (0, 1)):
                with self.subTest(count=count), path.open("w", newline="") as stream:
                    writer = csv.DictWriter(stream, fieldnames=[*rows[0], "unused"])
                    writer.writeheader()
                    writer.writerows(rows[:count])
                    stream.flush()
                    result = subprocess.run([sys.executable, str(script), str(path)],
                                            capture_output=True, text=True)
                    self.assertEqual(result.returncode, status, result.stderr)
                    self.assertEqual(json.loads(result.stdout)["coverage"],
                                     "measured" if status == 0 else "insufficient")
            path.write_text("seconds,snapshot\n0,1\n", encoding="utf-8")
            result = subprocess.run([sys.executable, str(script), str(path)],
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, 2)
            self.assertIn("Missing columns:", result.stderr)
