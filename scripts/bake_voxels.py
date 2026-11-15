"""Bake bounded, pivot-local cuboid sources into deterministic indexed surfaces.

No third-party packages, editor, window or network required. This is an offline
asset build: movable parts are never fused and socket transforms are not snapped.
"""
import argparse
import hashlib
import itertools
import json
import math
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
SOURCES = ROOT / "assets/voxel"
OUTPUT = ROOT / "assets/baked"
MAX_BYTES = 1_048_576
MAX_CELLS = 262_144
MAX_VERTICES = 100_000
MAX_LOD_DEVIATION = 0.002
AXES = "right_handed_y_up_negative_z_forward"


class InvalidAsset(ValueError):
    pass


def require(condition, path, message):
    if not condition:
        raise InvalidAsset(f"{path}: {message}")


def fields(value, names, path, optional=""):
    require(isinstance(value, dict), path, "expected object")
    require(set(names.split()) <= set(value) <= set((names + " " + optional).split()), path,
            f"expected fields [{names}], got {sorted(value)}")


def number(value, low, high, path):
    require(type(value) in (int, float) and math.isfinite(value)
            and low <= value <= high, path, f"expected finite number in [{low}, {high}]")


def vector(value, size, low, high, path):
    require(isinstance(value, list) and len(value) == size, path, f"expected {size} values")
    for i, component in enumerate(value):
        number(component, low, high, f"{path}[{i}]")


def name(value, path):
    require(isinstance(value, str) and re.fullmatch(r"[a-z][a-z0-9_.-]{0,63}", value),
            path, "expected stable lowercase ID (1..64 characters)")


def sequence(value, low, high, path):
    require(isinstance(value, list) and low <= len(value) <= high,
            path, f"expected {low}..{high} entries")


def transform(value, path):
    fields(value, "translation rotation", path)
    vector(value["translation"], 3, -8, 8, f"{path}.translation")
    vector(value["rotation"], 4, -1, 1, f"{path}.rotation")
    require(abs(sum(v * v for v in value["rotation"]) - 1) <= 1e-6,
            f"{path}.rotation", "expected normalized xyzw quaternion")


def validate(source):
    fields(source, "version id units axes provenance materials parts sockets", "asset")
    require(type(source["version"]) is int and source["version"] == 1,
            "version", "unsupported version; expected 1")
    name(source["id"], "id")
    require(source["units"] == "metres", "units", "expected metres; implicit rescaling is forbidden")
    require(source["axes"] == AXES, "axes", f"expected {AXES}")
    require(isinstance(source["provenance"], str) and 1 <= len(source["provenance"]) <= 1024,
            "provenance", "expected original author/source/licence note")
    sequence(source["materials"], 1, 32, "materials")
    materials = {}
    for i, material in enumerate(source["materials"]):
        path = f"materials[{i}]"
        fields(material, "id surface color roughness metallic", path)
        name(material["id"], f"{path}.id")
        require(material["id"] not in materials, path, "duplicate material ID")
        require(material["surface"] in ("metal", "polymer", "wood", "rubber"),
                f"{path}.surface", "unknown surface family")
        vector(material["color"], 4, 0, 1, f"{path}.color")
        require(material["color"][3] == 1, f"{path}.color", "only opaque surfaces supported")
        number(material["roughness"], 0.04, 1, f"{path}.roughness")
        number(material["metallic"], 0, 1, f"{path}.metallic")
        materials[material["id"]] = material
    sequence(source["parts"], 1, 64, "parts")
    parts = {}
    for i, part in enumerate(source["parts"]):
        path = f"parts[{i}]"
        fields(part, "id parent pivot movable boxes", path, "far_boxes")
        name(part["id"], f"{path}.id")
        require(part["id"] not in parts, path, "duplicate part ID")
        require(part["parent"] is None or isinstance(part["parent"], str),
                f"{path}.parent", "expected part ID or null")
        require(type(part["movable"]) is bool, f"{path}.movable", "expected boolean")
        transform(part["pivot"], f"{path}.pivot")
        for level in ("boxes", "far_boxes"):
            if level not in part:
                continue
            sequence(part[level], 1, 256, f"{path}.{level}")
            for j, box in enumerate(part[level]):
                loc = f"{path}.{level}[{j}]"
                fields(box, "min max material", loc)
                vector(box["min"], 3, -8, 8, f"{loc}.min")
                vector(box["max"], 3, -8, 8, f"{loc}.max")
                require(all(b - a >= 0.00001 for a, b in zip(box["min"], box["max"])),
                        loc, "extent must be at least 0.01 mm on every axis")
                require(isinstance(box["material"], str) and box["material"] in materials,
                        f"{loc}.material", "unknown material reference")
        require(not part["movable"] or "far_boxes" not in part, path,
                "movable mechanisms retain near geometry at every LOD")
        parts[part["id"]] = part
    require(sum(p["parent"] is None for p in parts.values()) == 1,
            "parts", "expected exactly one root")
    for part in parts.values():
        seen = set()
        current = part
        while current is not None:
            require(current["id"] not in seen, f"parts.{part['id']}.parent", "cycle")
            seen.add(current["id"])
            require(len(seen) <= 16, f"parts.{part['id']}.parent", "depth exceeds 16")
            parent = current["parent"]
            require(parent is None or parent in parts, f"parts.{current['id']}.parent",
                    "unknown parent reference")
            current = parts.get(parent)
    sequence(source["sockets"], 0, 128, "sockets")
    sockets = set()
    for i, socket in enumerate(source["sockets"]):
        path = f"sockets[{i}]"
        fields(socket, "id part pose", path)
        name(socket["id"], f"{path}.id")
        require(socket["id"] not in sockets, path, "duplicate socket ID")
        sockets.add(socket["id"])
        require(isinstance(socket["part"], str) and socket["part"] in parts,
                f"{path}.part", "unknown part reference")
        transform(socket["pose"], f"{path}.pose")
    return parts


def surface(part):
    """Coordinate compression preserves partial cells and continuous dimensions.

    Fill the union of cuboids, reject ambiguous material overlaps, then greedily
    merge boundary cells with the same normal/material into rectangular faces.
    All loops are ordered; authoring order does not affect geometry.
    """
    axes = [sorted({box[edge][d] for box in part["boxes"] for edge in ("min", "max")})
            for d in range(3)]
    dimensions = [len(axis) - 1 for axis in axes]
    require(math.prod(dimensions) <= MAX_CELLS, f"parts.{part['id']}",
            f"compressed volume exceeds {MAX_CELLS} cells")
    indices = [{v: i for i, v in enumerate(axis)} for axis in axes]
    cells = {}
    for box in part["boxes"]:
        ranges = [range(indices[d][box["min"][d]], indices[d][box["max"][d]]) for d in range(3)]
        for cell in itertools.product(*ranges):
            previous = cells.get(cell)
            require(previous is None or previous == box["material"],
                    f"parts.{part['id']}.boxes", "overlapping solids have different materials")
            cells[cell] = box["material"]
    meshes = {}
    exposed = 0
    vertices = 0
    for d in range(3):
        u, v = (d + 1) % 3, (d + 2) % 3
        for plane in range(dimensions[d] + 1):
            mask = {}
            for j in range(dimensions[v]):
                for i in range(dimensions[u]):
                    cell = [0, 0, 0]
                    cell[d], cell[u], cell[v] = plane, i, j
                    positive = cells.get(tuple(cell))
                    cell[d] -= 1
                    negative = cells.get(tuple(cell))
                    if bool(negative) != bool(positive):
                        mask[i, j] = (negative or positive, 1 if negative else -1)
            exposed += len(mask)
            for j in range(dimensions[v]):
                for i in range(dimensions[u]):
                    key = mask.get((i, j))
                    if key is None:
                        continue
                    width = 1
                    while mask.get((i + width, j)) == key:
                        width += 1
                    height = 1
                    while all(mask.get((x, j + height)) == key for x in range(i, i + width)):
                        height += 1
                    for y in range(j, j + height):
                        for x in range(i, i + width):
                            del mask[x, y]
                    material, sign = key
                    vertices += 4
                    require(vertices <= MAX_VERTICES, f"parts.{part['id']}",
                            f"mesh exceeds {MAX_VERTICES} vertices")
                    mesh = meshes.setdefault(material, {"material": material, "positions": [],
                                                        "normals": [], "indices": []})
                    corners = [(i, j), (i + width, j), (i + width, j + height), (i, j + height)]
                    if sign < 0:
                        corners.reverse()
                    base = len(mesh["positions"])
                    normal = [0, 0, 0]
                    normal[d] = sign
                    for x, y in corners:
                        p = [0, 0, 0]
                        p[d], p[u], p[v] = axes[d][plane], axes[u][x], axes[v][y]
                        mesh["positions"].append(p)
                        mesh["normals"].append(normal.copy())
                    mesh["indices"].extend(base + n for n in (0, 1, 2, 0, 2, 3))
    for mesh in meshes.values():
        mesh["bounds"] = bounds(mesh["positions"])
    return [meshes[key] for key in sorted(meshes)], exposed


def bounds(points):
    return {"min": [min(p[d] for p in points) for d in range(3)],
            "max": [max(p[d] for p in points) for d in range(3)]}


def lod_deviation(part):
    """Conservative bidirectional solid Hausdorff bound, not a vertex sample.

    On the shared compressed grid occupancy is constant in every cell. For
    each differing cell, one target cuboid must contain all its points within
    the tolerance. Distance to a convex box reaches its maximum at a corner,
    so this also bounds the complete cell, including the interior.
    """
    if "far_boxes" not in part:
        return 0
    near, far = part["boxes"], part["far_boxes"]
    axes = [sorted({b[edge][d] for b in near + far for edge in ("min", "max")}) for d in range(3)]
    require(math.prod(len(a) - 1 for a in axes) <= MAX_CELLS,
            f"parts.{part['id']}.far_boxes", "LOD comparison exceeds compressed cell limit")
    error = 0
    for indices in itertools.product(*(range(len(a) - 1) for a in axes)):
        low = [axes[d][indices[d]] for d in range(3)]
        high = [axes[d][indices[d] + 1] for d in range(3)]
        center = [(a + b) / 2 for a, b in zip(low, high)]
        occupied = [any(all(b["min"][d] <= center[d] < b["max"][d] for d in range(3))
                        for b in boxes) for boxes in (near, far)]
        if occupied[0] == occupied[1]:
            continue
        target = far if occupied[0] else near
        corners = list(itertools.product(*zip(low, high)))
        distance = min(max(math.sqrt(sum(
            max(b["min"][d] - p[d], 0, p[d] - b["max"][d]) ** 2 for d in range(3)))
                           for p in corners) for b in target)
        error = max(error, distance)
        require(error <= MAX_LOD_DEVIATION + 1e-9, f"parts.{part['id']}.far_boxes",
                f"silhouette deviation {error:.6f} m exceeds {MAX_LOD_DEVIATION} m")
    return error


def canonical(value):
    return (json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False) + "\n").encode()


def bake(source, source_bytes):
    validate(source)
    baked = []
    manifest = []
    total_vertices = 0
    for part in sorted(source["parts"], key=lambda p: p["id"]):
        meshes, exposed = surface(part)
        deviation = lod_deviation(part)
        far_meshes, _ = surface({"id": part["id"], "boxes": part.get("far_boxes", part["boxes"])})
        require([m["material"] for m in meshes] == [m["material"] for m in far_meshes],
                f"parts.{part['id']}.far_boxes", "LOD must preserve visible material groups")
        positions = [point for mesh in meshes for point in mesh["positions"]]
        vertices = len(positions)
        total_vertices += vertices
        require(total_vertices <= MAX_VERTICES, "parts", f"asset exceeds {MAX_VERTICES} vertices")
        index_count = sum(len(mesh["indices"]) for mesh in meshes)
        item = {key: part[key] for key in ("id", "parent", "pivot", "movable")}
        far_vertices = sum(len(m["positions"]) for m in far_meshes)
        far_indices = sum(len(m["indices"]) for m in far_meshes)
        require(far_vertices <= vertices and far_indices <= index_count,
                f"parts.{part['id']}.far_boxes", "far LOD must not increase geometry cost")
        item.update(bounds=bounds(positions + [p for m in far_meshes for p in m["positions"]]),
                    meshes=meshes, far_meshes=far_meshes, lod_deviation=deviation)
        baked.append(item)
        manifest.append({"id": part["id"], "bounds": item["bounds"], "vertices": vertices,
                         "triangles": index_count // 3, "draw_calls": len(meshes),
                         "mesh_bytes": vertices * 24 + index_count * 4,
                         "exposed_cells": exposed, "quads": index_count // 6,
                         "far_vertices": far_vertices, "far_triangles": far_indices // 3,
                         "far_mesh_bytes": far_vertices * 24 + far_indices * 4,
                         "lod_deviation": deviation,
                         "cached_mesh_bytes": vertices * 24 + index_count * 4 + sum(
                             len(f["positions"]) * 24 + len(f["indices"]) * 4
                             for n, f in zip(meshes, far_meshes) if n != f)})
    sockets = sorted(source["sockets"], key=lambda s: s["id"])
    return {"version": 1, "baker_version": 2, "id": source["id"], "units": source["units"],
            "axes": source["axes"], "source_sha256": hashlib.sha256(source_bytes).hexdigest(),
            "provenance": source["provenance"],
            "materials": sorted(source["materials"], key=lambda m: m["id"]),
            "parts": baked, "sockets": sockets,
            "manifest": {"parts": manifest, "sockets": sockets,
                         **{k: sum(p[k] for p in manifest)
                            for k in ("vertices", "triangles", "draw_calls", "mesh_bytes",
                                      "far_vertices", "far_triangles", "far_mesh_bytes",
                                      "cached_mesh_bytes")},
                         "lod_deviation": max(p["lod_deviation"] for p in manifest)}}


def reject_duplicates(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, key, "duplicate JSON field")
        result[key] = value
    return result


def load(path):
    raw = path.read_bytes()
    require(len(raw) <= MAX_BYTES, str(path), f"source exceeds {MAX_BYTES} bytes")
    try:
        source = json.loads(raw, object_pairs_hook=reject_duplicates)
    except (ValueError, RecursionError) as error:
        raise InvalidAsset(f"{path}: {error}") from error
    try:
        return bake(source, raw)
    except InvalidAsset as error:
        raise InvalidAsset(f"{path}: {error}") from error


def compose_pose(parent, local):
    """Rigid composition for the headless logical export (xyzw, metres)."""
    def normalized(q):
        length = math.sqrt(sum(v * v for v in q))
        return [v / length for v in q]

    a = normalized(parent["rotation"])
    b = normalized(local["rotation"])
    x, y, z, w = a
    p = local["translation"]
    cross = [y * p[2] - z * p[1], z * p[0] - x * p[2], x * p[1] - y * p[0]]
    again = [y * cross[2] - z * cross[1], z * cross[0] - x * cross[2],
             x * cross[1] - y * cross[0]]
    translation = [parent["translation"][i] + p[i] + 2 * (w * cross[i] + again[i])
                   for i in range(3)]
    rotation = [
        w * b[0] + x * b[3] + y * b[2] - z * b[1],
        w * b[1] - x * b[2] + y * b[3] + z * b[0],
        w * b[2] + x * b[1] - y * b[0] + z * b[3],
        w * b[3] - x * b[0] - y * b[1] - z * b[2],
    ]
    return {"translation": translation, "rotation": normalized(rotation)}


def logical_rigs(assets):
    """Export only root-local sockets/pivots, never render buffers or materials."""
    identity = {"translation": [0, 0, 0], "rotation": [0, 0, 0, 1]}
    rigs = {}
    for asset in sorted(assets, key=lambda a: a["id"]):
        parts = {p["id"]: p for p in asset["parts"]}
        pivots = {}
        for key in sorted(parts):
            pose = compose_pose(identity, parts[key]["pivot"])
            parent = parts[key]["parent"]
            while parent is not None:
                pose = compose_pose(parts[parent]["pivot"], pose)
                parent = parts[parent]["parent"]
            pivots[key] = pose
        sockets = {s["id"]: compose_pose(pivots[s["part"]], s["pose"])
                   for s in sorted(asset["sockets"], key=lambda s: s["id"])}
        rigs[asset["id"]] = {"pivots": pivots, "sockets": sockets}
    return rigs


def logical_ron(assets):
    lines = ["// Generated by bake_voxels.py. Author coordinates in assets/voxel.", "{"]
    for asset, rig in logical_rigs(assets).items():
        lines.append(f'    "{asset}": (')
        for kind, poses in rig.items():
            lines.append(f"        {kind}: {{")
            for name, pose in poses.items():
                vectors = [f"{field}: ({', '.join(repr(float(v)) for v in pose[field])})"
                           for field in ("translation", "rotation")]
                lines.append(f'            "{name}": ({", ".join(vectors)}),')
            lines.append("        },")
        lines.append("    ),")
    lines.append("}")
    return ("\n".join(lines) + "\n").encode()


def build(sources=SOURCES, output=OUTPUT, check=False):
    paths = sorted(sources.glob("*.json"))
    require(bool(paths), str(sources), "no voxel sources")
    results = []
    ids = set()
    for path in paths:
        baked = load(path)
        require(baked["id"] not in ids, str(path), "duplicate asset ID")
        ids.add(baked["id"])
        results.append((path, baked, canonical(baked)))
    # Validate the complete set before writing anything. Never leave a partial
    # successful build when the last asset is invalid.
    extras = {p.stem for p in output.glob("*.json")} - ids
    require(not extras, str(output), f"orphaned bakes have no source: {sorted(extras)}")
    logical = logical_ron([baked for _, baked, _ in results])
    logical_path = output / "logical_rigs.ron"
    if check:
        require(logical_path.is_file() and logical_path.read_bytes() == logical,
                str(logical_path), "missing/stale logical rig; run python scripts/bake_voxels.py")
    for path, baked, data in results:
        destination = output / f"{baked['id']}.json"
        if check:
            require(destination.is_file() and destination.read_bytes() == data, str(destination),
                    "missing/stale bake; run python scripts/bake_voxels.py")
        else:
            output.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(data)
        costs = baked["manifest"]
        print(f"VOXEL {baked['id']} vertices={costs['vertices']} triangles={costs['triangles']} "
              f"draw_calls={costs['draw_calls']} mesh_bytes={costs['mesh_bytes']} source={path.name}")
    if not check:
        logical_path.write_bytes(logical)
    return results


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="reject stale committed bakes")
    parser.add_argument("--sources", type=Path, default=SOURCES)
    parser.add_argument("--output", type=Path, default=OUTPUT)
    args = parser.parse_args(argv)
    try:
        build(args.sources, args.output, args.check)
    except (InvalidAsset, OSError) as error:
        print(f"VOXEL BUILD FAILED: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
