"""Run a frozen original-presentation export with its unchanged historical checks.

The candidate runner always retains its ADS/living-rig assertions. Original
presentation predates those fields, so use its fingerprinted original runner,
while recording the same actual camera settings and performance metadata.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import struct
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("export", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--clients", type=int, choices=(1, 2), default=2)
    parser.add_argument("--video", action="store_true")
    parser.add_argument("--animation-review", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    export, output = args.export.resolve(), args.output.resolve()
    manifest = json.loads((export / "baseline-source.json").read_text())
    assert manifest["presentation_changes"] is False, "Never use baseline checks for a candidate"
    runner = export / "scripts/playtest.py"
    assert hashlib.sha256(runner.read_bytes()).hexdigest() == manifest["original_runner_sha256"], "Historical assertions changed"
    assert not (output / "client.log").exists(), "Use a fresh baseline evidence directory"
    assert not args.animation_review or args.clients == 2
    suffix = ".exe" if os.name == "nt" else ""
    server, client = export / f"server{suffix}", export / f"client{suffix}"
    env = os.environ.copy()
    for flag in ("TK_PLAYTEST_VIDEO", "TK_ANIMATION_REVIEW", "TK_PHYSICAL_REVIEW", "TK_DROP_SNAPSHOT_EVERY", "TK_SHOT_DIAGNOSTICS"):
        env.pop(flag, None)
    if args.video:
        env["TK_PLAYTEST_VIDEO"] = "1"
    if args.animation_review:
        env["TK_ANIMATION_REVIEW"] = "1"
    output.mkdir(parents=True, exist_ok=True)
    settings = {
        "original_revision": manifest["revision"], "original_runner_sha256": manifest["original_runner_sha256"],
        "baseline_overlays": manifest["overlays"],
        "server_executable": str(server), "client_executable": str(client),
        "server_sha256": hashlib.sha256(server.read_bytes()).hexdigest(),
        "client_sha256": hashlib.sha256(client.read_bytes()).hexdigest(),
        "clients": args.clients, "transport": "real UDP 127.0.0.1:19080",
        "peer_snapshot_drop_every": 3 if args.clients == 2 else 0,
        "resolution": [1280, 800], "video": args.video, "animation_review": args.animation_review,
        "capture_target_hz": 30 if args.video else 0,
        "shot_diagnostics": False,
        "loadouts": "leader Rifle / peer SMG; AP; heavy vest; helmet; both carry pistol",
    }
    (output / "run-settings.json").write_text(json.dumps(settings, indent=2), encoding="utf-8")
    subprocess.run([sys.executable, str(runner), "--clients", str(args.clients), "--no-build",
                    "--output", str(output), "--server-path", str(server), "--client-path", str(client)],
                   cwd=root, env=env, check=True)
    settings.update(json.loads((output / "camera-settings.json").read_text()))
    resolutions = [list(struct.unpack(">II", (output / f"{name}.png").read_bytes()[16:24]))
                   for name in ("lobby", "raid", "extracting", "debrief", "return-lobby")]
    assert all(value == resolutions[0] for value in resolutions), "Baseline native viewport changed during the run"
    settings["resolution"] = resolutions[0]
    (output / "run-settings.json").write_text(json.dumps(settings, indent=2), encoding="utf-8")
    if args.video:
        subprocess.run([sys.executable, str(root / "scripts/encode_playtest_video.py"), str(output)], cwd=root, check=True)


if __name__ == "__main__":
    main()
