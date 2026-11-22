"""Render the shared voxel bakes into one deterministic inventory icon page."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import re
import struct
import time
import zlib

from bake_voxels import OUTPUT, ROOT, canonical, compose_pose

CELL = 96
COLUMNS = 3
BINDINGS = ROOT / "common/data/item-visuals.ron"
PAGE = ROOT / "assets/item-icons"
ROW = re.compile(r'\s*"([^"]+)": \(asset_key: "([a-z0-9_]+)", min: \([^)]*\), max: \([^)]*\)\),')


def bindings(raw):
    rows = [line for line in raw.decode().splitlines()
            if line.strip() and not line.startswith("//") and line.strip() not in ("{", "}")]
    pairs = []
    for row in rows:
        match = ROW.fullmatch(row)
        if not match:
            raise ValueError(f"unrecognized item visual binding: {row}")
        pairs.append(match.groups())
    if not pairs or len(dict(pairs)) != len(pairs):
        raise ValueError("empty or duplicate item visual bindings")
    return sorted(pairs)


def dot(a, b):
    return sum(x * y for x, y in zip(a, b))


def normalized(v):
    length = math.sqrt(dot(v, v))
    return tuple(x / length for x in v)


def cross(a, b):
    return tuple(a[i - 2] * b[i - 1] - a[i - 1] * b[i - 2] for i in range(3))


def edge(a, b, x, y):
    return (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0])


def icon(asset):
    eye = normalized((3, 3.3, -5))
    right = normalized(cross((0, 1, 0), eye))
    up = cross(eye, right)
    light = normalized((-.4, 1, -.65))
    parts = {part["id"]: part for part in asset["parts"]}
    materials = {mat["id"]: mat for mat in asset["materials"]}
    triangles = []
    for part in parts.values():
        pose = part["pivot"]
        parent = part["parent"]
        while parent is not None:
            pose = compose_pose(parts[parent]["pivot"], pose)
            parent = parts[parent]["parent"]
        for mesh in part["meshes"]:
            world = [compose_pose(pose, {"translation": p, "rotation": [0, 0, 0, 1]})["translation"]
                     for p in mesh["positions"]]
            projected = [(dot(p, right), -dot(p, up), dot(p, eye)) for p in world]
            color = materials[mesh["material"]]["color"]
            for index in range(0, len(mesh["indices"]), 3):
                i, j, k = mesh["indices"][index:index + 3]
                normal = normalized(cross(tuple(b-a for a, b in zip(world[i], world[j])),
                                          tuple(b-a for a, b in zip(world[i], world[k]))))
                if dot(normal, eye) > 0:
                    shade = 0.55 + 0.45 * max(0, dot(normal, light))
                    triangles.append(((projected[i], projected[j], projected[k]),
                                      tuple(c * shade for c in color[:3]), color[3]))
    points = [p for vertices, _, _ in triangles for p in vertices]
    mins = [min(p[axis] for p in points) for axis in range(2)]
    maxs = [max(p[axis] for p in points) for axis in range(2)]
    scale = (CELL - 16) / max(b - a for a, b in zip(mins, maxs))
    offset = [CELL / 2 - scale * (a + b) / 2 for a, b in zip(mins, maxs)]
    fragments = [[] for _ in range(CELL * CELL)]
    for vertices, color, alpha in triangles:
        a, b, c = [(x * scale + offset[0], y * scale + offset[1], z) for x, y, z in vertices]
        area = edge(a, b, *c[:2])
        for y in range(math.floor(min(a[1], b[1], c[1])), math.ceil(max(a[1], b[1], c[1]))):
            for x in range(math.floor(min(a[0], b[0], c[0])), math.ceil(max(a[0], b[0], c[0]))):
                weights = [edge(u, v, x + .5, y + .5) / area for u, v in ((b, c), (c, a), (a, b))]
                if min(weights) >= -1e-9:
                    depth = dot(weights, (a[2], b[2], c[2]))
                    fragments[y * CELL + x].append((round(depth, 10), color, alpha))
    rgba = bytearray()
    for pixel in fragments:
        acc = [0., 0., 0.]
        opacity = 0.
        for _, color, alpha in sorted(set(pixel)):
            acc = [old * (1 - alpha) + new * alpha for old, new in zip(acc, color)]
            opacity = opacity * (1 - alpha) + alpha
        rgba.extend(round(255 * v) for v in (*([v / opacity for v in acc] if opacity else acc), opacity))
    return rgba


def png(width, height, rgba):
    def chunk(kind, content):
        return struct.pack(">I", len(content)) + kind + content + struct.pack(">I", zlib.crc32(kind + content))
    rows = b"".join(b"\0" + rgba[start:start + width * 4] for start in range(0, len(rgba), width * 4))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b""))


def build(page=PAGE, *, check=False, inputs=BINDINGS, bakes=OUTPUT):
    digest = lambda data: hashlib.sha256(data).hexdigest()
    raw = inputs.read_bytes()
    sources = [(item, key, (bakes / f"{key}.json").read_bytes()) for item, key in bindings(raw)]
    width, height = CELL * COLUMNS, CELL * math.ceil(len(sources) / COLUMNS)
    manifest = {"width": width, "height": height, "rgba_bytes": width * height * 4,
                "baker_sha256": digest(Path(__file__).read_bytes()),
                "voxel_tools_sha256": digest(Path(__file__).with_name("bake_voxels.py").read_bytes()),
                "bindings_sha256": digest(raw),
                "items": {}}
    for i, (item, key, data) in enumerate(sources):
        asset = json.loads(data)
        manifest["items"][item] = {"asset": key, "bake_sha256": digest(data),
                                   "source_sha256": asset["source_sha256"],
                                   "rect": [i % COLUMNS * CELL, i // COLUMNS * CELL, CELL, CELL]}
    metadata, texture = page / "atlas.json", page / "atlas.png"
    if metadata.is_file() and texture.is_file():
        cached = json.loads(metadata.read_bytes())
        if cached.pop("texture_sha256", None) == digest(texture.read_bytes()) and cached == manifest:
            return "hit"
    if check:
        raise ValueError("missing or stale item icon atlas; run python scripts/bake_item_icons.py")
    rgba = bytearray(width * height * 4)
    for item, _, data in sources:
        pixels = icon(json.loads(data))
        x, y, _, _ = manifest["items"][item]["rect"]
        for row in range(CELL):
            start = ((y + row) * width + x) * 4
            rgba[start:start + CELL * 4] = pixels[row * CELL * 4:(row + 1) * CELL * 4]
    encoded = png(width, height, rgba)
    manifest["texture_sha256"] = digest(encoded)
    page.mkdir(parents=True, exist_ok=True)
    texture.write_bytes(encoded)
    metadata.write_bytes(canonical(manifest))
    return "miss"


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    start = time.perf_counter()
    print(f"ITEM ICONS cache={build(check=args.check)} elapsed_ms={(time.perf_counter() - start) * 1000:.1f}")
