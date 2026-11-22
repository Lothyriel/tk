"""Measure one stationary crouched lean reversal and return from an observer CSV.

Use read_trace(path) to parse the gait sidecar (roll is confirmed, presented_roll
is sampled from the observer). analyze(rows) returns JSON-compatible evidence;
``measured`` describes coverage, not a visual or motion-quality pass.
"""

import argparse
import csv
import json
import math
from pathlib import Path

SIDES = ("left", "right")
CHEST = tuple(f"chest_q{axis}" for axis in "xyzw")
FLAGS = ("standing", "grounded", "crouched")
INTEGERS = ("snapshot", "actor_id", *FLAGS)
NUMBERS = (
    "seconds", "speed", "roll", "presented_roll", "yaw", "lateral_velocity",
    "left_weight", "right_weight", *CHEST,
    *(f"{side}{suffix}_{axis}" for side in SIDES
      for suffix in ("", "_anchor", "_confirmed") for axis in "xyz"),
)
MAX_GAP_SECONDS = 0.05


def read_trace(path):
    """Return typed rows from the sidecar; extra gait columns are ignored."""
    with Path(path).open(newline="", encoding="utf-8") as stream:
        reader = csv.DictReader(stream)
        missing = set(INTEGERS + NUMBERS) - set(reader.fieldnames or ())
        if missing:
            raise ValueError(f"Missing columns: {', '.join(sorted(missing))}")
        rows = []
        for raw in reader:
            try:
                row = {key: int(raw[key]) for key in INTEGERS}
                row.update({key: float(raw[key]) for key in NUMBERS})
                if not all(math.isfinite(row[key]) for key in NUMBERS):
                    raise ValueError("nonfinite measurement")
                if not all(row[key] in (0, 1) for key in FLAGS):
                    raise ValueError("expected 0/1 posture flags")
                if row["speed"] < 0 or not all(0 <= row[f"{side}_weight"] <= 1 for side in SIDES):
                    raise ValueError("invalid speed or support weight")
                if not 0.99 <= math.hypot(*(row[key] for key in CHEST)) <= 1.01:
                    raise ValueError("expected a unit chest quaternion")
                rows.append(row)
            except (TypeError, ValueError) as error:
                raise ValueError(f"CSV line {reader.line_num}: {error}") from error
    return rows


def _stamp(row):
    return {key: row[key] for key in ("seconds", "snapshot", "roll", "presented_roll")}


def _chest_angle(a, b):
    u, v = ([row[key] for key in CHEST] for row in (a, b))
    cosine = abs(sum(x * y for x, y in zip(u, v))) / (math.hypot(*u) * math.hypot(*v))
    return 2 * math.acos(min(1.0, cosine))


def _window(rows):
    first, last = rows[0], rows[-1]
    pairs = [(a, b, b["seconds"] - a["seconds"]) for a, b in zip(rows, rows[1:])]
    gaps = [{"from": _stamp(a), "to": _stamp(b), "seconds": dt}
            for a, b, dt in pairs
            if dt >= MAX_GAP_SECONDS or math.isclose(dt, MAX_GAP_SECONDS)]
    report = {
        "coverage": "insufficient", "from": _stamp(first), "to": _stamp(last),
        "seconds": last["seconds"] - first["seconds"], "pairs": len(pairs),
        "max_frame_seconds": max(dt for _, _, dt in pairs), "frame_gaps": gaps,
        "max_snapshot_advance": max(b["snapshot"] - a["snapshot"] for a, b, _ in pairs),
        "loaded_feet": {}, "chest": None,
    }
    if gaps:
        return dict(report, reason="frame_gap")
    if not all(row["speed"] < 0.08 and all(row[key] for key in FLAGS) for row in rows):
        return dict(report, reason="not_stationary_grounded_crouch")
    if last["snapshot"] == first["snapshot"]:
        return dict(report, reason="no_new_snapshot")
    a, b, dt = max(pairs, key=lambda pair: _chest_angle(*pair[:2]))
    report["chest"] = {
        "peak_pair_rad": _chest_angle(a, b), "pair_seconds": dt,
        "from": _stamp(a), "to": _stamp(b),
    }
    for side in SIDES:
        if not all(row[f"{side}_weight"] > 0.95 for row in rows):
            continue

        def excursion(row, prefix):
            return math.dist(*(tuple(r[f"{prefix}_{axis}"] for axis in "xyz") for r in (first, row)))

        peak = max(rows[1:], key=lambda row: excursion(row, side))
        report["loaded_feet"][side] = {
            "peak_at": _stamp(peak), "foot_m": excursion(peak, side),
            "presented_anchor_m": excursion(peak, f"{side}_anchor"),
            "confirmed_anchor_m": excursion(peak, f"{side}_confirmed"),
        }
    if not report["loaded_feet"]:
        return dict(report, reason="no_continuously_loaded_foot")
    return dict(report, coverage="measured")


def analyze(rows):
    """Measure the first ordered left→right→neutral sequence in parsed rows.

    Both windows must have a continuously loaded foot in common. Snapshot jumps
    are reported, not treated as frame gaps, and presentation can change on a
    repeated snapshot. No drift or angular quality threshold is inferred.
    """
    rows = list(rows)
    report = {"coverage": "insufficient", "actor_id": None, "windows": {}, "common_feet": []}
    if not rows:
        return dict(report, reason="no_rows")
    report["actor_id"] = rows[0]["actor_id"]
    if any(row["actor_id"] != report["actor_id"] for row in rows):
        raise ValueError("expected one observer target actor")
    for a, b in zip(rows, rows[1:]):
        if a["seconds"] >= b["seconds"] or a["snapshot"] > b["snapshot"]:
            raise ValueError("expected increasing frame time and nondecreasing snapshot sequence")
        if a["snapshot"] == b["snapshot"] and any(
            a[k] != b[k] for k in ("roll", "speed", "yaw", "lateral_velocity", *FLAGS)
        ):
            raise ValueError("confirmed state changed without a new snapshot")

    # Match main's StopLean observer gates, including the strict inequalities.
    stages = (
        ("left", lambda r: r["roll"] < -0.29 and r["presented_roll"] < -0.27),
        ("right", lambda r: r["roll"] > 0.29 and r["presented_roll"] > 0.27),
        ("neutral", lambda r: abs(r["roll"]) < 0.001 and abs(r["presented_roll"]) < 0.01),
    )
    start = -1
    for (stage, at_stage), window in zip(stages, (None, "reversal", "return")):
        end = next((i for i in range(start + 1, len(rows)) if at_stage(rows[i])), None)
        if end is None:
            return dict(report, reason=f"missing_{stage}")
        if window:
            report["windows"][window] = _window(rows[start:end + 1])
        start = end

    for name, window in report["windows"].items():
        if window["coverage"] != "measured":
            return dict(report, reason=f"{name}:{window['reason']}")
    common = set(report["windows"]["reversal"]["loaded_feet"]) & set(
        report["windows"]["return"]["loaded_feet"]
    )
    if not common:
        return dict(report, reason="no_common_loaded_foot")
    return dict(report, coverage="measured", common_feet=sorted(common))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trace", type=Path)
    args = parser.parse_args()
    try:
        report = analyze(read_trace(args.trace))
    except (OSError, ValueError) as error:
        parser.error(str(error))
    print(json.dumps(report, indent=2))
    return 0 if report["coverage"] == "measured" else 1


if __name__ == "__main__":
    raise SystemExit(main())
