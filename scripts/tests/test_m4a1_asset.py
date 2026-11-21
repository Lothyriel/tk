"""Independent dimensional/clearance oracles for the authored M4A1 asset.

These checks qualify source geometry, not its pending native artistic review.
"""
import itertools
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import bake_voxels


ROOT = Path(__file__).resolve().parents[2]


def load(name):
    return json.loads((ROOT / "assets" / "voxel" / f"{name}.json").read_text())


def rotate(q, p):
    x, y, z, w = q
    # Quaternion vector rotation, independent of the production baker.
    cross = [y * p[2] - z * p[1], z * p[0] - x * p[2], x * p[1] - y * p[0]]
    again = [y * cross[2] - z * cross[1], z * cross[0] - x * cross[2],
             x * cross[1] - y * cross[0]]
    return [p[i] + 2 * (w * cross[i] + again[i]) for i in range(3)]


def point(parts, part_id, p):
    while part_id is not None:
        part = parts[part_id]
        p = [v + t for v, t in zip(rotate(part["pivot"]["rotation"], p),
                                   part["pivot"]["translation"])]
        part_id = part["parent"]
    return p


def bounds(parts, part_id, box):
    corners = [point(parts, part_id, p)
               for p in itertools.product(*zip(box["min"], box["max"]))]
    return [[fn(p[i] for p in corners) for i in range(3)] for fn in (min, max)]


def intersects(a, b):
    return all(min(a[1][i], b[1][i]) - max(a[0][i], b[0][i]) > 1e-8
                for i in range(3))


def asset_solids(asset, translation=(0, 0, 0)):
    parts = {p["id"]: p for p in asset["parts"]}
    return [[[v + t for v, t in zip(edge, translation)] for edge in bounds(parts, p["id"], b)]
            for p in parts.values() for b in p["boxes"]]


class M4A1AssetTests(unittest.TestCase):
    def setUp(self):
        self.asset = load("m4a1_refined")
        self.parts = {p["id"]: p for p in self.asset["parts"]}
        self.sockets = {s["id"]: point(self.parts, s["part"], s["pose"]["translation"])
                        for s in self.asset["sockets"]}

    def test_published_lengths_and_continuous_sight_axis(self):
        solids = [bounds(self.parts, key, box) for key, part in self.parts.items()
                  for box in part["boxes"]]
        front = min(b[0][2] for b in solids)
        rear = max(b[1][2] for b in solids)
        self.assertAlmostEqual(rear - front, 33 * 0.0254, delta=0.001)
        self.assertAlmostEqual(rear - 0.08255 - front, 29.75 * 0.0254, delta=0.001)
        self.assertAlmostEqual(self.sockets["bore.breech"][2]
                               - self.sockets["bore.muzzle"][2], 14.5 * 0.0254, delta=0.001)
        self.assertAlmostEqual(self.sockets["muzzle_device.exit"][2], front, delta=1e-6)
        self.assertAlmostEqual(self.sockets["stock.shoulder"][2], rear, delta=1e-6)
        for name in ("sight.front", "sight.rear", "sight.eye"):
            self.assertAlmostEqual(self.sockets[name][0], 0, delta=0.00025)
            self.assertAlmostEqual(self.sockets[name][1], 0.066, delta=0.00025)

    def test_ejection_aperture_is_open_and_controls_are_independent(self):
        # 65 x 18 mm unobstructed right receiver opening, excluding the actual
        # internal moving carrier. A painted face or filled receiver fails.
        aperture = [[0.013, -0.009, 0.015], [0.032, 0.009, 0.080]]
        for box in self.parts["receiver"]["boxes"]:
            self.assertFalse(intersects(bounds(self.parts, "receiver", box), aperture))
        self.assertEqual(self.parts["bolt"]["parent"], "receiver")
        for name in ("charging_handle", "control", "dust_cover", "trigger", "selector",
                     "forward_assist", "magazine_release"):
            self.assertTrue(self.parts[name]["movable"])
            parent = self.parts[name]["parent"]
            while parent:
                self.assertNotEqual(parent, "bolt", f"{name} must not reciprocate with the carrier")
                parent = self.parts[parent]["parent"]
        # The rear aperture stays hollow on the sight line, rather than becoming
        # a solid rectangle hidden by a centred socket.
        ray = [[-0.001, 0.065, 0.135], [0.001, 0.067, 0.150]]
        for box in self.parts["rear_sight"]["boxes"]:
            self.assertFalse(intersects(bounds(self.parts, "rear_sight", box), ray))

    def test_magazine_removal_sweep_clears_real_magwell_surfaces(self):
        seat = self.sockets["magazine.seat"]
        walls = [bounds(self.parts, "lower", box) for box in self.parts["lower"]["boxes"]]
        # Full continuous downward sweep is an AABB for this translation. It
        # covers every intermediate position, not only selected animation frames.
        for name in ("m4a1_magazine_30", "m4a1_pmag_20"):
            with self.subTest(magazine=name):
                magazine = load(name)
                parts = {p["id"]: p for p in magazine["parts"]}
                for part in magazine["parts"]:
                    for box in part["boxes"]:
                        local = bounds(parts, part["id"], box)
                        swept = [[local[edge][i] + seat[i] for i in range(3)]
                                 for edge in range(2)]
                        swept[0][1] -= 0.24
                        for wall in walls:
                            self.assertFalse(intersects(swept, wall), (swept, wall))

    def test_pmag20_published_length_open_feed_lips_and_surface_contact(self):
        magazine = load("m4a1_pmag_20")
        parts = {p["id"]: p for p in magazine["parts"]}
        solids = [bounds(parts, p["id"], box) for p in parts.values() for box in p["boxes"]]
        self.assertAlmostEqual(max(b[1][1] for b in solids) - min(b[0][1] for b in solids),
                               5.6 * 0.0254, delta=0.001)
        # A visible gap above the follower; a solid block with painted lips fails.
        feed = [[-0.006, 0.020, -0.017], [0.006, 0.026, 0.025]]
        self.assertTrue(all(not intersects(solid, feed) for solid in solids))
        grip = next(s for s in magazine["sockets"] if s["id"] == "magazine.grasp")
        contact = point(parts, grip["part"], grip["pose"]["translation"])
        # Contact must lie on an exterior face, not merely somewhere in the bounds.
        self.assertTrue(any(
            all(b[0][i] <= contact[i] <= b[1][i] for i in range(3))
            and any(abs(contact[i] - b[edge][i]) < 1e-6
                    for edge in range(2) for i in range(3))
            for b in solids))
        base_grip = next(s for s in load("m4a1_magazine_30")["sockets"]
                         if s["id"] == "magazine.grasp")["pose"]["translation"]
        self.assertGreater(sum((a - b) ** 2 for a, b in zip(contact, base_grip)), 0.003 ** 2)

    def test_modular_exports_partition_base_geometry_without_moving_surfaces(self):
        exports = list(bake_voxels.exported_sources(self.asset))
        seen = []
        for selection, asset in zip(self.asset["exports"], exports):
            parts = {p["id"]: p for p in asset["parts"]}
            seen.extend(parts)
            for key, part in parts.items():
                original = self.parts[key]
                self.assertEqual(part["boxes"], original["boxes"])
                self.assertEqual(part.get("far_boxes"), original.get("far_boxes"))
                self.assertEqual(part["movable"], original["movable"])
                for box in part["boxes"]:
                    for corner in itertools.product(*zip(box["min"], box["max"])):
                        local = point(parts, key, corner)
                        reassembled = point(self.parts, selection["root"], local)
                        expected = point(self.parts, key, corner)
                        for a, b in zip(reassembled, expected):
                            self.assertAlmostEqual(a, b, delta=1e-8)
        # Every surface has exactly one runtime owner, including all controls.
        self.assertCountEqual(seen, list(self.parts))

    def test_devices_have_open_outlets_and_suppressor_clears_its_real_mount(self):
        brake, can = (load(name) for name in ("m4a1_sfmb", "m4a1_rc2"))
        for asset, length in ((brake, 2.6 * 0.0254), (can, 6.4 * 0.0254)):
            solids = asset_solids(asset)
            self.assertAlmostEqual(max(b[1][2] for b in solids) - min(b[0][2] for b in solids),
                                   length, delta=1e-6)
            bore = [[-0.0028, -0.0028, -length], [0.0028, 0.0028, 0]]
            self.assertTrue(all(not intersects(b, bore) for b in solids))
            outlet = next(s for s in asset["sockets"] if s["id"] == "muzzle_device.exit")
            self.assertEqual(outlet["pose"]["translation"], [0, 0, -length])
        can_solids = asset_solids(can)
        for axis in (0, 1):
            self.assertAlmostEqual(max(b[1][axis] for b in can_solids)
                                   - min(b[0][axis] for b in can_solids),
                                   1.5 * 0.0254, delta=1e-6)
        mount = next(s for s in brake["sockets"] if s["id"] == "suppressor.mount")
        installed = asset_solids(can, mount["pose"]["translation"])
        self.assertTrue(all(not intersects(a, b) for a in installed for b in asset_solids(brake)))
        # The can surrounds the adapter. Its published approximate added length
        # is not the full can length added after the brake's front face.
        self.assertAlmostEqual(-min(b[0][2] for b in installed) - 2.6 * 0.0254,
                               3.8 * 0.0254, delta=1e-6)
        for z0, z1 in ((-0.061, -0.051), (-0.045, -0.036)):
            port = [[-0.012, -0.004, z0], [0.012, 0.004, z1]]
            self.assertTrue(all(not intersects(b, port) for b in asset_solids(brake)))

    def test_pro_mount_axis_open_sight_line_and_separate_surfaces(self):
        mount, optic = (load(name) for name in ("m4a1_qrp2", "m4a1_pro"))
        socket = next(s for s in mount["sockets"] if s["id"] == "optic.mount")
        parts = {p["id"]: p for p in mount["parts"]}
        seat = point(parts, socket["part"], socket["pose"]["translation"])
        self.assertEqual(seat, [0, 0.039, 0.030])
        mount_solids, optic_solids = asset_solids(mount), asset_solids(optic, seat)
        self.assertTrue(all(not intersects(a, b) for a in mount_solids for b in optic_solids))
        sight = [[-0.010, 0.029, -0.037], [0.010, 0.049, 0.141]]
        self.assertTrue(all(not intersects(b, sight) for b in mount_solids + optic_solids))
        optic_parts = {p["id"]: p for p in optic["parts"]}
        markers = {s["id"]: s for s in optic["sockets"]}
        bounds = []
        for name in ("sight.aperture_min", "sight.aperture_max"):
            marker = markers[name]
            bounds.append([a + b for a, b in zip(
                point(optic_parts, marker["part"], marker["pose"]["translation"]), seat
            )])
        self.assertTrue(all(lo < hi for lo, hi in zip(*bounds)))
        self.assertTrue(all(not intersects(b, bounds) for b in mount_solids + optic_solids))
        self.assertGreater(bounds[1][2] + 1e-8, max(b[1][2] for b in optic_solids))
        self.assertLess(bounds[0][2], min(b[1][2] for b in optic_solids))
        self.assertAlmostEqual(max(b[1][2] for b in optic_solids)
                               - min(b[0][2] for b in optic_solids), 0.132, delta=1e-6)
        all_solids = mount_solids + optic_solids
        self.assertAlmostEqual(max(b[1][0] for b in all_solids)
                               - min(b[0][0] for b in all_solids), 0.072, delta=1e-6)

    def test_short_barrel_keeps_carbine_front_sight_and_moves_device(self):
        short = load("m4a1_dd_103")
        sockets = {s["id"]: s["pose"]["translation"] for s in short["sockets"]}
        self.assertAlmostEqual(sockets["bore.breech"][2] - sockets["bore.muzzle"][2],
                               10.3 * 0.0254, delta=1e-6)
        self.assertEqual(sockets["front_sight.mount"], self.sockets["front_sight.mount"])
        self.assertAlmostEqual(sockets["muzzle_device.mount"][2]
                               - self.sockets["muzzle_device.mount"][2], 4.2 * 0.0254, delta=1e-6)
        # Keep the external barrel inside the authored handguard's open channel.
        handguard = next(a for a in bake_voxels.exported_sources(self.asset) if a["id"] == "m4a1_ras")
        shell = asset_solids(handguard, self.sockets["handguard.mount"])
        self.assertTrue(all(not intersects(a, b) for a in shell for b in asset_solids(short)))

    def test_ras_current_body_length_and_exposed_bottom_rail(self):
        ras = next(a for a in bake_voxels.exported_sources(self.asset) if a["id"] == "m4a1_ras")
        solids = asset_solids(ras)
        self.assertAlmostEqual(max(b[1][2] for b in solids) - min(b[0][2] for b in solids),
                               6.25 * 0.0254, delta=1e-6)
        # Open slot at the bottom; a polymer panel across the rail fails.
        slot = [[-0.010, -0.027, -0.088], [0.010, -0.023, -0.084]]
        self.assertTrue(all(not intersects(b, slot) for b in solids))
        # The reference has side vents above the ribbed panels.
        vent = [[-0.028, 0.009, -0.028], [0.028, 0.015, -0.010]]
        self.assertTrue(all(not intersects(b, vent) for b in solids))
        for actual, expected in zip(self.sockets["grip.support"], [0, -0.027, -0.110]):
            self.assertAlmostEqual(actual, expected, delta=1e-8)

    def test_handguard_front_cap_fit_and_short_suppressed_sl_interference(self):
        exports = {a["id"]: a for a in bake_voxels.exported_sources(self.asset)}
        tower = asset_solids(exports["m4a1_front_iron"], self.sockets["front_sight.mount"])
        short = load("m4a1_dd_103")
        mount = next(s for s in short["sockets"] if s["id"] == "muzzle_device.mount")
        can = asset_solids(load("m4a1_rc2"), mount["pose"]["translation"])
        for guard, can_fits in ((exports["m4a1_ras"], True), (load("m4a1_moe_sl"), False)):
            shell = asset_solids(guard, self.sockets["handguard.mount"])
            self.assertTrue(all(not intersects(a, b) for a in shell for b in tower))
            self.assertEqual(all(not intersects(a, b) for a in shell for b in can), can_fits)
        # This real exterior interference must become a declarative clearance
        # rejection in the catalog matrix, not a weapon/component deny-list.

    def test_grip_rail_endpoints_clear_both_actual_magazine_sweeps(self):
        rail = self.sockets["rail.bottom"]
        for name, positions in (("m4a1_rvg", 10), ("m4a1_afg2", 3)):
            for position in range(positions):
                placement = [rail[0], rail[1], rail[2] + position * 0.01]
                grip = asset_solids(load(name), placement)
                for magazine in ("m4a1_magazine_30", "m4a1_pmag_20"):
                    swept = asset_solids(load(magazine), self.sockets["magazine.seat"])
                    for box in swept:
                        box[0][1] -= 0.24
                    self.assertTrue(all(not intersects(a, b) for a in grip for b in swept),
                                    (name, position, magazine))
                # Attachment frames start at the occupied footprint's front.
                local = asset_solids(load(name))
                self.assertAlmostEqual(min(b[0][2] for b in local), 0, delta=1e-8)
                self.assertGreaterEqual(min(b[0][2] for b in grip), -0.176)
                self.assertLessEqual(max(b[1][2] for b in grip), -0.030)

    def test_furniture_published_envelopes_openings_and_contacts(self):
        # Published dimensions, independently converted from inches.
        cases = [
            ("m4a1_ctr", [1.7, 5.2, 7.0], "stock.shoulder",
             [[-0.012, -0.012, 0.005], [0.012, 0.012, 0.160]]),
            ("m4a1_afg2", [1.37, 1.94, 4.70], "grip.support",
             [[-0.010, -0.025, 0.01469], [0.010, -0.015, 0.02969]]),
            ("m4a1_rvg", [None, 3.5, None], "grip.support", None),
            ("m4a1_moe_sl", [1.8, 2.4, 8.9], "grip.support",
             [[-0.012, -0.012, -0.220], [0.012, 0.012, 0]]),
        ]
        for name, inches, contact_id, opening in cases:
            with self.subTest(asset=name):
                asset = load(name)
                parts = {p["id"]: p for p in asset["parts"]}
                solids = [bounds(parts, p["id"], box)
                          for p in parts.values() for box in p["boxes"]]
                for axis, value in enumerate(inches):
                    if value is not None:
                        extent = max(b[1][axis] for b in solids) - min(b[0][axis] for b in solids)
                        self.assertAlmostEqual(extent, value * 0.0254, delta=0.001)
                if opening:
                    self.assertTrue(all(not intersects(b, opening) for b in solids))
                socket = next(s for s in asset["sockets"] if s["id"] == contact_id)
                contact = point(parts, socket["part"], socket["pose"]["translation"])
                self.assertTrue(any(all(b[0][i] <= contact[i] <= b[1][i] for i in range(3))
                                    for b in solids))
                self.assertFalse(any(all(b[0][i] < contact[i] < b[1][i] for i in range(3))
                                     for b in solids), "contact buried inside another solid")
        stock = load("m4a1_ctr")
        # A-frame has real negative space below the sleeve, rather than a filled wedge.
        opening = [[-0.010, -0.060, 0.095], [0.010, -0.035, 0.125]]
        self.assertTrue(all(not intersects([b["min"], b["max"]], opening)
                            for b in stock["parts"][0]["boxes"]))


if __name__ == "__main__":
    unittest.main()
