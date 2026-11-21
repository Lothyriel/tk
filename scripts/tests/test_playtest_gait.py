import csv
from pathlib import Path
import tempfile
import unittest

from scripts.playtest import gait_metrics


class GaitMetricsTests(unittest.TestCase):
    def test_reports_walk_and_run_without_counting_unloaded_feet(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "gait.csv"
            fields = ["seconds", "snapshot", "speed"]
            for side in ("left", "right"):
                fields.extend(f"{side}_{suffix}" for suffix in ("weight", "x", "y", "z"))
            with path.open("w", newline="", encoding="utf-8") as stream:
                writer = csv.writer(stream)
                writer.writerow(fields)
                for i in range(32):
                    writer.writerow([
                        i / 60, i, 3.25 if i < 16 else 5.5,
                        1.0, i * 0.001, 0, 0, 0.1, i, 0, 0,
                    ])
            metrics = gait_metrics(path)
            for speed in ("walk", "run"):
                self.assertEqual(metrics[speed]["loaded_pairs"], 15)
                self.assertAlmostEqual(metrics[speed]["max_displacement_m"], 0.001)
