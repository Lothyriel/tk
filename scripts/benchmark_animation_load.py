"""Interleave identical 320-piece Rust fixtures from frozen before/after exports."""
import argparse
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import tomllib


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before-source", type=Path, required=True)
    parser.add_argument("--after-source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    env = os.environ.copy()
    env["CARGO_TARGET_DIR"] = str(root / "target")
    rows = []
    # Alternate order to reduce a systematic warm/cold-machine bias.
    for repetition in range(3):
        order = ("before", "after") if repetition != 1 else ("after", "before")
        for name in order:
            source = (args.before_source if name == "before" else args.after_source).resolve()
            package = tomllib.loads((source / "server/Cargo.toml").read_text())["package"]["name"]
            command = ["cargo", "test", "--locked", "--offline", "--manifest-path", str(source / "Cargo.toml"),
                       "-p", package, "full_physical_load_stays_bounded_and_reset_removes_physics", "--", "--nocapture"]
            result = subprocess.run(command, cwd=root, env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            path = output / f"{name}-{repetition + 1}.log"
            path.write_text(result.stdout, encoding="utf-8")
            assert result.returncode == 0, f"Load fixture failed; original budget assertion retained: {path}"
            match = re.search(r"TK LOAD 320 pieces tick_ms mean=([\d.]+) p95=([\d.]+) max=([\d.]+)", result.stdout)
            assert match and "1 passed; 0 failed" in result.stdout, f"Missing load proof: {path}"
            row = {"version": name, "repetition": repetition + 1, "command": command,
                   "mean_ms": float(match[1]), "p95_ms": float(match[2]), "max_ms": float(match[3])}
            rows.append(row)
            print(f"{name} repetition{repetition + 1}: p95={row['p95_ms']:.3f}ms", flush=True)
    summary = {name: {"median_run_mean_ms": statistics.median(r["mean_ms"] for r in rows if r["version"] == name),
                      "median_run_p95_ms": statistics.median(r["p95_ms"] for r in rows if r["version"] == name),
                      "worst_run_p95_ms": max(r["p95_ms"] for r in rows if r["version"] == name)} for name in ("before", "after")}
    (output / "load-comparison.json").write_text(json.dumps({
        "fixture": "20 dead actors, bounded at 320 pieces, 640 measured updates, original p95 <7.8ms assertion",
        "note": "Run percentiles are measured by the same fixture; their median is not a pooled global p95. Run without video/other test workloads.",
        "summary": summary, "runs": rows}, indent=2), encoding="utf-8")


if __name__ == "__main__":
    main()
