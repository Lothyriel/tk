"""Index contact timestamps and adjacent real native frames without synthesizing art."""
import argparse
from datetime import datetime
import json
from pathlib import Path
import re
from PIL import Image, ImageDraw


def review(directory):
    text = (directory / "client.log").read_text(encoding="utf-8", errors="replace")
    start = re.search(r"PLAYTEST AnimationSetup -> Animation at ([\d.]+)s", text)
    assert start, "Continuous route stage clock missing"
    start = float(start[1])
    # Older recordings used a fixed peer offset; new routes log the actual
    # stage-relative clock while waiting for replicated mechanical outcomes.
    offset = 0.0 if "ANIMATION CLOCK stage-relative" in text else (29.0 if directory.name == "peer" else 0.0)
    frames = [(int(n), float(t)) for n, t in (line.split(",") for line in (directory / "frame-times.csv").read_text().splitlines())]
    contacts = []
    output = directory / "contact-review"
    output.mkdir(exist_ok=True)
    for name, local in re.findall(r"ANIMATION CONTACT ([\w-]+) local_t=([\d.]+)", text):
        at = start + float(local) + offset
        nearest = min(range(len(frames)), key=lambda i: abs(frames[i][1] - at))
        samples = frames[max(0, nearest - 3):nearest + 4]
        sheet = Image.new("RGB", (4 * 400, 2 * 280), "#14232c")
        draw = ImageDraw.Draw(sheet)
        for i, (number, time) in enumerate(samples):
            image = Image.open(directory / "frames" / f"{number:06}.png").convert("RGB")
            # Crop only the HUD-heavy top left; hands and gun stay inside the real image.
            image = image.crop((300, 240, 1280, 800))
            image.thumbnail((400, 250))
            x, y = (i % 4) * 400, (i // 4) * 280
            sheet.paste(image, (x, y + 24))
            draw.text((x + 5, y + 5), f"{name}  frame {number}  t={time:.3f}s", fill="white")
        path = output / f"{name}-adjacent.png"
        sheet.save(path)
        contacts.append({"action": name, "client_clock_seconds": at, "video_seconds": at - frames[0][1],
                         "nearest_real_frame": frames[nearest][0], "adjacent_samples": samples, "sheet": str(path)})
    assert len(contacts) == 17, "All weapon action contact timestamps must be present"
    (output / "timestamps.json").write_text(json.dumps(contacts, indent=2), encoding="utf-8")
    print(f"CONTACT INDEX {directory}: {len(contacts)} action timestamps, 7 adjacent real frames per sheet")


def observer_review(actor, observer):
    actor_log = (actor / "client.log").read_text(encoding="utf-8", errors="replace")
    observer_log = (observer / "client.log").read_text(encoding="utf-8", errors="replace")
    clock = re.search(r"(\S+) .*PLAYTEST Connect -> Equip at ([\d.]+)s", observer_log)
    assert clock, "observer wall-clock anchor missing"
    epoch = datetime.fromisoformat(clock[1]).timestamp() - float(clock[2])
    frames = [(int(n), float(t)) for n, t in (line.split(",") for line in (observer / "frame-times.csv").read_text().splitlines())]
    output = actor / "contact-review"
    rows = []
    for stamp, name, local in re.findall(r"(\S+) .*ANIMATION CONTACT ([\w-]+) local_t=([\d.]+)", actor_log):
        at = datetime.fromisoformat(stamp).timestamp() - epoch
        nearest = min(range(len(frames)), key=lambda i: abs(frames[i][1] - at))
        samples = frames[max(0, nearest - 3):nearest + 4]
        sheet = Image.new("RGB", (4 * 400, 2 * 280), "#14232c")
        draw = ImageDraw.Draw(sheet)
        for i, (number, time) in enumerate(samples):
            image = Image.open(observer / "frames" / f"{number:06}.png").convert("RGB")
            image.thumbnail((400, 250))
            x, y = (i % 4) * 400, (i // 4) * 280
            sheet.paste(image, (x, y + 24))
            draw.text((x + 5, y + 5), f"observer {name} frame {number} t={time:.3f}s", fill="white")
        path = output / f"{name}-observer-adjacent.png"
        sheet.save(path)
        rows.append({"action": name, "observer_video_seconds": at - frames[0][1], "adjacent_samples": samples, "sheet": str(path)})
    assert len(rows) == 17
    (output / "observer-timestamps.json").write_text(json.dumps(rows, indent=2), encoding="utf-8")
    print(f"OBSERVER INDEX {actor}: 17 action points in the other real UDP client's framebuffer")


def movement_review(directory):
    text = (directory / "client.log").read_text(encoding="utf-8", errors="replace")
    clock = re.search(r"(\S+) .*PLAYTEST Connect -> Equip at ([\d.]+)s", text)
    assert clock, "Native wall-clock anchor missing"
    epoch = datetime.fromisoformat(clock[1]).timestamp() - float(clock[2])
    frames = [(int(n), float(t)) for n, t in (line.split(",") for line in (directory / "frame-times.csv").read_text().splitlines())]
    events = re.findall(r"(\S+) .*(?:POSE EVIDENCE|INJURY EVIDENCE|ADS EVIDENCE|RELOAD MECHANISM EVIDENCE) ([\w-]+) ", text)
    events += [(stamp, "live-" + name) for stamp, name in re.findall(r"(\S+) .*LIVE RECOVERY EVIDENCE (fall|getup) ", text)]
    output = directory / "movement-review"
    output.mkdir(exist_ok=True)
    rows = []
    for stamp, name in events:
        at = datetime.fromisoformat(stamp).timestamp() - epoch
        nearest = min(range(len(frames)), key=lambda i: abs(frames[i][1] - at))
        samples = frames[max(0, nearest - 3):nearest + 4]
        sheet = Image.new("RGB", (1600, 560), "#14232c")
        draw = ImageDraw.Draw(sheet)
        for i, (number, time) in enumerate(samples):
            image = Image.open(directory / "frames" / f"{number:06}.png").convert("RGB")
            image.thumbnail((400, 250))
            x, y = (i % 4) * 400, (i // 4) * 280
            sheet.paste(image, (x, y + 24))
            draw.text((x + 5, y + 5), f"{name} frame {number} t={time:.3f}s", fill="white")
        path = output / f"{name}-adjacent.png"
        sheet.save(path)
        row = {"action": name, "video_seconds": at - frames[0][1], "adjacent_samples": samples, "sheet": str(path)}
        if name in ("live-fall", "live-getup"):
            duration = 2.2 if name == "live-fall" else 1.5
            interval = [min(frames, key=lambda f: abs(f[1] - (at - 0.2 + duration * i / 11))) for i in range(12)]
            sheet = Image.new("RGB", (1600, 840), "#14232c")
            draw = ImageDraw.Draw(sheet)
            for i, (number, time) in enumerate(interval):
                image = Image.open(directory / "frames" / f"{number:06}.png").convert("RGB")
                image.thumbnail((400, 250))
                x, y = (i % 4) * 400, (i // 4) * 280
                sheet.paste(image, (x, y + 24))
                draw.text((x + 5, y + 5), f"{name} frame {number} t={time:.3f}s", fill="white")
            path = output / f"{name}-full-interval.png"
            sheet.save(path)
            row.update(interval_samples=interval, interval_sheet=str(path))
        rows.append(row)
    (output / "timestamps.json").write_text(json.dumps(rows, indent=2), encoding="utf-8")
    print(f"MOVEMENT INDEX {directory}: {len(rows)} pose/injury/get-up points")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    review(args.directory)
    movement_review(args.directory)
    if (args.directory / "peer").is_dir():
        review(args.directory / "peer")
        movement_review(args.directory / "peer")
        observer_review(args.directory, args.directory / "peer")
        observer_review(args.directory / "peer", args.directory)
