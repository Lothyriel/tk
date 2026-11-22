import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import bake_item_icons as icons


class ItemIconTests(unittest.TestCase):
    def test_committed_atlas_is_current(self):
        self.assertEqual(icons.build(check=True), "hit")

    def test_cache_reuses_page_and_invalidates_on_baked_material_change(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            bakes = root / "baked"
            page = root / "page"
            bakes.mkdir()
            for _, key in icons.bindings(icons.BINDINGS.read_bytes()):
                shutil.copyfile(icons.OUTPUT / f"{key}.json", bakes / f"{key}.json")
            self.assertEqual(icons.build(page, bakes=bakes), "miss")
            self.assertEqual((page / "atlas.png").read_bytes(), (icons.PAGE / "atlas.png").read_bytes())
            timestamp = (page / "atlas.png").stat().st_mtime_ns
            self.assertEqual(icons.build(page, bakes=bakes), "hit")
            self.assertEqual((page / "atlas.png").stat().st_mtime_ns, timestamp)
            path = bakes / "item_water500.json"
            bake = json.loads(path.read_bytes())
            for material in bake["materials"]:
                if material["id"] == "pet":
                    material["color"][3] = 0.8
            path.write_bytes(icons.canonical(bake))
            with self.assertRaisesRegex(ValueError, "stale"):
                icons.build(page, bakes=bakes, check=True)
            self.assertEqual(icons.build(page, bakes=bakes), "miss")
            self.assertNotEqual((page / "atlas.png").read_bytes(), (icons.PAGE / "atlas.png").read_bytes())
