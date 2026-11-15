"""Portable local checks. Defaults to headless common/server tests, never game runs."""

import argparse
from pathlib import Path
import re
import shlex
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def parser():
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--mode", choices=["quick", "full", "docs"], default="quick",
                        help="quick: common/server tests; full: workspace lint/check/tests; docs: no Cargo")
    result.add_argument("--package", choices=["common", "server"], help="quick mode only")
    result.add_argument("--filter", help="nonempty Cargo test name filter; requires --package")
    result.add_argument("--jobs", type=int, default=4, help="Cargo build jobs (default: 4)")
    result.add_argument("--feature", help="validate this .specs/features/<name> spec and optional tasks")
    return result


def commands_for(args, root):
    if args.jobs < 1:
        raise ValueError("--jobs must be positive")
    if args.mode != "quick" and (args.package or args.filter is not None):
        raise ValueError("--package and --filter are only supported in quick mode")
    if args.filter is not None and (not args.package or not args.filter.strip()):
        raise ValueError("--filter requires --package and a nonempty test name")
    if args.filter is not None and args.filter.startswith("-"):
        raise ValueError("--filter must be a test name, not a Cargo option")

    commands = [
        ["git", "diff", "--check"],
        ["git", "diff", "--cached", "--check"],
        [sys.executable, "-m", "unittest", "discover", "-s", "scripts/tests", "-v"],
    ]
    if args.feature is not None:
        if not re.fullmatch(r"[a-z0-9][a-z0-9_-]*", args.feature):
            raise ValueError("--feature must be a simple lowercase feature name")
        feature = (root / ".specs/features" / args.feature).resolve()
        if not feature.is_relative_to((root / ".specs/features").resolve()) or not (feature / "spec.md").is_file():
            raise ValueError(f"feature spec not found inside .specs/features: {args.feature}")
        validators = root / ".opencode/skills/tlc-spec-driven/scripts"
        for artifact in ["spec", "tasks"]:
            if (feature / f"{artifact}.md").is_file():
                commands.append([sys.executable, str(validators / f"validate_{artifact}.py"),
                                 args.feature, "--root", str(root)])

    jobs = ["-j", str(args.jobs)]
    if args.mode == "quick":
        packages = ["-p", args.package] if args.package else ["-p", "common", "-p", "server"]
        commands.append(["cargo", "test", *packages, *jobs, *([args.filter] if args.filter is not None else [])])
    elif args.mode == "full":
        commands.extend([
            ["cargo", "fmt", "--all", "--", "--check"],
            ["cargo", "check", "--workspace", *jobs],
            ["cargo", "clippy", "--workspace", "--all-targets", *jobs, "--", "-D", "warnings"],
            ["cargo", "test", "--workspace", *jobs],
        ])
    return commands


def main(argv=None):
    arguments = parser()
    args = arguments.parse_args(argv)
    try:
        commands = commands_for(args, ROOT)
    except ValueError as error:
        arguments.error(str(error))

    for command in commands:
        print(f"RUN {shlex.join(command)}", flush=True)
        try:
            result = subprocess.run(command, cwd=ROOT, check=False)
        except FileNotFoundError:
            print(f"FAIL: executable not found: {command[0]}", file=sys.stderr)
            return 127
        if result.returncode:
            print(f"FAIL: exit {result.returncode}: {shlex.join(command)}", file=sys.stderr)
            return result.returncode
    print(f"PASS: {args.mode} checks (no rendered/network acceptance was run)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
