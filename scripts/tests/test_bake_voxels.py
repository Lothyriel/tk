"""Geometry/authoring acceptance oracles independent of the meshing algorithm."""
import copy
import hashlib
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from contextlib import redirect_stdout

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import bake_voxels as bake


def source():
    return json.loads((bake.SOURCES / "pipeline_fixture.json").read_text())


def cube_source():
    asset = source()
    asset["parts"] = [asset["parts"][0]]
    asset["parts"][0]["boxes"] = [{"min": [0, 0, 0], "max": [1, 1, 1], "material": "steel"}]
    asset["sockets"] = []
    return asset


def compiled(asset):
    return bake.bake(asset, bake.canonical(asset))


def metrics(asset):
    """Compute triangle area, signed volume and winding from emitted buffers."""
    area = volume = 0
    for part in asset["parts"]:
        for mesh in part["meshes"]:
            vertices = mesh["positions"]
            for i in range(0, len(mesh["indices"]), 3):
                ia, ib, ic = mesh["indices"][i:i + 3]
                a, b, c = (vertices[n] for n in (ia, ib, ic))
                ab, ac = ([b[d] - a[d] for d in range(3)], [c[d] - a[d] for d in range(3)])
                cross = [ab[1] * ac[2] - ab[2] * ac[1],
                         ab[2] * ac[0] - ab[0] * ac[2],
                         ab[0] * ac[1] - ab[1] * ac[0]]
                assert sum(x * n for x, n in zip(cross, mesh["normals"][ia])) > 0
                area += sum(x * x for x in cross) ** 0.5 / 2
                volume += sum(a[d] * cross[d] for d in range(3)) / 6
    return area, volume


class VoxelBakeTests(unittest.TestCase):
    def test_union_removes_internal_faces_and_merges_without_losing_steps(self):
        asset = cube_source()
        asset["parts"][0]["boxes"].append(
            {"min": [1, 0, 0], "max": [2, 1, 1], "material": "steel"})
        result = compiled(asset)
        self.assertEqual(result["manifest"]["vertices"], 24)
        self.assertEqual(result["manifest"]["triangles"], 12)
        self.assertEqual(result["parts"][0]["bounds"], {"min": [0, 0, 0], "max": [2, 1, 1]})
        area, volume = metrics(result)
        self.assertAlmostEqual(area, 10)
        self.assertAlmostEqual(volume, 2)
        # An L, not its bounding box: concavity must survive greedy rectangles.
        asset["parts"][0]["boxes"].append(
            {"min": [0, 1, 0], "max": [1, 2, 1], "material": "steel"})
        area, volume = metrics(compiled(asset))
        self.assertAlmostEqual(area, 14)
        self.assertAlmostEqual(volume, 3)

    def test_overlaps_partial_boundaries_and_material_interfaces(self):
        asset = cube_source()
        asset["parts"][0]["boxes"].append(
            {"min": [0.5, 0, 0], "max": [1.3683, 1, 1], "material": "steel"})
        area, volume = metrics(compiled(asset))
        self.assertAlmostEqual(area, 2 + 4 * 1.3683)
        self.assertAlmostEqual(volume, 1.3683)
        asset["parts"][0]["boxes"][1]["material"] = "wood"
        with self.assertRaisesRegex(bake.InvalidAsset, "overlapping"):
            compiled(asset)
        asset["parts"][0]["boxes"][1]["min"][0] = 1
        result = compiled(asset)
        self.assertEqual(result["manifest"]["draw_calls"], 2)
        area, volume = metrics(result)
        self.assertAlmostEqual(area, 2 + 4 * 1.3683)
        self.assertAlmostEqual(volume, 1.3683)

    def test_movable_parts_are_independent_even_at_a_shared_surface(self):
        asset = cube_source()
        other = copy.deepcopy(asset["parts"][0])
        other.update(id="bolt", parent="receiver", movable=True)
        other["pivot"]["translation"] = [0.0003, 1, 0]
        asset["parts"].append(other)
        result = compiled(asset)
        self.assertEqual(result["manifest"]["vertices"], 48)
        self.assertEqual(result["manifest"]["draw_calls"], 2)
        self.assertEqual(result["parts"][0]["pivot"], other["pivot"])
        self.assertTrue(result["parts"][0]["movable"])

    def test_fixture_preserves_continuous_pivots_sockets_and_deterministic_order(self):
        asset = source()
        first = compiled(asset)
        self.assertEqual(first["sockets"], sorted(asset["sockets"], key=lambda s: s["id"]))
        for part in first["parts"]:
            original = next(p for p in asset["parts"] if p["id"] == part["id"])
            self.assertEqual(part["pivot"], original["pivot"])
        asset["parts"].reverse()
        asset["materials"].reverse()
        for part in asset["parts"]:
            part["boxes"].reverse()
        second = compiled(asset)
        self.assertNotEqual(first["source_sha256"], second["source_sha256"])
        first.pop("source_sha256")
        second.pop("source_sha256")
        self.assertEqual(bake.canonical(first), bake.canonical(second))
        # Repeat the same input: identical full artifact, including hash.
        self.assertEqual(bake.canonical(compiled(asset)), bake.canonical(compiled(asset)))

    def test_invalid_authoring_reports_field_before_any_mesh_build(self):
        mutations = [
            ("units", lambda a: a.update(units="millimetres")),
            ("axes", lambda a: a.update(axes="z_up")),
            ("version", lambda a: a.update(version=True)),
            ("rotation", lambda a: a["parts"][1]["pivot"].update(rotation=[0, 0, 0, 0])),
            ("translation", lambda a: a["parts"][1]["pivot"].update(translation=[float("nan"), 0, 0])),
            ("unknown material", lambda a: a["parts"][0]["boxes"][0].update(material="gold")),
            ("extent", lambda a: a["parts"][0]["boxes"][0].update(max=[-1, -1, -1])),
            ("unknown parent", lambda a: a["parts"][1].update(parent="missing")),
            ("cycle", lambda a: a["parts"][1].update(parent="bolt")),
            ("unknown part", lambda a: a["sockets"][0].update(part="missing")),
            ("duplicate socket", lambda a: a["sockets"].append(a["sockets"][0])),
            ("unknown surface", lambda a: a["materials"][0].update(surface="glass")),
            ("expected fields", lambda a: a["parts"][0].update(typo=4)),
        ]
        for diagnostic, mutate in mutations:
            with self.subTest(diagnostic=diagnostic):
                asset = source()
                mutate(asset)
                with self.assertRaisesRegex(bake.InvalidAsset, diagnostic):
                    bake.bake(asset, b"invalid test input")

    def test_limits_and_duplicate_json_keys_are_rejected(self):
        asset = cube_source()
        asset["parts"][0]["boxes"] = [
            {"min": [i / 100, i / 100, i / 100],
             "max": [(i + 0.5) / 100] * 3, "material": "steel"} for i in range(40)]
        with self.assertRaisesRegex(bake.InvalidAsset, "compressed volume"):
            compiled(asset)
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "invalid.json"
            path.write_text('{"version":1,"version":2}')
            with self.assertRaisesRegex(bake.InvalidAsset, "duplicate JSON"):
                bake.load(path)

    def test_manifest_costs_match_buffers_and_committed_bake_is_current(self):
        with redirect_stdout(io.StringIO()):
            results = bake.build(check=True)
        self.assertTrue(results)
        for path, result, _ in results:
            self.assertEqual(result["source_sha256"], hashlib.sha256(path.read_bytes()).hexdigest())
            vertices = triangles = calls = memory = 0
            for part in result["parts"]:
                for mesh in part["meshes"]:
                    vertices += len(mesh["positions"])
                    triangles += len(mesh["indices"]) // 3
                    calls += 1
                    memory += len(mesh["positions"]) * 24 + len(mesh["indices"]) * 4
            self.assertEqual((vertices, triangles, calls, memory),
                             tuple(result["manifest"][k] for k in
                                   ("vertices", "triangles", "draw_calls", "mesh_bytes")))

    def test_build_rejects_stale_bake_and_invalid_batch_does_not_write(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            sources, output = root / "sources", root / "output"
            sources.mkdir()
            (sources / "a.json").write_bytes(bake.canonical(source()))
            (sources / "z.json").write_text("{}")
            with self.assertRaises(bake.InvalidAsset):
                bake.build(sources, output)
            self.assertFalse(output.exists())
            (sources / "z.json").unlink()
            with redirect_stdout(io.StringIO()):
                bake.build(sources, output)
                bake.build(sources, output, check=True)
            (output / "pipeline_fixture.json").write_text("{}")
            with self.assertRaisesRegex(bake.InvalidAsset, "stale bake"):
                bake.build(sources, output, check=True)
            (output / "orphan.json").write_text("{}")
            with self.assertRaisesRegex(bake.InvalidAsset, "orphaned bakes"):
                bake.build(sources, output, check=True)
