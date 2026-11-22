"""Run the native stale/cancel/reconnect widget checkpoint with no capture."""

import argparse
import os
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import time


def wait_marker(path, client, server):
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        client_exit, server_exit = client.poll(), server.poll()
        if client_exit is not None or server_exit is not None:
            break
        if path.is_file():
            return
        time.sleep(0.02)
    log = (path.parent / "client.log").read_text(errors="replace")
    observations = [
        line.split("STALE RECONNECT ", 1)[1]
        for line in log.splitlines()
        if "STALE RECONNECT " in line
    ]
    last = observations[-1] if observations else "no client observations"
    raise RuntimeError(
        f"missing {path.name}, client_exit={client.poll()}, server_exit={server.poll()}; "
        f"last checkpoint: {last}; see {path.parent} logs"
    )


def stop(child, *, resume=False):
    if child.poll() is None:
        if resume:
            child.send_signal(signal.SIGCONT)
        child.terminate()
        try:
            child.wait(timeout=3)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()


def run(binaries, output):
    # Use an unused local port; the bind in the server is authoritative.
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    env = dict(
        os.environ,
        SERVER_ADDR="127.0.0.1",
        TK_PORT=str(port),
        TK_STALE_RECONNECT_DIR=str(output),
        TK_PREFERENCES_PATH=str(output / "preferences.json"),
        RUST_LOG="info",
    )
    with (output / "server.log").open("w") as server_log:
        server = subprocess.Popen(
            [binaries / "server"], env=env, stdout=server_log, stderr=subprocess.STDOUT
        )
        try:
            client_log_path = output / "client.log"
            with client_log_path.open("w") as client_log:
                client = subprocess.Popen(
                    [binaries / "client", "--arena-menu-probe=keyboard"],
                    env=env,
                    stdout=client_log,
                    stderr=subprocess.STDOUT,
                )
                try:
                    wait_marker(output / "lobby", client, server)
                    server.send_signal(signal.SIGSTOP)
                    print(f"Paused disposable server PID {server.pid}", flush=True)
                    wait_marker(output / "cancelled", client, server)
                    server.send_signal(signal.SIGCONT)
                    print(f"Resumed disposable server PID {server.pid}", flush=True)
                    (output / "server-resumed").touch(exist_ok=False)
                    if client.wait(timeout=20) or "STALE RECONNECT PASS" not in client_log_path.read_text():
                        raise RuntimeError(f"checkpoint failed; see {client_log_path}")
                finally:
                    stop(client)
        finally:
            stop(server, resume=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True, help="parent for fresh run logs")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    output = Path(tempfile.mkdtemp(prefix="stale-reconnect-", dir=args.output.resolve()))
    print(f"Logs: {output}", flush=True)

    def interrupted(_signum, _frame):
        raise KeyboardInterrupt

    signal.signal(signal.SIGTERM, interrupted)
    run(args.bin_dir.resolve(), output)
    print("PASS: native UDP stale/cancel/reconnect (synthetic Bevy keyboard)")


if __name__ == "__main__":
    main()
