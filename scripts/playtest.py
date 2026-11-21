"""Run the native rendered client against an isolated real UDP server.

The client drives ordinary ClientInput messages; there are no server teleports,
damage overrides, accelerated raid clocks or test-only rewards.
Evidence is saved under target/playtest (or --output).
"""
import argparse
import csv
import hashlib
import json
import math
import os
from pathlib import Path
import re
import struct
import statistics
import subprocess
import sys


POV_CAPTURES = {
    "ads": ("rest", "ads", "return"),
    "inspect": ("rest", "inspect", "return"),
    "stop-lean": ("rest", "walk", "run", "stop", "crouch", "left", "right", "return"),
    "side-cover": ("rest", "walk", "run", "stop", "crouch", "left", "right", "return"),
}


def gait_metrics(path):
    with path.open(newline="", encoding="utf-8") as stream:
        rows = [{key: float(value) for key, value in row.items()}
                for row in csv.DictReader(stream)]
    assert len(rows) >= 30, f"Insufficient observer telemetry: {path}"
    assert all(math.isfinite(value) for row in rows for value in row.values()), path
    assert all(a["seconds"] < b["seconds"] and a["snapshot"] <= b["snapshot"]
               for a, b in zip(rows, rows[1:])), f"Unordered observer telemetry: {path}"
    result = {}
    for name, low, high in (("walk", 2.5, 3.7), ("run", 4.5, 7.0)):
        samples = []
        transitions = {"loading": [], "steady": [], "releasing": []}
        for a, b in zip(rows, rows[1:]):
            dt = b["seconds"] - a["seconds"]
            if dt >= 0.05 or not (low < a["speed"] < high and low < b["speed"] < high):
                continue
            for side in ("left", "right"):
                if min(a[f"{side}_weight"], b[f"{side}_weight"]) <= 0.95:
                    continue
                distance = math.dist(
                    [a[f"{side}_{axis}"] for axis in "xyz"],
                    [b[f"{side}_{axis}"] for axis in "xyz"],
                )
                samples.append((distance, dt))
                delta_weight = b[f"{side}_weight"] - a[f"{side}_weight"]
                phase = ("loading" if delta_weight > 0.005
                         else "releasing" if delta_weight < -0.005 else "steady")
                transitions[phase].append(distance)
        assert samples, f"No consecutive loaded {name} samples: {path}"
        peak, interval = max(samples)
        result[name] = {
            "loaded_pairs": len(samples),
            "max_displacement_m": peak,
            "max_displacement_interval_s": interval,
            "median_displacement_m": statistics.median(distance for distance, _ in samples),
            "by_weight_transition": {
                phase: {"pairs": len(distances), "max_displacement_m": max(distances, default=None)}
                for phase, distances in transitions.items()
            },
        }
    return result


def parse_viewport(value):
    match = re.fullmatch(r"([0-9]{1,4})x([0-9]{1,4})", value)
    if match:
        width, height = map(int, match.groups())
        if 600 <= width <= 3840 and 400 <= height <= 2160:
            return width, height
    raise argparse.ArgumentTypeError("expected WIDTHxHEIGHT between 600x400 and 3840x2160")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--no-build", action="store_true")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--clients", type=int, choices=(1, 2), default=1)
    parser.add_argument("--server-path", type=Path)
    parser.add_argument("--client-path", type=Path)
    parser.add_argument("--video", action="store_true", help="Record native framebuffers and encode timestamped MP4 reviews")
    parser.add_argument("--animation-review", action="store_true", help="Add reciprocal continuous weapon review before the ordinary dual raid")
    parser.add_argument("--weapon-kit", choices=("compact", "suppressed", "afg"),
                        help="Seed server-owned M4A1 kit and physical magazines for native acceptance")
    parser.add_argument("--configure-weapons", action="store_true",
                        help="Choose compact/suppressed kits through real lobby panel actions")
    parser.add_argument("--weapon-debug", action="store_true",
                        help="Show live socket/pivot overlays and use a close side observer during animation review")
    parser.add_argument("--clean-art", action="store_true",
                        help="Keep the asset review cameras but hide socket/pivot overlays")
    parser.add_argument("--neutral-art", action="store_true",
                        help="Use neutral white lighting without cast shadows for material review")
    parser.add_argument("--weapon-lod", choices=("auto", "near", "far"), default="auto",
                        help="Explicit LOD inspection override (requires --weapon-debug)")
    parser.add_argument("--pov", choices=POV_CAPTURES, help="Run only the named POV bench after deploy")
    parser.add_argument("--viewport", type=parse_viewport,
                        help="Require this exact POV or clean-art framebuffer (WIDTHxHEIGHT)")
    parser.add_argument("--pov-hz", type=int, choices=range(15, 241), metavar="HZ",
                        help="Cap the stop-lean client update rate and write the actual camera trace")
    args = parser.parse_args()
    if args.viewport and not (args.pov or args.clean_art):
        parser.error("--viewport requires --pov or --clean-art")
    if args.pov_hz and args.pov != "stop-lean":
        parser.error("--pov-hz requires --pov stop-lean")
    if args.pov and (args.animation_review or args.configure_weapons
                     or (args.video and (args.pov != "stop-lean" or args.clients != 1 or args.pov_hz))
                     or (args.clients == 2 and args.pov != "stop-lean")):
        parser.error("--pov is standalone; only stop-lean supports a second client or solo video without --pov-hz")
    assert not args.weapon_debug or args.animation_review, "--weapon-debug requires --animation-review"
    assert not args.clean_art or args.weapon_debug, "--clean-art requires --weapon-debug"
    assert not args.neutral_art or args.clean_art, "--neutral-art requires --clean-art"
    assert args.weapon_debug or args.weapon_lod == "auto", "--weapon-lod requires --weapon-debug"
    assert not (args.configure_weapons and args.weapon_kit), "Panel acceptance must not seed server fixtures"
    root = Path(__file__).resolve().parents[1]
    output = (args.output or root / "target" / "playtest").resolve()
    output.mkdir(parents=True, exist_ok=True)
    if args.pov:
        for name in POV_CAPTURES[args.pov]:
            (output / f"pov-{name}.png").unlink(missing_ok=True)
        (output / "pov-trace.csv").unlink(missing_ok=True)
        if args.clients == 2:
            (output / "peer" / "gait-trace.csv").unlink(missing_ok=True)
            for name in ("run", "left", "right", "return"):
                (output / "peer" / f"pov-{name}.png").unlink(missing_ok=True)
    if args.video:
        assert not (output / "frame-times.csv").exists(), "Use a fresh output directory for video evidence"
    # Old screenshots must never satisfy evidence checks for a new run.
    for name in ("lobby.png", "raid.png", "extracting.png", "debrief.png", "return-lobby.png", "result.txt"):
        (output / name).unlink(missing_ok=True)
    if not args.no_build:
        # Isolated executable outputs never overwrite a player's running Windows binary.
        for binary in ("server", "client"):
            suffix = ".exe" if os.name == "nt" else ""
            subprocess.run(["cargo", "rustc", "--locked", "-p", binary, "--bin", binary,
                            "--", "-o", str(output / f"{binary}{suffix}")], cwd=root, check=True)
    suffix = ".exe" if os.name == "nt" else ""
    env = os.environ.copy()
    env.pop("TK_PLAYTEST_WEAPON_KIT", None)
    if args.weapon_kit:
        env["TK_PLAYTEST_WEAPON_KIT"] = args.weapon_kit
    for flag in ("TK_PLAYTEST_VIDEO", "TK_ANIMATION_REVIEW", "TK_PHYSICAL_REVIEW", "TK_DROP_SNAPSHOT_EVERY",
                  "TK_WEAPON_DEBUG", "TK_WEAPON_LOD", "TK_PLAYTEST_CONFIGURATOR", "TK_PLAYTEST_POV",
                  "TK_PLAYTEST_VIEWPORT", "TK_PLAYTEST_POV_HZ", "TK_PLAYTEST_CLEAN_ART",
                  "TK_PLAYTEST_NEUTRAL_ART"):
        env.pop(flag, None)
    if args.pov:
        env["TK_PLAYTEST_POV"] = args.pov
    if args.viewport:
        env["TK_PLAYTEST_VIEWPORT"] = f"{args.viewport[0]}x{args.viewport[1]}"
    if args.pov_hz:
        env["TK_PLAYTEST_POV_HZ"] = str(args.pov_hz)
    env.update(SERVER_ADDR="127.0.0.1", TK_PORT="19080", TK_PLAYTEST_DIR=str(output), TK_PLAYTEST_CLIENTS=str(args.clients), NO_COLOR="1")
    if args.video:
        env["TK_PLAYTEST_VIDEO"] = "1"
    if args.animation_review:
        assert args.clients == 2, "Animation review requires a real UDP observer"
        env["TK_ANIMATION_REVIEW"] = "1"
    if args.weapon_debug:
        env["TK_WEAPON_DEBUG"] = "1"
        env["TK_WEAPON_LOD"] = args.weapon_lod
    if args.clean_art:
        env["TK_PLAYTEST_CLEAN_ART"] = "1"
    if args.neutral_art:
        env["TK_PLAYTEST_NEUTRAL_ART"] = "1"
    if args.configure_weapons:
        env["TK_PLAYTEST_CONFIGURATOR"] = "1"
    server_path = output / f"server{suffix}"
    client_path = output / f"client{suffix}"
    if args.no_build:
        server_path = server_path if server_path.exists() else root / "target" / "debug" / f"server{suffix}"
        client_path = client_path if client_path.exists() else root / "target" / "debug" / f"client{suffix}"
    if args.server_path:
        server_path = args.server_path.resolve()
    if args.client_path:
        client_path = args.client_path.resolve()
    (output / "run-settings.json").write_text(json.dumps({
        "server_executable": str(server_path), "client_executable": str(client_path),
        "server_sha256": hashlib.sha256(server_path.read_bytes()).hexdigest(),
        "client_sha256": hashlib.sha256(client_path.read_bytes()).hexdigest(),
        "clients": args.clients, "transport": "real UDP 127.0.0.1:19080",
        "peer_snapshot_drop_every": 3 if args.clients == 2 else 0,
        "resolution": [1280, 800], "world_fov_degrees": 70,
        "requested_viewport": args.viewport,
        "requested_pov_hz": args.pov_hz,
        "video": args.video, "animation_review": args.animation_review,
        "weapon_debug": args.weapon_debug, "weapon_lod": args.weapon_lod,
        "clean_art": args.clean_art,
        "art_lighting": "neutral white, cast shadows disabled" if args.neutral_art else "raid",
        "initial_weapon_kit_fixture": args.weapon_kit,
        "lobby_configurator": args.configure_weapons,
        "capture_target_hz": 30 if args.video else 0,
        "shot_diagnostics": env.get("TK_SHOT_DIAGNOSTICS") is not None,
        "loadouts": ("leader compact M4A1 / peer suppressed M4A1" if args.configure_weapons
                     else "leader Rifle / peer SMG") + "; AP; heavy vest; helmet; both carry pistol",
        "route": args.pov or "optional reciprocal continuous weapon actions -> full-body route -> weapon exercise -> loot -> sentry combat -> extraction -> debrief -> lobby -> deploy -> limb injury/fall/recovery/death",
        "timestamps": "client.log PLAYTEST stage clocks and ANIMATION CONTACT local clocks; frame-times.csv real frame timestamps",
    }, indent=2), encoding="utf-8")
    with (output / "server.log").open("w", encoding="utf-8") as server_log:
        server = subprocess.Popen([str(server_path)], cwd=root, env=env, stdout=server_log, stderr=subprocess.STDOUT)
        try:
            processes = []
            logs = []
            try:
                for index in range(args.clients):
                    client_output = output if index == 0 else output / "peer"
                    client_output.mkdir(exist_ok=True)
                    client_env = env.copy()
                    client_env.update(TK_PLAYTEST_DIR=str(client_output), TK_PLAYTEST_ROLE=str(index))
                    if index == 1:
                        client_env["TK_DROP_SNAPSHOT_EVERY"] = "3"
                    log = (client_output / "client.log").open("w", encoding="utf-8")
                    logs.append(log)
                    processes.append(subprocess.Popen([str(client_path), "--playtest"], cwd=root,
                                                      env=client_env, stdout=log, stderr=subprocess.STDOUT))
                for process in processes:
                    code = process.wait(timeout=45 if args.pov else 300)
                    if code != 0:
                        raise AssertionError(f"Client failed ({code}); see {output}")
            finally:
                for process in processes:
                    if process.poll() is None:
                        process.terminate()
                        process.wait(timeout=10)
                for log in logs:
                    log.close()
            if server.poll() is not None:
                raise AssertionError(f"Server exited unexpectedly ({server.returncode}); see {output / 'server.log'}")
        finally:
            # Terminate only the process owned by this harness, never an existing game server.
            if server.poll() is None:
                server.terminate()
                try:
                    server.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait()
    client_text = (output / "client.log").read_text(encoding="utf-8", errors="replace")
    settings = json.loads((output / "run-settings.json").read_text())
    settings.update(json.loads((output / "camera-settings.json").read_text()))
    (output / "run-settings.json").write_text(json.dumps(settings, indent=2), encoding="utf-8")
    server_text = (output / "server.log").read_text(encoding="utf-8", errors="replace")
    combined = re.sub(r"\x1b\[[0-9;]*m", "", client_text + server_text)
    assert "panicked at" not in combined, f"Panic in logs: {output}"
    assert " ERROR " not in combined, f"Runtime/render error in logs: {output}"
    if args.pov:
        if args.clients == 2:
            peer = output / "peer"
            peer_text = (peer / "client.log").read_text(encoding="utf-8", errors="replace")
            peer_text = re.sub(r"\x1b\[[0-9;]*m", "", peer_text)
            assert "POV OBSERVER PASS" in peer_text and "panicked at" not in peer_text
            assert " ERROR " not in peer_text, f"Runtime/render error in observer log: {peer}"
            settings["observer_gait"] = gait_metrics(peer / "gait-trace.csv")
            for name in ("run", "left", "right", "return"):
                assert f"POV OBSERVER {name} " in peer_text, f"Missing observer cue {name}: {peer}"
                data = (peer / f"pov-{name}.png").read_bytes()
                assert data[:8] == b"\x89PNG\r\n\x1a\n" and len(data) > 5000
                if args.viewport:
                    assert struct.unpack(">II", data[16:24]) == args.viewport
        if args.pov_hz:
            trace = output / "pov-trace.csv"
            assert trace.is_file() and trace.stat().st_size > 500, f"Missing camera cadence trace: {trace}"
        proof = re.search(rf"POV PASS {re.escape(args.pov)} t=([0-9.]+)", client_text)
        assert proof, f"Missing POV completion proof: {output / 'client.log'}"
        names = tuple(f"pov-{name}" for name in POV_CAPTURES[args.pov])
        for name in names:
            assert f"POV EVIDENCE {name} " in client_text
            data = (output / f"{name}.png").read_bytes()
            assert data[:8] == b"\x89PNG\r\n\x1a\n" and len(data) > 5000
            width, height = struct.unpack(">II", data[16:24])
            assert width >= 600 and height >= 400
            assert args.viewport is None or args.viewport == (width, height), (
                f"{name}.png: expected {args.viewport}, got {(width, height)}; "
                "check whether the window manager overrides the requested size"
            )
            assert name == names[0] or settings["resolution"] == [width, height]
            settings["resolution"] = [width, height]
        settings["seconds_to_evidence"] = float(proof[1])
        (output / "run-settings.json").write_text(json.dumps(settings, indent=2), encoding="utf-8")
        if args.video:
            subprocess.run([sys.executable, str(root / "scripts/encode_playtest_video.py"), str(output)], check=True)
        result = f"PASS: native POV {args.pov}, real UDP, evidence at {proof[1]}s\n"
        (output / "result.txt").write_text(result, encoding="utf-8")
        print(result, end="")
        return
    assert "PLAYTEST PASS" in client_text, f"Missing completion proof: {output / 'client.log'}"
    if args.configure_weapons:
        assert "INITIAL EQUIPMENT FIXTURE" not in server_text
        assert "LOBBY CONFIGURATOR PASS peer=false capacity=20" in client_text
        assert "REDEPLOY CONFIGURATOR PASS capacity=20" in client_text
    if args.weapon_kit:
        assert f"INITIAL EQUIPMENT FIXTURE {args.weapon_kit}" in server_text
        capacity, definitions = {
            "compact": (20, [120, 122, 124, 126, 127, 130, 131]),
            "suppressed": (30, [101, 122, 123, 125, 128, 130, 131, 129, 132, 133, 134]),
            "afg": (20, [120, 122, 123, 126, 128, 130, 131, 135]),
        }[args.weapon_kit]
        assert f"DEPLOY KIT capacity={capacity} definitions={definitions}" in client_text, "Wrong replicated kit"
    if args.clients == 2:
        peer_text = (output / "peer" / "client.log").read_text(encoding="utf-8", errors="replace")
        assert "PLAYTEST PASS" in peer_text, "Peer did not finish the normal raid cycle"
        if args.configure_weapons:
            assert "LOBBY CONFIGURATOR PASS peer=true capacity=30" in peer_text
            assert "REDEPLOY CONFIGURATOR PASS capacity=30" in peer_text
        assert "panicked at" not in peer_text, "Peer panic"
        assert "SNAPSHOT LOSS" in peer_text, "Missing deliberate peer snapshot loss"
        for text in (client_text, peer_text):
            assert "WEAPON EXERCISE PASS" in text, "Missing weapon action evidence"
            assert "PHYSICAL PLAYTEST PASS" in text, "Missing physical scenario evidence"
            assert "ANATOMY RESET" in text, "Missing new raid anatomy/physics reset"
            assert "PRESENTATION CONVERGED" in text, "Physical render-entity convergence checker did not run"
            assert "CONFIRMED FLASH EVIDENCE" in text, "Missing visible confirmed muzzle flash evidence"
            assert "LIVE RECOVERY PASS" in text, "Missing articulated fall/get-up continuity proof"
            if args.animation_review:
                assert "ADS EVIDENCE m1911-ads" in text and "ADS EVIDENCE primary-ads" in text, "Missing authoritative ADS enter/release proof"
                assert "RELOAD MECHANISM EVIDENCE m1911-slide-release" in text and "RELOAD MECHANISM EVIDENCE primary-bolt-release" in text, "Missing empty-reload mechanism proof"
        recovery = [json.loads((directory / "live-recovery.json").read_text()) for directory in (output, output / "peer")]
        assert recovery[0] == recovery[1], "Clients disagree on surviving fall/recovery IDs"
        assert recovery[0]["control_restored"] and recovery[0]["fall_ids"] == recovery[0]["getup_ids"]
        convergence = r"CONVERGENCE victim=(\d+) missing=(\d+) wounds=(\d+) organs=(\[[^\]]+\]) pieces=(\[[^\]]+\])"
        leader_state = re.search(convergence, client_text)
        peer_state = re.search(convergence, peer_text)
        assert leader_state and peer_state, "Missing final converged state logs"
        assert leader_state.groups() == peer_state.groups(), "Clients disagree on persistent victim/organ/wound/piece state"
    for milestone in ("collected loot", "extracted; secured=1", "debrief"):
        assert milestone in server_text, f"Missing authoritative milestone: {milestone}"
    evidence = []
    resolution = None
    for name in ("lobby", "raid", "extracting", "debrief", "return-lobby"):
        path = output / f"{name}.png"
        data = path.read_bytes()
        assert data[:8] == b"\x89PNG\r\n\x1a\n", f"Not a real PNG: {path}"
        width, height = struct.unpack(">II", data[16:24])
        assert width >= 600 and height >= 400 and len(data) > 5000, f"Invalid capture: {path}"
        assert args.viewport is None or (width, height) == args.viewport, f"Unexpected viewport: {path}"
        if resolution is None:
            resolution = [width, height]
        assert [width, height] == resolution, f"Native viewport changed during the run: {path}"
        evidence.append(f"{name}.png: {width}x{height}, {len(data)} bytes")
    settings["resolution"] = resolution
    (output / "run-settings.json").write_text(json.dumps(settings, indent=2), encoding="utf-8")
    if args.clients == 2:
        names = ["pose-idle", "pose-walk", "pose-run", "pose-strafe", "pose-crouch", "pose-jump", "pose-land", "pose-stop",
                 "weapon-burst", "weapon-empty", "weapon-reload", "weapon-pistol", "weapon-pistol-reload", "weapon-pistol-ready", "weapon-primary-reload",
                 "injury-healthy", "injury-correction", "injury-lateral-limp", "injury-arm", "injury-separation", "injury-fall", "injury-recovery", "injury-ragdoll"]
        for directory in (output, output / "peer"):
            if args.weapon_debug:
                for label in ("right", "left", "front", "top", "three-quarter"):
                    path = directory / f"asset-{label}.png"
                    data = path.read_bytes()
                    assert data[:8] == b"\x89PNG\r\n\x1a\n" and len(data) > 5000, f"Missing live asset view: {path}"
                    assert args.viewport is None or struct.unpack(">II", data[16:24]) == args.viewport, (
                        f"Unexpected viewport: {path}"
                    )
            for name in names:
                path = directory / f"{name}.png"
                data = path.read_bytes()
                assert data[:8] == b"\x89PNG\r\n\x1a\n" and len(data) > 5000, f"Missing native evidence: {path}"
            if args.configure_weapons:
                kit = "compact" if directory == output else "suppressed"
                path = directory / f"configurator-{kit}.png"
                data = path.read_bytes()
                assert data[:8] == b"\x89PNG\r\n\x1a\n" and len(data) > 5000, f"Missing confirmed panel preview: {path}"
            primary = "m4a1" if directory == output or args.configure_weapons else "mp5"
            for weapon in (primary, "m1911"):
                path = directory / f"confirmed-{weapon}-flash.png"
                data = path.read_bytes()
                assert data[:8] == b"\x89PNG\r\n\x1a\n" and len(data) > 5000, f"Missing confirmed flash capture: {path}"
        evidence.append("50 additional native pose/weapon/injury/confirmed-flash frames; identical final anatomy/piece IDs; peer drops every third snapshot")
    (output / "evidence.json").write_text(json.dumps({"clients": args.clients, "captures": evidence,
        "server_performance": re.findall(r"TK PERF [^\r\n]+", server_text),
        "client_performance": re.findall(r"TK PERF [^\r\n]+", client_text)}, indent=2), encoding="utf-8")
    report = "\n".join([
        f"PASS: {args.clients} native rendered client(s) + real UDP server",
        "Lobby -> countdown -> combat -> loot -> extraction -> debrief -> lobby -> second raid",
        *evidence,
        "Server milestones: collected loot; extracted; secured=1; debrief",
        "Client assertions and exact positions: client.log",
    ])
    (output / "result.txt").write_text(report + "\n", encoding="utf-8")
    print(report)
    if args.video:
        subprocess.run([sys.executable, str(root / "scripts/encode_playtest_video.py"), str(output)], check=True)
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (AssertionError, OSError, subprocess.SubprocessError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        sys.exit(1)
