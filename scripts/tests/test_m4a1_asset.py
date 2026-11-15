"""Independent dimensional/clearance oracles for the authored M4A1 asset.

These checks qualify source geometry, not its pending native artistic review.
"""
import itertools
import json
from pathlib import Path
import unittest


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
        magazine = load("m4a1_magazine_30")
        seat = self.sockets["magazine.seat"]
        walls = [bounds(self.parts, "lower", box) for box in self.parts["lower"]["boxes"]]
        # Full continuous downward sweep is an AABB for this translation. It
        # covers every intermediate position, not only selected animation frames.
        for box in magazine["parts"][0]["boxes"]:
            swept = [[box["min"][i] + seat[i] for i in range(3)],
                     [box["max"][i] + seat[i] for i in range(3)]]
            swept[0][1] -= 0.24
            for wall in walls:
                self.assertFalse(intersects(swept, wall), (swept, wall))


if __name__ == "__main__":
    unittest.main()
