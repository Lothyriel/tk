"""Encode real native framebuffer captures, preserving their wall-clock timestamps.

Requires imageio-ffmpeg (or --ffmpeg). Keeps source frames for frame-by-frame review.
"""
import argparse
import json
from pathlib import Path
import statistics
import subprocess
import struct


def encode(directory, ffmpeg):
    rows = [line.split(",") for line in (directory / "frame-times.csv").read_text().splitlines()]
    requested = len(rows)
    end = float(rows[-1][1])
    # Old recorder builds may exit before the last readback completes. Never fill a
    # missing interior frame; trim only the uncaptured tail and disclose the cut.
    while rows and not (directory / "frames" / f"{int(rows[-1][0]):06}.png").exists():
        rows.pop()
    assert rows and end - float(rows[-1][1]) < 1.0, "More than one second of missing capture tail"
    if len(rows) != requested:
        print(f"CAPTURE TAIL: {requested - len(rows)} incomplete final readbacks trimmed ({end - float(rows[-1][1]):.3f}s); original sidecar retained")
    assert len(rows) > 30, f"Too few native video frames: {directory}"
    manifest = directory / "frames.ffconcat"
    lines = ["ffconcat version 1.0"]
    for index, (number, timestamp) in enumerate(rows):
        path = directory / "frames" / f"{int(number):06}.png"
        assert path.exists(), f"Missing native frame {path}"
        with path.open("rb") as image:
            header = image.read(24)
        assert header[:8] == b"\x89PNG\r\n\x1a\n", f"Invalid native PNG: {path}"
        width, height = struct.unpack(">II", header[16:24])
        assert width >= 600 and height >= 400, f"Minimized/invalid native surface {width}x{height}: {path}"
        duration = float(rows[index + 1][1]) - float(timestamp) if index + 1 < len(rows) else 1 / 30
        lines.extend([f"file '{path.as_posix()}'", f"duration {duration:.6f}"])
    manifest.write_text("\n".join(lines) + "\n", encoding="utf-8")
    destination = directory / "raid-review.mp4"
    subprocess.run([ffmpeg, "-hide_banner", "-loglevel", "warning", "-y", "-safe", "0",
                    "-i", str(manifest), "-vf", "fps=30", "-c:v", "libx264", "-crf", "18",
                    "-pix_fmt", "yuv420p", "-movflags", "+faststart", str(destination)], check=True)
    subprocess.run([ffmpeg, "-hide_banner", "-loglevel", "error", "-i", str(destination), "-f", "null", "-"], check=True)
    intervals = sorted(float(b[1]) - float(a[1]) for a, b in zip(rows, rows[1:]))
    (directory / "video-evidence.json").write_text(json.dumps({
        "file": str(destination), "bytes": destination.stat().st_size,
        "requested_samples": requested, "real_captured_samples": len(rows),
        "trimmed_tail_seconds": end - float(rows[-1][1]),
        "first_sample_seconds": float(rows[0][1]), "last_sample_seconds": float(rows[-1][1]),
        "sample_interval_median_ms": statistics.median(intervals) * 1000,
        "sample_interval_p95_ms": intervals[int(len(intervals) * 0.95)] * 1000,
        "sample_interval_max_ms": max(intervals) * 1000,
        "encoding": "30 fps CFR; holds captured frames between real-time samples; no motion synthesis",
        "full_ffmpeg_decode": "PASS",
    }, indent=2), encoding="utf-8")
    print(f"VIDEO {destination}: {len(rows)} real framebuffer samples, {float(rows[-1][1]) - float(rows[0][1]):.2f}s")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--ffmpeg")
    args = parser.parse_args()
    if not args.ffmpeg:
        import imageio_ffmpeg
        args.ffmpeg = imageio_ffmpeg.get_ffmpeg_exe()
    encode(args.directory.resolve(), args.ffmpeg)
    if (args.directory / "peer" / "frame-times.csv").exists():
        encode((args.directory / "peer").resolve(), args.ffmpeg)
