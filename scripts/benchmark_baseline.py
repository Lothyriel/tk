"""Benchmark the immutable pre-change HEAD in an isolated source export.

Only timing/log instrumentation is added to the export. The working tree and
the player's running binaries are never replaced. No Git checkout/reset/stash.
"""
import argparse
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    output = args.output.resolve()
    source = output / "source"
    source.mkdir(parents=True, exist_ok=False)
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
    archive = subprocess.check_output(["git", "archive", revision, "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo", "common", "client", "server", "assets"], cwd=root)
    with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
        tar.extractall(source, filter="data")

    # Timing surrounds the complete FixedMain tick, including Rapier and snapshot generation.
    tick = source / "server/src/tick/mod.rs"
    text = tick.read_text(encoding="utf-8")
    text = text.replace("mod bots;", "mod bots;\nmod metrics;")
    text = text.replace(".init_resource::<WorldState>()", ".init_resource::<WorldState>()\n.init_resource::<metrics::TickMetrics>()\n.add_systems(FixedFirst,metrics::begin)\n.add_systems(FixedLast,metrics::end)")
    text = text.replace("server.broadcast_message(DefaultChannel::Unreliable, data::encode(&snapshot));", "let bytes=data::encode(&snapshot); if state.snapshot_sequence%150==0 {info!(\"TK PERF snapshot_bytes={} actors={} pieces=0\",bytes.len(),snapshot.players.len());} server.broadcast_message(DefaultChannel::Unreliable,bytes);")
    tick.write_text(text, encoding="utf-8")
    metrics = (root / "server/src/tick/metrics.rs").read_text(encoding="utf-8")
    metrics = metrics.replace("pieces: Query<(), With<super::physical::PhysicsPiece>>,", "")
    metrics = metrics.replace("pieces.iter().count()", "0")
    (source / "server/src/tick/metrics.rs").write_text(metrics, encoding="utf-8")
    driver = source / "client/src/playtest.rs"
    text = driver.read_text(encoding="utf-8")
    text = text.replace("struct Driver {", "struct Driver {\nframes:Vec<f32>,")
    text = text.replace("let now = time.elapsed_secs_f64();", """let now = time.elapsed_secs_f64();
driver.frames.push(time.delta_secs()*1000.0);
if driver.frames.len()==300 {driver.frames.sort_by(f32::total_cmp);info!("TK PERF frame_ms mean={:.3} p95={:.3} max={:.3}",driver.frames.iter().sum::<f32>()/300.0,driver.frames[285],driver.frames[299]);driver.frames.clear();}
""")
    driver.write_text(text, encoding="utf-8")
    (output / "baseline.json").write_text(json.dumps({"revision": revision, "source": str(source),
        "changes": ["complete tick timing", "encoded snapshot byte logging", "real client frame timing"],
        "gameplay_changes": False}, indent=2), encoding="utf-8")
    env = os.environ.copy()
    # Cargo's relocated path-package fingerprints can alias a same-name workspace
    # crate in a shared target directory. Never let baseline archives replace current rlibs.
    env["CARGO_TARGET_DIR"] = str(output / "build")
    suffix = ".exe" if os.name == "nt" else ""
    for binary in ("server", "client"):
        subprocess.run(["cargo", "rustc", "--locked", "-p", binary, "--bin", binary, "--", "-o", str(output / f"{binary}{suffix}")], cwd=source, env=env, check=True)
    subprocess.run(["python", str(root / "scripts/playtest.py"), "--no-build", "--output", str(output),
                    "--server-path", str(output / f"server{suffix}"), "--client-path", str(output / f"client{suffix}")], cwd=root, check=True)


if __name__ == "__main__":
    main()
