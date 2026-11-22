import csv
from pathlib import Path
import tempfile
import unittest

from scripts.playtest import gait_metrics


def write_trace(path, rows):
    fields = ["seconds", "snapshot", "speed", "left_weight", "right_weight"]
    for side in ("left", "right"):
        for suffix in ("", "_anchor", "_confirmed"):
            fields.extend(f"{side}{suffix}_{axis}" for axis in "xyz")
    with path.open("w", newline="", encoding="utf-8") as stream:
        writer = csv.DictWriter(stream, fieldnames=fields)
        writer.writeheader()
        for row in rows:
            writer.writerow(dict.fromkeys(fields, 0) | row)


class GaitMetricsTests(unittest.TestCase):
    def test_reports_walk_and_run_without_counting_unloaded_feet(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "gait.csv"
            write_trace(path, ({
                "seconds": i / 60, "snapshot": i, "speed": 3.25 if i < 16 else 5.5,
                "left_weight": 0.98 if i in (1, 17) else 1.0, "left_x": i * 0.001,
                "left_anchor_x": i * 0.0002, "right_weight": 0.1, "right_x": i,
            } for i in range(32)))
            metrics = gait_metrics(path)
            for speed in ("walk", "run"):
                self.assertEqual(metrics[speed]["loaded_pairs"], 15)
                self.assertAlmostEqual(metrics[speed]["max_displacement_m"], 0.001)
                transitions = metrics[speed]["by_weight_transition"]
                self.assertEqual(
                    {phase: metrics["pairs"] for phase, metrics in transitions.items()},
                    {"loading": 1, "steady": 13, "releasing": 1},
                )
                excursion = metrics[speed]["peak_support_excursion"]
                self.assertEqual(excursion["pairs"], 15)
                self.assertEqual(excursion["side"], "left")
                self.assertAlmostEqual(excursion["foot_m"], 0.015)
                self.assertAlmostEqual(excursion["presented_anchor_m"], 0.003)
                self.assertEqual(excursion["confirmed_anchor_m"], 0.0)

    def test_excursions_restart_after_unloading_and_long_frame_gaps(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "gait.csv"
            write_trace(path, ({
                "seconds": i / 60 + (0.1 if i >= 23 else 0.0),
                "snapshot": i // 2,
                "speed": 3.25 if i < 16 else 5.5,
                "left_weight": 0.4 if i == 7 else 1.0,
                "left_x": i * 0.001 + (1.0 if i >= 8 else 0.0) + (1.0 if i >= 23 else 0.0),
                "left_confirmed_x": i * 0.0005,
            } for i in range(32)))
            metrics = gait_metrics(path)
            for speed, start, end in (("walk", 8, 15), ("run", 23, 31)):
                excursion = metrics[speed]["peak_support_excursion"]
                self.assertEqual(excursion["start_snapshot"], start // 2)
                self.assertEqual(excursion["end_snapshot"], end // 2)
                self.assertEqual(excursion["pairs"], end - start)
                self.assertAlmostEqual(excursion["foot_m"], (end - start) * 0.001)
                self.assertAlmostEqual(excursion["confirmed_anchor_m"], (end - start) * 0.0005)
