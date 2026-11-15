"""Export the pre-polish revision and add only the current ordinary-input recorder.

Unique path-package names reuse compiled third-party dependencies without aliasing
the real workspace's crate outputs. Never checks out, stashes or resets user files.
"""
import argparse
import hashlib
import io
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import tarfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--revision", default="705d035")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--prepare-only", action="store_true")
    parser.add_argument("--current-source", action="store_true", help="Freeze the current candidate instead of exporting original presentation")
    parser.add_argument("--match-current-camera", action="store_true", help="Original presentation with only camera projection settings matched to the candidate (70/65 degrees)")
    args = parser.parse_args()
    assert not (args.current_source and args.match_current_camera), "Camera matching applies only to the original baseline"
    root = Path(__file__).resolve().parents[1]
    output = args.output.resolve()
    source = output / "source"
    source.mkdir(parents=True, exist_ok=False)
    revision = subprocess.check_output(["git", "rev-parse", args.revision], cwd=root, text=True).strip()
    if args.current_source:
        for name in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml"):
            shutil.copy2(root / name, source / name)
        for name in (".cargo", "common", "client", "server", "assets"):
            shutil.copytree(root / name, source / name, dirs_exist_ok=True)
    else:
        archive = subprocess.check_output(["git", "archive", revision, "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo", "common", "client", "server", "assets"], cwd=root)
        with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
            tar.extractall(source, filter="data")
    shutil.copy2(root / "client/src/playtest.rs", source / "client/src/playtest.rs")
    shutil.copytree(root / "client/src/playtest", source / "client/src/playtest", dirs_exist_ok=True)
    if not args.current_source:
        if args.match_current_camera:
            camera = source / "client/src/render/mod.rs"
            text = camera.read_text(encoding="utf-8")
            original = "fov: 90.0_f32.to_radians(),"
            assert text.count(original) == 2, "Original world/weapon camera defaults changed"
            text = text.replace(original, "fov: 70.0_f32.to_radians(),", 1)
            text = text.replace(original, "fov: 65.0_f32.to_radians(),", 1)
            original = "view_model_perspective.fov = fov;"
            assert text.count(original) == 1, "Original weapon camera synchronization changed"
            text = text.replace(original, "view_model_perspective.fov = 65.0_f32.to_radians();")
            camera.write_text(text, encoding="utf-8")
        driver = source / "client/src/playtest.rs"
        text = driver.read_text(encoding="utf-8")
        # Original presentation/protocol has no packed physical rig. Keep its
        # original assertions and actions; omit only the new candidate-only proof.
        text = re.sub(r"(?ms)^\s*// LIVE_RAGDOLL_PROOF_BEGIN.*?^\s*// LIVE_RAGDOLL_PROOF_END[^\n]*\n", "", text)
        text = re.sub(r"(?ms)^\s*// ADS_PROOF_BEGIN.*?^\s*// ADS_PROOF_END[^\n]*\n", "", text)
        driver.write_text(text, encoding="utf-8")
        animation = source / "client/src/playtest/animation.rs"
        text = re.sub(r"(?ms)^\s*// ADS_PROOF_BEGIN.*?^\s*// ADS_PROOF_END[^\n]*\n", "", animation.read_text(encoding="utf-8"))
        animation.write_text(text, encoding="utf-8")
    label = "tk-animation-before-" + hashlib.sha256(str(output).encode()).hexdigest()[:8]
    original_runner = None
    if not args.current_source:
        # Keep every original acceptance assertion. The original cannot prove
        # candidate-only ADS/packed living rigs, so do not run it through newer
        # candidate assertions or remove those assertions from the real runner.
        original_runner = subprocess.check_output(["git", "show", f"{revision}:scripts/playtest.py"], cwd=root)
        (output / "scripts").mkdir()
        (output / "scripts/playtest.py").write_bytes(original_runner)
    for crate in ("common", "client", "server"):
        path = source / crate / "Cargo.toml"
        text = path.read_text().replace(f'name = "{crate}"', f'name = "{label}-{crate}"')
        text = text.replace('common = { path = "../common" }', f'common = {{ package = "{label}-common", path = "../common" }}')
        path.write_text(text, encoding="utf-8")
    files = sorted([*source.glob("**/*.rs"), *source.glob("**/Cargo.toml")])
    (output / "baseline-source.json").write_text(json.dumps({
        "revision": revision, "presentation_changes": args.current_source,
        "original_runner_sha256": hashlib.sha256(original_runner).hexdigest() if original_runner else None,
        "overlays": ["ordinary-input playtest driver", "native framebuffer video recorder", "unique Cargo package names"]
                    + (["matched camera projections only: world 70 / weapon 65 vertical degrees"] if args.match_current_camera else []),
        "sha256": {str(p.relative_to(source)): hashlib.sha256(p.read_bytes()).hexdigest() for p in files},
    }, indent=2), encoding="utf-8")
    env = os.environ.copy()
    env["CARGO_TARGET_DIR"] = str(root / "target")
    suffix = ".exe" if os.name == "nt" else ""
    for binary in ("server", "client"):
        subprocess.run(["cargo", "rustc", "--offline", "-p", f"{label}-{binary}", "--bin", f"{label}-{binary}", "--", "-o", str(output / f"{binary}{suffix}")], cwd=source, env=env, check=True)
    if not args.prepare_only:
        if args.current_source:
            command = ["python", str(root / "scripts/playtest.py"), "--clients", "2", "--video", "--animation-review", "--no-build", "--output", str(output / "dual"), "--server-path", str(output / f"server{suffix}"), "--client-path", str(output / f"client{suffix}")]
        else:
            command = ["python", str(root / "scripts/playtest_animation_baseline.py"), str(output), "--output", str(output / "dual"), "--clients", "2", "--video", "--animation-review"]
        subprocess.run(command, cwd=root, env=env, check=True)
    print(f"{'CANDIDATE' if args.current_source else 'BASELINE'} PREPARED {revision}: {output}")


if __name__ == "__main__":
    main()
