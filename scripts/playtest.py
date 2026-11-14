"""Run the native rendered client against an isolated real UDP server.

The client drives ordinary ClientInput messages; there are no server teleports,
damage overrides, accelerated raid clocks or test-only rewards.
Evidence is saved under target/playtest (or --output).
"""
import argparse
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--no-build", action="store_true")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--clients", type=int, choices=(1, 2), default=1)
    parser.add_argument("--server-path", type=Path)
    parser.add_argument("--client-path", type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    output = (args.output or root / "target" / "playtest").resolve()
    output.mkdir(parents=True, exist_ok=True)
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
    env.update(SERVER_ADDR="127.0.0.1", TK_PORT="19080", TK_PLAYTEST_DIR=str(output), TK_PLAYTEST_CLIENTS=str(args.clients), NO_COLOR="1")
    server_path = output / f"server{suffix}"
    client_path = output / f"client{suffix}"
    if args.no_build:
        server_path = server_path if server_path.exists() else root / "target" / "debug" / f"server{suffix}"
        client_path = client_path if client_path.exists() else root / "target" / "debug" / f"client{suffix}"
    if args.server_path:
        server_path = args.server_path.resolve()
    if args.client_path:
        client_path = args.client_path.resolve()
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
                    code = process.wait(timeout=180)
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
    server_text = (output / "server.log").read_text(encoding="utf-8", errors="replace")
    combined = re.sub(r"\x1b\[[0-9;]*m", "", client_text + server_text)
    assert "panicked at" not in combined, f"Panic in logs: {output}"
    assert " ERROR " not in combined, f"Runtime/render error in logs: {output}"
    assert "PLAYTEST PASS" in client_text, f"Missing completion proof: {output / 'client.log'}"
    if args.clients == 2:
        peer_text = (output / "peer" / "client.log").read_text(encoding="utf-8", errors="replace")
        assert "PLAYTEST PASS" in peer_text, "Peer did not finish the normal raid cycle"
        assert "panicked at" not in peer_text, "Peer panic"
        assert "SNAPSHOT LOSS" in peer_text, "Missing deliberate peer snapshot loss"
        for text in (client_text, peer_text):
            assert "WEAPON EXERCISE PASS" in text, "Missing weapon action evidence"
            assert "PHYSICAL PLAYTEST PASS" in text, "Missing physical scenario evidence"
            assert "ANATOMY RESET" in text, "Missing new raid anatomy/physics reset"
            assert "PRESENTATION CONVERGED" in text, "Physical render-entity convergence checker did not run"
            assert "CONFIRMED FLASH EVIDENCE" in text, "Missing visible confirmed muzzle flash evidence"
        convergence = r"CONVERGENCE victim=(\d+) missing=(\d+) wounds=(\d+) organs=(\[[^\]]+\]) pieces=(\[[^\]]+\])"
        leader_state = re.search(convergence, client_text)
        peer_state = re.search(convergence, peer_text)
        assert leader_state and peer_state, "Missing final converged state logs"
        assert leader_state.groups() == peer_state.groups(), "Clients disagree on persistent victim/organ/wound/piece state"
    for milestone in ("collected loot", "extracted; secured=1", "debrief"):
        assert milestone in server_text, f"Missing authoritative milestone: {milestone}"
    evidence = []
    for name in ("lobby", "raid", "extracting", "debrief", "return-lobby"):
        path = output / f"{name}.png"
        data = path.read_bytes()
        assert data[:8] == b"\x89PNG\r\n\x1a\n", f"Not a real PNG: {path}"
        width, height = struct.unpack(">II", data[16:24])
        assert width >= 600 and height >= 400 and len(data) > 5000, f"Invalid capture: {path}"
        evidence.append(f"{name}.png: {width}x{height}, {len(data)} bytes")
    if args.clients == 2:
        names = ["pose-idle", "pose-walk", "pose-run", "pose-strafe", "pose-crouch", "pose-jump", "pose-land", "pose-stop",
                 "weapon-burst", "weapon-empty", "weapon-reload", "weapon-pistol", "weapon-pistol-reload", "weapon-pistol-ready", "weapon-primary-reload",
                 "injury-healthy", "injury-correction", "injury-lateral-limp", "injury-arm", "injury-separation", "injury-fall", "injury-recovery", "injury-ragdoll"]
        for directory in (output, output / "peer"):
            for name in names:
                path = directory / f"{name}.png"
                data = path.read_bytes()
                assert data[:8] == b"\x89PNG\r\n\x1a\n" and len(data) > 5000, f"Missing native evidence: {path}"
            primary = "rifle" if directory == output else "smg"
            for weapon in (primary, "pistol"):
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
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (AssertionError, OSError, subprocess.SubprocessError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        sys.exit(1)
