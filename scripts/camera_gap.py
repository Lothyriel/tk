"""Verify saved solo camera-gap CSVs; this does not run or classify network tests."""

import argparse
import csv
from dataclasses import dataclass
import json
import math
from pathlib import Path
from statistics import median


FIELDS = ("seconds", "frame_seconds", "player", "raid", "sequence", "snapshot_age_seconds",
          "record_gap_seconds", "reset", "mode", "accepted_x", "accepted_y", "accepted_z",
          "root_x", "root_y", "root_z")
STALE_SECONDS = 0.250
# Each of the three terms in the frame continuity check is rounded to 6 places.
ROUNDING = 0.000002


@dataclass(frozen=True)
class Frame:
    seconds: float
    dt: float
    player: int
    raid: int
    sequence: int
    age: float
    gap: float
    reset: str
    mode: str
    confirmed: tuple[float, float, float]
    root: tuple[float, float, float]


def read_raid(lines):
    reader = csv.reader(lines, strict=True)
    if tuple(next(reader, ())) != FIELDS:
        raise ValueError("unexpected camera-gap header")
    raid = []
    prev = None
    for line, row in enumerate(reader, 2):
        if len(row) != len(FIELDS):
            raise ValueError(f"row {line}: incomplete camera frame")
        t, dt, player, epoch, seq, age, gap, reset, mode, *xyz = row
        x, y, z, rx, ry, rz = map(float, xyz)
        frame = Frame(float(t), float(dt), int(player), int(epoch), int(seq), float(age),
                      float(gap), reset, mode, (x, y, z), (rx, ry, rz))
        if (not all(map(math.isfinite, (frame.seconds, frame.dt, frame.age, x, y, z, rx, ry, rz)))
                or frame.seconds < 0 or frame.dt <= 0 or frame.age < 0
                or frame.player <= 0 or frame.raid < 0 or frame.sequence <= 0
                or not (math.isnan(frame.gap) or math.isfinite(frame.gap) and frame.gap >= 0)
                or frame.mode not in {"hold_latest", "hold_oldest", "interpolate"}):
            raise ValueError(f"row {line}: invalid camera frame")
        if prev is None:
            if frame.reset != "initial" or not math.isnan(frame.gap):
                raise ValueError("missing initial sample")
        else:
            if frame.player != prev.player or frame.sequence < prev.sequence:
                raise ValueError(f"row {line}: player changed or sequence moved backward")
            elapsed = frame.seconds - prev.seconds
            if elapsed <= 0 or abs(elapsed - frame.dt) > ROUNDING:
                raise ValueError(f"row {line}: nonconsecutive frames")
            if frame.raid != prev.raid:
                if prev.raid != 0 or frame.raid <= 0 or frame.reset != "lifecycle" or frame.sequence == prev.sequence:
                    raise ValueError(f"row {line}: mixed raid lifecycle")
            elif frame.sequence == prev.sequence:
                if (frame.reset != "none" or not math.isnan(frame.gap)
                        or frame.confirmed != prev.confirmed or abs(frame.age - prev.age - elapsed) > ROUNDING):
                    raise ValueError(f"row {line}: inconsistent repeated snapshot")
            elif (not math.isfinite(frame.gap) or frame.reset not in {"none", "arrival_gap"}
                  or abs(frame.gap - (prev.age + elapsed - frame.age)) > ROUNDING):
                raise ValueError(f"row {line}: invalid fresh sample or reset within lifecycle")
        if frame.raid:
            raid.append(frame)
        prev = frame
    if len(raid) < 3 or raid[0].sequence == raid[-1].sequence:
        raise ValueError("incomplete raid evidence")
    return raid


def summarize(rows):
    return {
        "player": rows[0].player,
        "raid": rows[0].raid,
        "frames": len(rows),
        "raid_median_fps": 1 / median(frame.dt for frame in rows[1:]),
        "peak_age_seconds": max(frame.age for frame in rows),
    }


def compare(control_lines, recovery_lines):
    control, recovery = map(read_raid, (control_lines, recovery_lines))
    def stale(frame):
        return frame.age > STALE_SECONDS or frame.gap > STALE_SECONDS or frame.reset == "arrival_gap"

    if any(map(stale, control)):
        raise ValueError("control contains stale evidence")
    resets = [i for i, frame in enumerate(recovery) if frame.reset == "arrival_gap"]
    if len(resets) != 1:
        raise ValueError("expected exactly one arrival-gap reset")
    index = resets[0]
    if index == 0:
        raise ValueError("missing frames before reset")
    fresh = recovery[index]
    last = recovery[index - 1]
    start = index - 1
    while start > 0 and recovery[start - 1].sequence == last.sequence:
        start -= 1
    hold_start = next((i for i in range(start, index) if recovery[i].mode == "hold_latest"), index)
    held = recovery[hold_start:index]
    if (start == 0 or not any(frame.sequence > fresh.sequence for frame in recovery[index + 1:])
            or len(held) < 2 or last.age <= STALE_SECONDS or fresh.gap <= STALE_SECONDS
            or fresh.sequence <= last.sequence or fresh.mode != "hold_oldest"
            or fresh.root != fresh.confirmed
            or any(frame.mode != "hold_latest" or frame.root != last.confirmed for frame in held)
            or any(stale(frame) for frame in recovery[:hold_start] + recovery[index + 1:])):
        raise ValueError("missing or inconsistent consecutive hold and first-fresh recovery")
    return {
        "control": summarize(control),
        "recovery": summarize(recovery) | {
            "held_sequence": last.sequence,
            "held_frames": len(held),
            "held_span_seconds": last.seconds - held[0].seconds,
            "last_held_age_seconds": last.age,
            "first_fresh_sequence": fresh.sequence,
            "record_gap_seconds": fresh.gap,
            "recovery_displacement_m": math.dist(last.root, fresh.root),
        },
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("control", type=Path)
    parser.add_argument("recovery", type=Path)
    args = parser.parse_args()
    try:
        with args.control.open(newline="") as control, args.recovery.open(newline="") as recovery:
            print(json.dumps(compare(control, recovery), indent=2))
    except (OSError, ValueError, csv.Error) as error:
        parser.error(str(error))


if __name__ == "__main__":
    main()
