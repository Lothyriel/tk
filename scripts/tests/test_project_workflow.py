"""Check portable skill/command/config wiring, not process prose or inventories."""

import json
from pathlib import Path
import re
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[2]
SKILLS = ROOT / ".agents/skills"


class ProjectWorkflowTests(unittest.TestCase):
    def test_discoverable_skills_have_unique_matching_ids_and_descriptions(self):
        entries = list(SKILLS.glob("*/SKILL.md"))
        self.assertTrue(entries)
        names = set()
        for path in entries:
            with self.subTest(path=path):
                source = path.read_text(encoding="utf-8")
                self.assertTrue(source.startswith("---\n"))
                metadata = source.split("---", 2)[1]
                name = re.search(r"^name: (.+)$", metadata, re.M).group(1)
                self.assertRegex(name, r"^[a-z0-9]+(-[a-z0-9]+)*$")
                self.assertEqual(name, path.parent.name)
                self.assertNotIn(name, names)
                names.add(name)
                self.assertRegex(metadata, r"\ndescription: \S.+")

    def test_command_adapters_resolve_shared_entries_and_forward_arguments(self):
        for name in ["planejar", "implementar"]:
            with self.subTest(name=name):
                source = (ROOT / ".opencode/commands" / f"{name}.md").read_text(encoding="utf-8")
                self.assertIn("$ARGUMENTS", source)
                targets = re.findall(r"`(\.agents/skills/[^`]+/SKILL\.md)`", source)
                self.assertTrue(targets)
                for target in targets:
                    self.assertTrue((ROOT / target).is_file())
                # Planning creates cards through MCP; a read-only agent would block it.
                self.assertNotRegex(source, r"(?m)^agent: plan\s*$")

    def test_shared_guides_have_resolving_local_links(self):
        guides = [ROOT / "AGENTS.md", ROOT / "docs/DEVELOPMENT.md", *SKILLS.rglob("*.md")]
        for guide in guides:
            for target in re.findall(r"\]\(([^)]+)\)", guide.read_text(encoding="utf-8")):
                if "://" not in target and not target.startswith("#"):
                    with self.subTest(guide=str(guide), target=target):
                        self.assertTrue((guide.parent / target.split("#")[0]).resolve().is_file())

    def test_harnesses_share_remote_linear_endpoint(self):
        opencode = json.loads((ROOT / "opencode.json").read_text(encoding="utf-8"))
        codex = tomllib.loads((ROOT / ".codex/config.toml").read_text(encoding="utf-8"))
        remote = opencode["mcp"]["servers"]["linear"]
        self.assertEqual(remote["type"], "remote")
        self.assertEqual(remote["url"], "https://mcp.linear.app/mcp")
        self.assertEqual(codex["mcp_servers"]["linear"]["url"], remote["url"])


if __name__ == "__main__":
    unittest.main()
