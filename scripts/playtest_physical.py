"""Record healthy rest/run deaths on flat ground and by a wall with two real UDP clients.

Ordinary input and anatomical shots only. This focused matrix complements, not
replaces, the extraction/debrief/redeploy acceptance in scripts/playtest.py.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import subprocess
import struct
import sys


def run(root, output, server_path, client_path, scenario, video):
    output.mkdir(parents=True, exist_ok=False)
    env = os.environ.copy()
    for flag in ("TK_PLAYTEST_VIDEO", "TK_ANIMATION_REVIEW", "TK_PHYSICAL_REVIEW", "TK_DROP_SNAPSHOT_EVERY", "TK_SHOT_DIAGNOSTICS"):
        env.pop(flag, None)
    env.update(SERVER_ADDR="127.0.0.1", TK_PORT="19081", TK_PLAYTEST_CLIENTS="2", TK_PHYSICAL_REVIEW=scenario, NO_COLOR="1")
    if video:
        env["TK_PLAYTEST_VIDEO"] = "1"
    (output / "run-settings.json").write_text(json.dumps({
        "scenario": scenario, "clients": 2, "transport": "real UDP 127.0.0.1:19081", "peer_snapshot_drop_every": 3,
        "server": str(server_path), "client": str(client_path),
        "server_sha256": hashlib.sha256(server_path.read_bytes()).hexdigest(),
        "client_sha256": hashlib.sha256(client_path.read_bytes()).hexdigest(),
        "resolution": [1280, 800], "world_fov": 70, "video": video,
        "shot_diagnostics": False,
        "controls": "walk to staging; healthy victim runs or rests; normal AP rifle head shots; observe authoritative pieces until rest",
        "simulation_overrides": False,
    }, indent=2), encoding="utf-8")
    processes, logs = [], []
    with (output / "server.log").open("w", encoding="utf-8") as log:
        server = subprocess.Popen([str(server_path)], cwd=root, env=env, stdout=log, stderr=subprocess.STDOUT)
        try:
            for role in (0, 1):
                directory = output if role == 0 else output / "peer"
                directory.mkdir(exist_ok=True)
                client_env = env.copy()
                client_env.update(TK_PLAYTEST_DIR=str(directory), TK_PLAYTEST_ROLE=str(role))
                if role == 1:
                    client_env["TK_DROP_SNAPSHOT_EVERY"] = "3"
                client_log = (directory / "client.log").open("w", encoding="utf-8")
                logs.append(client_log)
                processes.append(subprocess.Popen([str(client_path), "--playtest"], cwd=root, env=client_env, stdout=client_log, stderr=subprocess.STDOUT))
            for process in processes:
                assert process.wait(timeout=120) == 0, f"Native physical client failed: {output}"
            assert server.poll() is None, "Physical server exited unexpectedly"
        finally:
            for process in [*processes, server]:
                if process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()
            for client_log in logs:
                client_log.close()
    states = []
    resolution = None
    settings = json.loads((output / "run-settings.json").read_text())
    camera = json.loads((output / "camera-settings.json").read_text())
    settings.update(world_fov=camera["world_fov_degrees"], weapon_fov_degrees=camera["weapon_fov_degrees"])
    (output / "run-settings.json").write_text(json.dumps(settings, indent=2), encoding="utf-8")
    for directory in (output, output / "peer"):
        text = (directory / "client.log").read_text(encoding="utf-8", errors="replace")
        assert "panicked at" not in text and " ERROR " not in text, f"Runtime error: {directory}"
        assert f"PHYSICAL REVIEW PASS scenario={scenario}" in text
        assert "PHYSICAL NATIVE BEFORE" in text and "PHYSICAL NATIVE HANDOFF" in text
        if directory.name == "peer":
            assert "SNAPSHOT LOSS" in text
        for stage in ("before", "handoff", "rest-start", "rest"):
            data = (directory / f"{scenario}-{stage}.png").read_bytes()
            assert data[:8] == b"\x89PNG\r\n\x1a\n" and len(data) > 5000
            dimensions = list(struct.unpack(">II", data[16:24]))
            assert dimensions[0] >= 600 and dimensions[1] >= 400, "Minimized native physical surface"
            if resolution is None:
                resolution = dimensions
            assert dimensions == resolution, "Physical native viewport changed during the scenario"
        state = json.loads((directory / "physical-rest.json").read_text())
        assert len(state["pieces"]) == 17 and state["max_position_delta"] < 0.02 and state["max_angle_delta"] < 0.05
        states.append(state)
    assert states[0]["victim"] == states[1]["victim"]
    a, b = ({p["id"]: p for p in state["pieces"]} for state in states)
    assert a.keys() == b.keys()
    max_position, max_angle = 0.0, 0.0
    for id in a:
        assert a[id]["segment"] == b[id]["segment"]
        distance = math.dist(a[id]["position"], b[id]["position"])
        angle = 2 * math.acos(min(1.0, abs(sum(x * y for x, y in zip(a[id]["rotation"], b[id]["rotation"])))))
        max_position, max_angle = max(max_position, distance), max(max_angle, angle)
    assert max_position < 0.02 and max_angle < 0.05, "Clients disagree on resting position/orientation"
    settings["resolution"] = resolution
    (output / "run-settings.json").write_text(json.dumps(settings, indent=2), encoding="utf-8")
    report = {"scenario": scenario, "result": "PASS", "clients": 2, "segments": 17,
              "final_client_position_difference_m": max_position, "final_client_angle_difference_rad": max_angle,
              "server_performance": re.findall(r"TK PERF [^\r\n]+", (output / "server.log").read_text())}
    if video:
        subprocess.run([sys.executable, str(root / "scripts/encode_playtest_video.py"), str(output)], cwd=root, check=True)
    (output / "result.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(f"PASS {scenario}: 17 segments, two real UDP clients, snapshot loss, full pose/orientation rest convergence {max_position:.6f}m/{max_angle:.6f}rad")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--server-path", type=Path, required=True)
    parser.add_argument("--client-path", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--video", action="store_true")
    parser.add_argument("--keep-going", action="store_true", help="Collect all failing baseline cases too; failures still return a nonzero exit")
    parser.add_argument("--scenario", choices=("rest-flat", "run-flat", "rest-wall", "run-wall"), action="append")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    reports = []
    for name in args.scenario or ("rest-flat", "run-flat", "rest-wall", "run-wall"):
        try:
            reports.append(run(root, output / name, args.server_path.resolve(), args.client_path.resolve(), name, args.video))
        except (AssertionError, OSError, subprocess.SubprocessError) as error:
            reports.append({"scenario": name, "result": "FAIL", "error": str(error)})
            (output / "matrix.json").write_text(json.dumps(reports, indent=2), encoding="utf-8")
            if not args.keep_going:
                raise
            print(f"BASELINE FAILURE RETAINED {name}: {error}", flush=True)
            if args.video and (output / name / "frame-times.csv").exists():
                subprocess.run([sys.executable, str(root / "scripts/encode_playtest_video.py"), str(output / name)], cwd=root, check=True)
    (output / "matrix.json").write_text(json.dumps(reports, indent=2), encoding="utf-8")
    assert all(report["result"] == "PASS" for report in reports), "Matrix contains failed scenarios (logs/videos retained)"


if __name__ == "__main__":
    try:
        main()
    except (AssertionError, OSError, subprocess.SubprocessError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        sys.exit(1)
