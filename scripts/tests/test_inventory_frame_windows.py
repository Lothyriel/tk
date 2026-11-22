"""TAR-77 summary parser and paired analysis, with no client or sampler."""

import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from inventory_frame_windows import analyze


def records(p95=(14, 17, 19, 16, 20, 17, 19, 22, 18, 24, 22, 18)):
    panels = (
        "closed", "open", "open", "closed",
        "open", "closed", "closed", "open",
        "closed", "open", "open", "closed",
    )
    return [
        dict(stage="confirmed_loot", index=index, panel=panel, samples=300,
             revision=42, viewport=[1280, 800], p50_ms=12, p95_ms=p95[index])
        for index, panel in enumerate(panels)
    ]


def log(windows):
    return ["unrelated client output\n", *(
        "TK INVENTORY PERF v1 " + json.dumps(window) + "\n" for window in windows
    )]


class InventoryFrameWindowsTests(unittest.TestCase):
    def test_pairs_open_minus_closed_in_both_orders_and_reports_warnings(self):
        report = analyze(iter(log(records())))
        self.assertEqual(report["revision"], 42)
        self.assertEqual(report["viewport"], [1280, 800])
        self.assertEqual(report["samples_per_window"], 300)
        self.assertEqual(len(report["windows"]), 12)
        self.assertEqual(report["p95"], {
            "blocks": [
                {"pair_deltas_ms": [3, 3], "delta_ms": 3, "closed_drift_ms": 2},
                {"pair_deltas_ms": [3, 3], "delta_ms": 3, "closed_drift_ms": 2},
                {"pair_deltas_ms": [6, 4], "delta_ms": 5, "closed_drift_ms": 0},
            ],
            "median_delta_ms": 3,
            "delta_range_ms": [3, 5],
            "max_abs_closed_drift_ms": 2,
            "inconclusive": False,
        })
        self.assertEqual(report["p50"]["median_delta_ms"], 0)
        self.assertTrue(report["p50"]["inconclusive"])
        self.assertEqual(len(report["warnings"]), 2)
        self.assertIn("+1 ms", report["warnings"][0])
        self.assertIn("20 ms", report["warnings"][1])
        json.dumps(report, allow_nan=False)

    def test_signed_closed_drift_equal_to_negative_delta_is_inconclusive(self):
        report = analyze(log(records((18, 16, 14, 16, 14, 16, 16, 14, 16, 14, 14, 16))))
        self.assertEqual(report["p95"]["median_delta_ms"], -2)
        self.assertEqual(report["p95"]["blocks"][0]["closed_drift_ms"], -2)
        self.assertEqual(report["p95"]["max_abs_closed_drift_ms"], 2)
        self.assertTrue(report["p95"]["inconclusive"])
        self.assertEqual(len(report["warnings"]), 1)
        self.assertIn("inconclusive", report["warnings"][0])

    def test_alert_limits_are_not_exceeded_at_equality(self):
        report = analyze(log(records((19, 20, 20, 19, 20, 19, 19, 20, 19, 20, 20, 19))))
        self.assertEqual(report["p95"]["median_delta_ms"], 1)
        self.assertFalse(report["p95"]["inconclusive"])
        self.assertEqual(report["warnings"], [])

    def test_rejects_missing_duplicate_or_out_of_order_windows(self):
        windows = records()
        for candidate, error in (
            ([], "got 0"),
            (windows[:-1], "got 11"),
            (windows + windows, "extra frame window"),
            ([windows[0], *windows], "index must be 1"),
            ([windows[1], windows[0], *windows[2:]], "index must be 0"),
        ):
            with self.subTest(error=error), self.assertRaisesRegex(ValueError, error):
                analyze(log(candidate))

    def test_rejects_mismatched_window_metadata(self):
        for field, value, error in (
            ("stage", "raid_exit", "stage"),
            ("panel", "closed", "panel"),
            ("samples", 299, "samples"),
            ("samples", 300.0, "samples"),
            ("revision", 43, "revision changed"),
            ("revision", True, "revision"),
            ("viewport", [1280, 720], "viewport"),
            ("viewport", [1280.0, 800], "viewport"),
        ):
            with self.subTest(field=field, value=value), self.assertRaisesRegex(ValueError, error):
                windows = records()
                windows[1][field] = value
                analyze(log(windows))

    def test_rejects_bad_payloads_and_invalid_percentiles(self):
        for line, error in (
            ("TK INVENTORY PERF v2 {}", "version"),
            ("TK INVENTORY PERF v1 {", "invalid JSON"),
            ("TK INVENTORY PERF v1 []", "JSON object"),
            ("TK INVENTORY PERF v1 {}", "fields"),
        ):
            with self.subTest(line=line), self.assertRaisesRegex(ValueError, error):
                analyze([line])
        for value, error in (
            (True, "finite"),
            (0, "finite"),
            (float("inf"), "finite"),
            (float("nan"), "finite"),
            (11, "cannot exceed"),
        ):
            with self.subTest(value=value), self.assertRaisesRegex(ValueError, error):
                windows = records()
                windows[0]["p95_ms"] = value
                analyze(log(windows))
