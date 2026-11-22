import struct
import tempfile
import unittest
from pathlib import Path

from scripts.playtest import grid_review_evidence


class GridReviewEvidenceTests(unittest.TestCase):
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
            self.assertEqual(result["not_observed"], ["drag-invalid", "pending", "hover", "context"])

            (output / "grid-drag-valid.png").write_bytes(header(1280, 800))
            result = grid_review_evidence(output, log.replace("inside", "CLIPPED", 1), (1280, 720), 1.25)
            self.assertEqual(len(result["findings"]), 2)
            self.assertIn("idle:", result["findings"][0])
            self.assertIn("layout=CLIPPED", result["findings"][0])
            self.assertIn("PNG=(1280, 800)", result["findings"][1])
            self.assertIn("Required state not observed: idle",
                          grid_review_evidence(output, "", (1280, 720), 1.25)["findings"])
