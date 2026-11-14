"""Run the specification's structural, simulation and native UDP acceptance gates."""
import argparse
from datetime import datetime
import json
import hashlib
import os
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    output = (args.output or root / "target/playtest" / datetime.now().strftime("acceptance-%Y%m%d-%H%M%S")).resolve()
    output.mkdir(parents=True, exist_ok=False)
    files = sorted([*root.glob("common/src/**/*.rs"), *root.glob("client/src/**/*.rs"), *root.glob("server/src/**/*.rs"),
                    root / "Cargo.toml", root / "Cargo.lock", root / "scripts/playtest.py"])
    fingerprint = {str(path.relative_to(root)): hashlib.sha256(path.read_bytes()).hexdigest() for path in files}
    (output / "source-fingerprint.json").write_text(json.dumps(fingerprint, indent=2), encoding="utf-8")
    commands = [
        ("fmt", ["cargo", "fmt", "--all", "--", "--check"]),
        ("check", ["cargo", "check", "--workspace"]),
        ("tests", ["cargo", "test", "--workspace", "-j", "4"]),
        ("physical-load", ["cargo", "test", "-p", "server", "full_physical_load", "--", "--nocapture"]),
        ("snapshot-budget", ["cargo", "test", "-p", "common", "persistent_anatomy", "--", "--nocapture"]),
        ("normal-raid", ["python", "scripts/playtest.py", "--output", str(output / "normal")]),
        ("dual-raid", ["python", "scripts/playtest.py", "--clients", "2", "--no-build", "--output", str(output / "dual"),
                       "--server-path", str(output / "normal" / ("server.exe" if os.name == "nt" else "server")),
                       "--client-path", str(output / "normal" / ("client.exe" if os.name == "nt" else "client"))]),
    ]
    results = []
    for name, command in commands:
        print("RUN", " ".join(command), flush=True)
        process = subprocess.run(command, cwd=root, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, encoding="utf-8", errors="replace")
        (output / f"{name}.log").write_text(process.stdout, encoding="utf-8")
        print(process.stdout, flush=True)
        results.append({"gate": name, "command": command, "exit": process.returncode})
        (output / "gates.json").write_text(json.dumps(results, indent=2), encoding="utf-8")
        if process.returncode:
            print(f"FAIL: {name}; evidence {output}")
            return process.returncode
    current = {str(path.relative_to(root)): hashlib.sha256(path.read_bytes()).hexdigest() for path in files}
    if current != fingerprint:
        print(f"FAIL: source changed during validation; rerun frozen source. Evidence {output}")
        return 1
    print(f"PASS: all acceptance command gates on unchanged source; evidence {output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
