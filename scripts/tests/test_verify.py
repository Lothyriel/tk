"""Verification CLI contracts, without executing Cargo or the game."""

import contextlib
import io
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import verify


class VerifyTests(unittest.TestCase):
    def commands(self, *arguments):
        args = verify.parser().parse_args(arguments)
        return verify.commands_for(args)

    def cargo(self, commands):
        return [command for command in commands if command[0] == "cargo"]

    def test_default_runs_only_common_and_server_tests(self):
        commands = self.commands()
        self.assertEqual(self.cargo(commands), [
            ["cargo", "test", "--locked", "-p", "common", "-p", "server", "-j", "4"],
        ])
        self.assertEqual(commands[:2], [
            ["git", "diff", "--check"], ["git", "diff", "--cached", "--check"],
        ])
        self.assertEqual(commands[2], [sys.executable, "-m", "unittest", "discover", "-s", "scripts/tests", "-v"])

    def test_full_runs_the_exact_workspace_gates_once(self):
        self.assertEqual(self.cargo(self.commands("--mode", "full")), [
            ["cargo", "fmt", "--all", "--", "--check"],
            ["cargo", "check", "--locked", "--workspace", "-j", "4"],
            ["cargo", "clippy", "--locked", "--workspace", "--all-targets", "-j", "4", "--", "-D", "warnings"],
            ["cargo", "test", "--locked", "--workspace", "-j", "4"],
        ])

    def test_docs_runs_tooling_tests_but_no_cargo(self):
        commands = self.commands("--mode", "docs")
        self.assertEqual(self.cargo(commands), [])
        self.assertIn([sys.executable, "-m", "unittest", "discover", "-s", "scripts/tests", "-v"], commands)

    def test_no_mode_launches_a_game_harness_or_video(self):
        for mode in ["quick", "full", "docs"]:
            commands = self.commands("--mode", mode)
            self.assertEqual([command[0] for command in commands],
                             ["git", "git", sys.executable] + ["cargo"] * len(self.cargo(commands)))
            for command in commands:
                self.assertNotIn("run", command)
                self.assertNotIn("--video", command)
                self.assertNotIn("client", command)

    def test_package_filter_and_jobs_are_forwarded_as_arguments(self):
        self.assertEqual(self.cargo(self.commands("--package", "server", "--filter", "real_rapier", "--jobs", "2")), [
            ["cargo", "test", "--locked", "-p", "server", "-j", "2", "real_rapier"],
        ])
        self.assertEqual(self.cargo(self.commands("--package", "common")), [
            ["cargo", "test", "--locked", "-p", "common", "-j", "4"],
        ])

    def test_client_selection_runs_unit_tests_without_launching_the_game(self):
        commands = self.commands("--package", "client", "--filter", "local_motion")
        self.assertEqual(self.cargo(commands), [
            ["cargo", "test", "--locked", "-p", "client", "-j", "4", "local_motion"],
        ])
        self.assertNotIn("run", [arg for command in commands for arg in command])

    def test_invalid_arguments_exit_two_before_any_command(self):
        invalid = [
            ["--filter", "ragdoll"], ["--package", "unknown"],
            ["--package", "server", "--filter", "   "],
            ["--package", "server", "--filter=--workspace"],
            ["--mode", "full", "--package", "server"],
            ["--mode", "docs", "--package", "common"],
            ["--mode", "full", "--filter", "ragdoll"],
            ["--jobs", "0"], ["--jobs", "-1"], ["--jobs", "abc"],
            ["--mode", "unknown"],
        ]
        for arguments in invalid:
            with self.subTest(arguments=arguments), patch("verify.subprocess.run") as run:
                with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as error:
                    verify.main(arguments)
                self.assertEqual(error.exception.code, 2)
                run.assert_not_called()

    def test_failure_stops_the_pipeline_and_preserves_the_exit_code(self):
        with patch("verify.subprocess.run", side_effect=[subprocess.CompletedProcess([], 0), subprocess.CompletedProcess([], 9)]) as run:
            with contextlib.redirect_stdout(io.StringIO()) as output, contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(verify.main(["--mode", "full"]), 9)
            self.assertEqual(run.call_count, 2)
            self.assertNotIn("PASS", output.getvalue())

    def test_missing_executable_exits_127_with_a_diagnostic(self):
        with patch("verify.subprocess.run", side_effect=FileNotFoundError("git")) as run:
            with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()) as errors:
                self.assertEqual(verify.main([]), 127)
            self.assertEqual(run.call_count, 1)
            self.assertIn("git", errors.getvalue())
            self.assertIn("not found", errors.getvalue())

    def test_commands_use_repository_cwd_without_a_shell(self):
        # Caller cwd cannot influence command paths; no process-wide chdir.
        with tempfile.TemporaryDirectory() as directory, patch("verify.subprocess.run", return_value=subprocess.CompletedProcess([], 0)) as run:
            with patch("pathlib.Path.cwd", return_value=Path(directory)), contextlib.redirect_stdout(io.StringIO()) as output:
                self.assertEqual(verify.main(["--mode", "docs"]), 0)
            for call in run.call_args_list:
                self.assertIsInstance(call.args[0], list)
                self.assertEqual(call.kwargs, {"cwd": verify.ROOT, "check": False})
            self.assertIn("PASS", output.getvalue())


if __name__ == "__main__":
    unittest.main()
