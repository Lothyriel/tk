"""Measure one right-to-left observer reversal from a normalized gait CSV."""

import argparse
import csv
import json
import math
from pathlib import Path

MAX_PAIR_SECONDS = 0.05
LOADED_WEIGHT = 0.95
YAW_TOLERANCE_RAD = 0.001
SIDES = ("left", "right")
CHEST_AXES = ("qx", "qy", "qz", "qw")


def point(row, prefix, axes="xyz"):
    return tuple(row[f"{prefix}_{axis}"] for axis in axes)


def read_trace(path):
    """Read required fields; ignore additional columns from the gait producer."""
    numbers = (
        "seconds", "lateral_velocity", "yaw", "left_weight", "right_weight",
        *(f"chest_q{axis}" for axis in "xyzw"),
        *(f"{side}{suffix}_{axis}" for side in SIDES
          for suffix in ("", "_anchor", "_confirmed") for axis in "xyz"),
    )
    integers = ("snapshot", "actor_id", "standing", "grounded", "crouched")
    with path.open(newline="", encoding="utf-8") as stream:
        reader = csv.DictReader(stream)
        missing = set(numbers + integers) - set(reader.fieldnames or ())
        if missing:
            raise ValueError(f"Missing columns: {', '.join(sorted(missing))}")
        rows = []
        for raw in reader:
            try:
                row = {key: float(raw[key]) for key in numbers}
                row.update({key: int(raw[key]) for key in integers})
                if not all(math.isfinite(row[key]) for key in numbers):
                    raise ValueError("nonfinite measurement")
                if not all(row[key] in (0, 1) for key in integers[2:]):
                    raise ValueError("expected 0/1 state flags")
                if not all(0 <= row[f"{side}_weight"] <= 1 for side in SIDES):
                    raise ValueError("support weight outside [0, 1]")
                if not 0.99 <= math.hypot(*point(row, "chest", CHEST_AXES)) <= 1.01:
                    raise ValueError("expected a unit chest quaternion")
                rows.append(row)
            except (TypeError, ValueError) as error:
                raise ValueError(f"CSV line {reader.line_num}: {error}") from error
    return rows


def endpoint(row):
    return {key: row[key] for key in ("seconds", "snapshot", "lateral_velocity")}


def chest_angle(a, b):
    a, b = point(a, "chest", CHEST_AXES), point(b, "chest", CHEST_AXES)
    cosine = abs(sum(x * y for x, y in zip(a, b))) / (math.hypot(*a) * math.hypot(*b))
    return 2 * math.acos(min(1.0, cosine))


def analyze(rows):
    """Analyze the first positive-to-negative bracket, without bridging bad rows."""
    report = {"coverage": "insufficient", "actor_id": None, "bracket": None,
              "loaded_intervals": [], "chest": None}
    if not rows:
        return dict(report, reason="no_rows")
    first = rows[0]
    report["actor_id"] = first["actor_id"]
    if any(row["actor_id"] != first["actor_id"] for row in rows):
        raise ValueError("expected one observer target actor")
    if any(a["seconds"] >= b["seconds"] or a["snapshot"] > b["snapshot"]
           for a, b in zip(rows, rows[1:])):
        raise ValueError("expected increasing frame time and nondecreasing snapshot sequence")
    if any(abs(math.remainder(row["yaw"] - first["yaw"], math.tau)) > YAW_TOLERANCE_RAD
           for row in rows):
        return dict(report, reason="yaw_not_fixed")

    start = None
    for stop, row in enumerate(rows):
        if row["lateral_velocity"] > 0:
            start = stop
        elif row["lateral_velocity"] < 0 and start is not None:
            break
    else:
        return dict(report, reason="no_positive_to_negative_bracket")

    def dt(index):
        return rows[index + 1]["seconds"] - rows[index]["seconds"]

    def contiguous(index):
        # Treat a nominal 50 ms decimal interval as a gap even if subtraction
        # rounded it just below the threshold.
        delta = dt(index)
        return delta < MAX_PAIR_SECONDS and not math.isclose(delta, MAX_PAIR_SECONDS)

    def standing(index):
        row = rows[index]
        return row["standing"] and row["grounded"] and not row["crouched"]

    gaps = [{"from": endpoint(rows[i]), "to": endpoint(rows[i + 1]), "seconds": dt(i)}
            for i in range(start, stop) if not contiguous(i)]
    jumps = [rows[i + 1]["snapshot"] - rows[i]["snapshot"] for i in range(start, stop)]
    report["bracket"] = {
        "from": endpoint(rows[start]), "to": endpoint(rows[stop]),
        "seconds": rows[stop]["seconds"] - rows[start]["seconds"],
        "pairs": stop - start,
        "max_frame_seconds": max(dt(i) for i in range(start, stop)),
        "frame_gaps": gaps,
        "max_snapshot_advance": max(jumps),
    }
    if any(rows[i]["lateral_velocity"] != rows[i + 1]["lateral_velocity"]
           and rows[i]["snapshot"] == rows[i + 1]["snapshot"] for i in range(start, stop)):
        return dict(report, reason="velocity_changed_without_new_snapshot")
    if not all(standing(i) for i in range(start, stop + 1)):
        return dict(report, reason="ineligible_stance_in_bracket")
    if gaps:
        return dict(report, reason="frame_gap_in_bracket")

    peak_turn = max(range(start, stop), key=lambda i: chest_angle(rows[i], rows[i + 1]))
    report["chest"] = {
        "peak_pair_rad": chest_angle(rows[peak_turn], rows[peak_turn + 1]),
        "from": endpoint(rows[peak_turn]), "to": endpoint(rows[peak_turn + 1]),
        "pair_seconds": dt(peak_turn),
    }

    for side in SIDES:
        def loaded(index):
            return standing(index) and rows[index][f"{side}_weight"] > LOADED_WEIGHT

        if not all(loaded(i) for i in range(start, stop + 1)):
            continue
        lo, hi = start, stop
        while lo > 0 and loaded(lo - 1) and contiguous(lo - 1):
            lo -= 1
        while hi + 1 < len(rows) and loaded(hi + 1) and contiguous(hi):
            hi += 1

        def displacement(i, prefix):
            return math.dist(point(rows[lo], prefix), point(rows[i], prefix))

        peak = max(range(lo + 1, hi + 1), key=lambda i: displacement(i, side))
        report["loaded_intervals"].append({
            "side": side, "from": endpoint(rows[lo]), "to": endpoint(rows[hi]),
            "pairs": hi - lo, "max_frame_seconds": max(dt(i) for i in range(lo, hi)),
            "peak_at": endpoint(rows[peak]),
            "foot_m": displacement(peak, side),
            "presented_anchor_m": displacement(peak, f"{side}_anchor"),
            "confirmed_anchor_m": displacement(peak, f"{side}_confirmed"),
        })
    if not report["loaded_intervals"]:
        return dict(report, reason="no_continuously_loaded_foot_across_bracket")
    return dict(report, coverage="measured")


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
