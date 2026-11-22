import struct
import tempfile
import unittest
from pathlib import Path

from scripts.playtest import grid_review_evidence


class GridReviewEvidenceTests(unittest.TestCase):
    def test_split_quantity_reports_one_observed_frame_and_checks_bounds(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            header = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR" + struct.pack(">II", 1280, 720)
            for state in ("idle", "drag-valid", "split-quantity"):
                (output / f"grid-{state}.png").write_bytes(header)
            def line(state):
                return f"GRID REVIEW {state} 1280x720 ui_scale=1 revision=5 items=2 layout=inside"
            base = line("idle") + "\n" + line("drag-valid")
            split = line("split-quantity") + " split_item=1 source_quantity=3 quantity=2 min=1 max=2"
            def review(log):
                return grid_review_evidence(output, log, (1280, 720), 1)
            self.assertIn("split-quantity", review(base)["not_observed"])
            result = review(base + "\n" + split)
            self.assertEqual(result["findings"], [])
            self.assertEqual(result["observed"]["split-quantity"]["split"],
                             {"item": 1, "source_quantity": 3, "quantity": 2, "min": 1, "max": 2})
            self.assertTrue(review(base + "\n" + split.replace("max=2", "max=3"))["findings"])
            self.assertTrue(review(base + "\n" + line("split-quantity"))["findings"])
            (output / "grid-split-quantity.png").unlink()
            self.assertIn("PNG=None", review(base + "\n" + split)["findings"][0])

    def test_inspection_requires_same_target_and_all_ordered_frames(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            header = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR" + struct.pack(">II", 1280, 720)
            def grid(state):
                (output / f"grid-{state}.png").write_bytes(header)
                return f"GRID REVIEW {state} 1280x720 ui_scale=1 revision=4 items=2 layout=inside\n"

            log = grid("idle") + grid("hover") + grid("context")
            log += "GRID POINTER hover/context observed for item=1 revision=4\n"
            for stage in ("open", "orbit", "zoom", "reset"):
                (output / f"inspect-{stage}.png").write_bytes(header)
                log += f"GRID INSPECT {stage} item=1 revision=4 asset=bandage\n"
            log += "GRID INSPECT closed via Escape; resuming keyboard route\n" + grid("drag-valid")

            def review(text):
                return grid_review_evidence(output, text, (1280, 720), 1, True, True)

            self.assertEqual(review(log)["findings"], [])
            self.assertTrue(review(log.replace("zoom item=1", "zoom item=2"))["findings"])
            self.assertTrue(review(log.replace("GRID INSPECT reset", "GRID INSPECT orbit"))["findings"])
            (output / "inspect-reset.png").unlink()
            self.assertIn("inspect-reset.png: expected (1280, 720), got None", review(log)["findings"])

    def test_pointer_checkpoint_requires_observed_frames_and_escape_before_drag(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            def line(state):
                return f"GRID REVIEW {state} 1280x720 ui_scale=1.25 revision=4 items=2 layout=inside\n"

            for state in ("idle", "hover", "context", "drag-valid"):
                (output / f"grid-{state}.png").write_bytes(
                    b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR" + struct.pack(">II", 1280, 720)
                )
            escape = "GRID POINTER context closed via Escape; resuming keyboard route\n"
            keyboard_only = line("idle") + line("drag-valid")
            self.assertEqual(grid_review_evidence(output, keyboard_only, (1280, 720), 1.25)["findings"], [])
            missing = grid_review_evidence(output, keyboard_only, (1280, 720), 1.25, pointer_checkpoint=True)
            self.assertIn("Required state not observed: hover", missing["findings"])
            self.assertIn("Required state not observed: context", missing["findings"])
            log = line("idle") + line("hover") + line("context") + escape + line("drag-valid")
            self.assertEqual(grid_review_evidence(output, log, (1280, 720), 1.25, True)["findings"], [])
            late = log.replace(escape, "") + escape
            self.assertIn("Missing ordered context → Escape → keyboard drag evidence",
                          grid_review_evidence(output, late, (1280, 720), 1.25, True)["findings"])
            (output / "grid-context.png").unlink()
            self.assertIn("PNG=None",
                          grid_review_evidence(output, log, (1280, 720), 1.25, True)["findings"][0])

    def test_requires_observation_and_matching_framebuffer(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            def header(width, height):
                return b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR" + struct.pack(">II", width, height)

            for state in ("idle", "drag-valid", "pending"):
                (output / f"grid-{state}.png").write_bytes(header(1280, 720))
            log = "\n".join(
                f"GRID REVIEW {state} 1280x720 ui_scale=1.25 revision=4 items=2 layout=inside"
                for state in ("idle", "drag-valid")
            )
            result = grid_review_evidence(output, log, (1280, 720), 1.25)
            self.assertEqual(result["findings"], [])
            self.assertEqual(list(result["observed"]), ["idle", "drag-valid"])
            self.assertEqual(result["not_observed"], ["drag-invalid", "pending", "hover", "context", "split-quantity"])

            (output / "grid-drag-valid.png").write_bytes(header(1280, 800))
            result = grid_review_evidence(output, log.replace("inside", "CLIPPED", 1), (1280, 720), 1.25)
            self.assertEqual(len(result["findings"]), 2)
            self.assertIn("idle:", result["findings"][0])
            self.assertIn("layout=CLIPPED", result["findings"][0])
            self.assertIn("PNG=(1280, 800)", result["findings"][1])
            self.assertIn("Required state not observed: idle",
                          grid_review_evidence(output, "", (1280, 720), 1.25)["findings"])
