"""Analyze TAR-77 solo inventory frame-window summaries (no capture or sampler).

Pass a single client's text log to ``analyze(lines)``. After a successful run,
emit one line per completed window, in order, starting at column one:

TK INVENTORY PERF v1 {"stage":"confirmed_loot","index":0,"panel":"closed","samples":300,"revision":5,"viewport":[1280,800],"p50_ms":12.2,"p95_ms":15.3}

Send exactly 12 windows, indexed 0–11, in ABBA / BAAB / ABBA order (A closed,
B open), at physical 1280×800 with one confirmed inventory revision. The
producer must warm both conditions, discard 120 frames after each transition,
and only summarize 300 steady-state Time<Real> intervals tagged by the panel
state of the preceding frame. Use nearest-rank percentiles (sorted samples
150 and 285, one-based). The producer must omit the result if the checkpoint
changes or is invalidated (focus, pose/view, cursor, revision, pending
operation, etc.). This module cannot verify those conditions, UI scale 1.0,
or same-process provenance from summaries alone.

Each block has two disjoint adjacent open-minus-closed pairs. Average the
pair deltas within each block, then report the median and range across the
three blocks. Closed drift is last-minus-first within each block; the largest
absolute drift is the noise floor. This is a heuristic, not a significance
test. The p95 alerts are warnings, not acceptance gates.
"""

import argparse
import json
import math
import statistics
import sys

TAG = "TK INVENTORY PERF "
PREFIX = TAG + "v1 "
STAGE = "confirmed_loot"
VIEWPORT = [1280, 800]
SAMPLES = 300
ORDER = "ABBABAABABBA"
PANEL = {"A": "closed", "B": "open"}
FIELDS = {"stage", "index", "panel", "samples", "revision", "viewport", "p50_ms", "p95_ms"}


def _windows(lines):
    windows = []
    for line_number, line in enumerate(lines, 1):
        if not line.startswith(TAG):
            continue
        index = len(windows)

        def invalid(reason):
            return ValueError(f"line {line_number}, window {index}: {reason}")

        if not line.startswith(PREFIX):
            raise invalid("unsupported frame-window log version")
        if index >= len(ORDER):
            raise invalid("extra frame window; expected one run")
        try:
            window = json.loads(line[len(PREFIX):])
        except ValueError as error:
            raise invalid("invalid JSON") from error
        if not isinstance(window, dict):
            raise invalid("expected a JSON object")
        if window.keys() != FIELDS:
            raise invalid("unexpected frame-window fields")
        if window.get("stage") != STAGE:
            raise invalid(f"stage must be {STAGE}")
        if type(window.get("index")) is not int or window["index"] != index:
            raise invalid(f"index must be {index}")
        if window.get("panel") != PANEL[ORDER[index]]:
            raise invalid(f"panel must be {PANEL[ORDER[index]]}")
        if type(window.get("samples")) is not int or window["samples"] != SAMPLES:
            raise invalid(f"samples must be {SAMPLES}")
        revision = window.get("revision")
        if type(revision) is not int or revision < 0:
            raise invalid("revision must be a nonnegative integer")
        if windows and revision != windows[0]["revision"]:
            raise invalid("inventory revision changed")
        viewport = window.get("viewport")
        if (not isinstance(viewport, list) or viewport != VIEWPORT
                or any(type(dimension) is not int for dimension in viewport)):
            raise invalid("viewport must be [1280, 800] physical pixels")
        for metric in ("p50_ms", "p95_ms"):
            value = window.get(metric)
            if type(value) not in (int, float) or not 0 < value <= sys.float_info.max:
                raise invalid(f"{metric} must be positive and finite")
        if window["p50_ms"] > window["p95_ms"]:
            raise invalid("p50_ms cannot exceed p95_ms")
        windows.append(window)
    if len(windows) != len(ORDER):
        raise ValueError(f"expected {len(ORDER)} frame windows, got {len(windows)}")
    return windows


def _metric(windows, key):
    blocks = []
    for start in range(0, len(windows), 4):
        values = windows[start:start + 4]
        pairs = []
        for offset in (0, 2):
            first, second = values[offset:offset + 2]
            opened, closed = (first, second) if first["panel"] == "open" else (second, first)
            pairs.append(opened[key] - closed[key])
        closed = [window[key] for window in values if window["panel"] == "closed"]
        blocks.append({
            "pair_deltas_ms": pairs,
            "delta_ms": statistics.mean(pairs),
            "closed_drift_ms": closed[1] - closed[0],
        })
    deltas = [block["delta_ms"] for block in blocks]
    median = statistics.median(deltas)
    noise = max(abs(block["closed_drift_ms"]) for block in blocks)
    # Treat near-equality as equality despite floating-point subtraction.
    return {
        "blocks": blocks,
        "median_delta_ms": median,
        "delta_range_ms": [min(deltas), max(deltas)],
        "max_abs_closed_drift_ms": noise,
        "inconclusive": abs(median) <= noise or math.isclose(abs(median), noise),
    }


def analyze(lines):
    """Return a JSON-serializable report; raise ValueError for invalid logs."""
    windows = _windows(lines)
    p50 = _metric(windows, "p50_ms")
    p95 = _metric(windows, "p95_ms")
    warnings = []
    if p95["inconclusive"]:
        warnings.append("p95 delta is inconclusive: closed drift matches or exceeds it")
    if p95["median_delta_ms"] > 1.0:
        warnings.append("median paired p95 delta exceeds the provisional +1 ms alert")
    if any(window["p95_ms"] > 20.0 for window in windows):
        warnings.append("a window p95 exceeds the provisional 20 ms solo context alert")
    return {
        "stage": STAGE,
        "revision": windows[0]["revision"],
        "viewport": windows[0]["viewport"],
        "samples_per_window": SAMPLES,
        "windows": windows,
        "p50": p50,
        "p95": p95,
        "warnings": warnings,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("log", type=argparse.FileType("r", encoding="utf-8"))
    with parser.parse_args().log as log:
        try:
            report = analyze(log)
        except ValueError as error:
            parser.error(str(error))
    print(json.dumps(report, indent=2, allow_nan=False))


if __name__ == "__main__":
    main()
