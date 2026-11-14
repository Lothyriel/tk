"""Compare fresh instrumented normal-route evidence, retaining raw window metrics."""
import argparse
import json
from pathlib import Path
import re
import statistics


def summarize(directory):
    evidence = json.loads((directory / "evidence.json").read_text(encoding="utf-8"))
    server = "\n".join(evidence["server_performance"])
    client = "\n".join(evidence["client_performance"])
    tick = [tuple(map(float, match)) for match in re.findall(r"tick_ms mean=([\d.]+) p95=([\d.]+) max=([\d.]+)", server)]
    frame = [tuple(map(float, match)) for match in re.findall(r"frame_ms mean=([\d.]+) p95=([\d.]+) max=([\d.]+)", client)]
    snapshots = list(map(int, re.findall(r"snapshot_bytes=(\d+)", server)))
    assert tick and len(frame) > 1 and snapshots, f"Insufficient measured windows: {directory}"
    warm = frame[1:]  # Preserve cold-start separately, never count shader startup as steady frame time.
    return {"source": str(directory), "tick_median_window_mean_ms": statistics.median(row[0] for row in tick),
            "tick_worst_window_p95_ms": max(row[1] for row in tick),
            "tick_observed_max_ms": max(row[2] for row in tick),
            "frame_median_warm_window_mean_ms": statistics.median(row[0] for row in warm),
            "frame_worst_warm_window_p95_ms": max(row[1] for row in warm),
            "frame_cold_window_max_ms": frame[0][2],
            "snapshot_bytes_min": min(snapshots), "snapshot_bytes_max": max(snapshots)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", type=Path, required=True)
    parser.add_argument("--after", type=Path, required=True)
    parser.add_argument("--load-log", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    before, after = summarize(args.before), summarize(args.after)
    load = args.load_log.read_text(encoding="utf-8")
    match = re.search(r"TK LOAD 320 pieces tick_ms mean=([\d.]+) p95=([\d.]+) max=([\d.]+)", load)
    assert match, "Missing measured 320-piece load"
    report = {"method": "same original one-client UDP loot/combat/extraction/debrief/second-deploy route; cold first frame window excluded only from steady frame statistics",
              "before": before, "after": after, "load_320_pieces": dict(zip(("mean_ms", "p95_ms", "max_ms"), map(float, match.groups())))}
    args.output.write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(json.dumps(report, indent=2))
    assert after["tick_worst_window_p95_ms"] < 7.8, "Normal raid tick budget regression"
    assert report["load_320_pieces"]["p95_ms"] < 7.8, "320-piece tick budget regression"
    assert after["frame_worst_warm_window_p95_ms"] < 16.7, "Steady native frame budget regression"


if __name__ == "__main__":
    main()
