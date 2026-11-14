"""Run the native rendered client against an isolated real UDP server.

The client drives ordinary ClientInput messages; there are no server teleports,
damage overrides, accelerated raid clocks or test-only rewards.
Evidence is saved under target/playtest (or --output).
"""
import argparse
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
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    output = (args.output or root / "target" / "playtest").resolve()
    output.mkdir(parents=True, exist_ok=True)
    # Old screenshots must never satisfy evidence checks for a new run.
    for name in ("lobby.png", "raid.png", "extracting.png", "debrief.png", "return-lobby.png", "result.txt"):
        (output / name).unlink(missing_ok=True)
    if not args.no_build:
        subprocess.run(["cargo", "build", "--workspace", "--locked", "-j", "4"], cwd=root, check=True)
    suffix = ".exe" if os.name == "nt" else ""
    env = os.environ.copy()
    env.update(SERVER_ADDR="127.0.0.1", TK_PORT="19080", TK_PLAYTEST_DIR=str(output), NO_COLOR="1")
    server_path = root / "target" / "debug" / f"server{suffix}"
    client_path = root / "target" / "debug" / f"client{suffix}"
    with (output / "server.log").open("w", encoding="utf-8") as server_log:
        server = subprocess.Popen([str(server_path)], cwd=root, env=env, stdout=server_log, stderr=subprocess.STDOUT)
        try:
            with (output / "client.log").open("w", encoding="utf-8") as client_log:
                client = subprocess.run([str(client_path), "--playtest"], cwd=root, env=env,
                                        stdout=client_log, stderr=subprocess.STDOUT, timeout=180)
            if server.poll() is not None:
                raise AssertionError(f"Server exited unexpectedly ({server.returncode}); see {output / 'server.log'}")
            if client.returncode != 0:
                raise AssertionError(f"Client failed ({client.returncode}); see {output / 'client.log'}")
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
    report = "\n".join([
        "PASS: native rendered client + real UDP server",
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
