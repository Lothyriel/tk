"""Preserve fail-fast and frozen-source guards without launching a native process."""

import contextlib
import io
import itertools
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import validate_humanoids


class HumanoidAcceptanceTests(unittest.TestCase):
    def run_suite(self, output, result):
        with patch("sys.argv", ["validate_humanoids.py", "--output", str(output)]), \
             patch("validate_humanoids.subprocess.run", side_effect=result) as run, \
             contextlib.redirect_stdout(io.StringIO()):
            code = validate_humanoids.main()
        return code, run

    def test_failed_central_gate_prevents_native_launch_and_retains_log(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "evidence"
            code, run = self.run_suite(output, [subprocess.CompletedProcess([], 101, "lint failure")])
            self.assertEqual(code, 101)
            self.assertEqual(run.call_count, 1)
            self.assertEqual(run.call_args.args[0],
                             [sys.executable, "scripts/verify.py", "--mode", "full"])
            self.assertEqual((output / "verify.log").read_text(), "lint failure")
            self.assertEqual(json.loads((output / "gates.json").read_text())[0]["exit"], 101)

    def test_success_keeps_measured_fixtures_and_reuses_binaries_for_dual(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "evidence"
            code, run = self.run_suite(output, lambda *a, **kw: subprocess.CompletedProcess([], 0, "fixture"))
            self.assertEqual(code, 0)
            commands = [call.args[0] for call in run.call_args_list]
            for name in ["full_physical_load", "persistent_anatomy"]:
                command = next(c for c in commands if name in c)
                self.assertIn("--locked", command)
                self.assertIn("--nocapture", command)
            native = [c for c in commands if "scripts/playtest.py" in c]
            self.assertEqual(len(native), 2)
            self.assertEqual(native[0][0], sys.executable)
            self.assertIn("--no-build", native[1])
            self.assertIn(str(output / "normal"), " ".join(native[1]))
            self.assertTrue((output / "source-fingerprint.json").is_file())

    def test_source_change_during_suite_cannot_be_reported_as_pass(self):
        with tempfile.TemporaryDirectory() as directory:
            # Mutate only mocked fingerprint bytes, never repository source.
            with patch("pathlib.Path.read_bytes", side_effect=itertools.chain([b"before"], itertools.repeat(b"after"))):
                code, _ = self.run_suite(Path(directory) / "evidence",
                                        lambda *a, **kw: subprocess.CompletedProcess([], 0, "fixture"))
            self.assertEqual(code, 1)


if __name__ == "__main__":
    unittest.main()
