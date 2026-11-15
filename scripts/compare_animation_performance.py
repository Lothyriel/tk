"""Compare matched native runs without mistaking batch p95s for a global percentile."""
import argparse
import json
from pathlib import Path
import re
import statistics


def summarize(directory):
    server = (directory / "server.log").read_text(encoding="utf-8", errors="replace")
    ticks = [dict(mean=float(a), p95=float(b), maximum=float(c), pieces=int(d))
             for a, b, c, d in re.findall(r"TK PERF tick_ms mean=([\d.]+) p95=([\d.]+) max=([\d.]+) pieces=(\d+)", server)]
    snapshots = [dict(bytes=int(a), actors=int(b), pieces=int(c))
                 for a, b, c in re.findall(r"TK PERF snapshot_bytes=(\d+) actors=(\d+) pieces=(\d+)", server)]
    clients = {}
    for name, path in (("leader", directory), ("peer", directory / "peer")):
        if not (path / "client.log").exists():
            continue
        text = (path / "client.log").read_text(encoding="utf-8", errors="replace")
        rows = [(float(a), float(b), float(c)) for a, b, c in re.findall(
            r"TK PERF frame_ms mean=([\d.]+) p95=([\d.]+) max=([\d.]+)", text)]
        steady = rows[1:]  # First batch includes shader/device/window startup.
        assert steady, f"Insufficient frame batches: {path}"
        clients[name] = {"median_batch_mean_ms": statistics.median(r[0] for r in steady),
                         "median_batch_p95_ms": statistics.median(r[1] for r in steady),
                         "worst_batch_p95_ms": max(r[1] for r in steady), "raw_batches": rows}
    assert len(ticks) > 1 and snapshots
    groups = {}
    for name, rows in (("no_pieces", [r for r in ticks[1:] if r["pieces"] == 0]),
                       ("physical_pieces", [r for r in ticks[1:] if r["pieces"] > 0])):
        if rows:
            groups[name] = {"median_batch_mean_ms": statistics.median(r["mean"] for r in rows),
                            "median_batch_p95_ms": statistics.median(r["p95"] for r in rows),
                            "worst_batch_p95_ms": max(r["p95"] for r in rows), "raw_batches": rows}
    return {"directory": str(directory.resolve()), "clients": clients, "tick_groups": groups,
            "snapshot_max_bytes": max(r["bytes"] for r in snapshots), "snapshot_samples": snapshots}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("before", type=Path)
    parser.add_argument("after", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    settings = [json.loads((p / "run-settings.json").read_text()) for p in (args.before, args.after)]
    for key in ("clients", "video", "scenario", "peer_snapshot_drop_every", "resolution", "world_fov", "world_fov_degrees", "weapon_fov_degrees", "animation_review", "shot_diagnostics"):
        assert settings[0].get(key) == settings[1].get(key), f"Unmatched setting {key}"
    result = {"note": "Steady batches exclude startup; median of batch p95s is NOT a global p95. Piece counts describe the end of each batch, not every tick.",
              "settings": settings, "before": summarize(args.before), "after": summarize(args.after)}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(f"Matched native performance evidence: {args.output}")


if __name__ == "__main__":
    main()
