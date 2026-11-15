"""DEV-01/02: portable skill inventory, routing boundaries and guide links."""

import ast
import json
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
SKILLS = ROOT / ".opencode/skills"


class ProjectWorkflowTests(unittest.TestCase):
    def test_skill_entry_points_have_matching_ids_and_descriptions(self):
        for name in ["tk-workflow", "tk-verification", "tk-contribution", "game-state", "tlc-spec-driven"]:
            with self.subTest(skill=name):
                source = (SKILLS / name / "SKILL.md").read_text(encoding="utf-8")
                self.assertTrue(source.startswith("---\n"))
                frontmatter = source.split("---", 2)[1]
                self.assertIn(f"name: {name}\n", frontmatter)
                self.assertRegex(frontmatter, r"\ndescription: \S.+")

    def test_vendor_snapshot_has_all_sources_and_licensing(self):
        vendor = SKILLS / "tlc-spec-driven"
        references = list((vendor / "references").glob("*.md"))
        scripts = list((vendor / "scripts").glob("*.py"))
        self.assertEqual(len(references), 12)
        self.assertEqual(len(scripts), 5)
        for script in scripts:
            ast.parse(script.read_text(encoding="utf-8"), filename=str(script))
        origin = (vendor / "UPSTREAM.md").read_text(encoding="utf-8")
        self.assertIn("691ed56173c07c7b0f4c83d9408f337e5aa19cac", origin)
        self.assertIn("CC-BY-4.0", origin)
        self.assertIn("references/implement.md", origin)
        self.assertIn("Copyright (c) 2026 Tech Leads Club", (vendor / "LICENSE").read_text(encoding="utf-8"))
        self.assertFalse((vendor / ".skill-meta.json").exists())

    def test_short_root_guide_keeps_nonoptional_guards(self):
        guide = (ROOT / "AGENTS.md").read_text(encoding="utf-8")
        self.assertLessEqual(len(guide.splitlines()), 80)
        for invariant in ["Activate `tk-workflow`", "headless-first", "Video capture is opt-in",
                          "Preserve unrelated local/staged work", "does not authorize push",
                          "common::data::{encode, decode}", "server headless/minimal", "game-state"]:
            self.assertIn(invariant, guide)

    def test_router_names_only_local_procedures_and_preserves_analysis_scope(self):
        router = (SKILLS / "tk-workflow/SKILL.md").read_text(encoding="utf-8")
        for child in ["tlc-spec-driven", "tk-verification", "tk-contribution", "game-state"]:
            self.assertIn(f"`{child}`", router)
        self.assertIn("do not implement, test, commit or publish", router)
        self.assertIn("Do not load all skills or call this router recursively", router)
        self.assertIn("not another development methodology", router)
        for name in ["tk-workflow", "tk-verification", "tk-contribution"]:
            text = (SKILLS / name / "SKILL.md").read_text(encoding="utf-8")
            self.assertNotIn("inlog-analisador", text)
            self.assertNotIn("C:/Users/", text)

    def test_guides_resolve_relative_local_links(self):
        guides = [ROOT / "AGENTS.md", ROOT / "CONTRIBUTING.md", SKILLS / "tk-contribution/SKILL.md"]
        for guide in guides:
            for target in re.findall(r"\]\(([^)]+)\)", guide.read_text(encoding="utf-8")):
                if "://" not in target and not target.startswith("#"):
                    with self.subTest(guide=str(guide), target=target):
                        self.assertTrue((guide.parent / target.split("#")[0]).resolve().is_file())

    def test_routing_examples_cover_physics_analysis_and_historical_permissions(self):
        examples = json.loads((SKILLS / "tk-workflow/evals/evals.json").read_text(encoding="utf-8"))
        self.assertEqual([example["id"] for example in examples["evals"]], [1, 2, 3])
        for example in examples["evals"]:
            self.assertTrue(example["prompt"])
            self.assertTrue(example["expected_output"])

    def test_prior_decisions_are_preserved_and_historical_authority_is_scoped(self):
        # DEV-09: compare historical entries, allowing future decisions to append.
        archive = (ROOT / ".specs/history/animation-ragdoll-state-2026-11-14.md").read_text(encoding="utf-8")
        current = (ROOT / ".specs/STATE.md").read_text(encoding="utf-8")
        archived_decisions = re.findall(r"^- AD-\d+:.*$", archive, re.M)
        self.assertEqual(len(archived_decisions), 13)
        current_decisions = re.findall(r"^- AD-\d+:.*$", current.split("## Handoff")[0], re.M)
        self.assertEqual(current_decisions[:13], archived_decisions)
        self.assertIn("do not grant standing authority", current)
        self.assertIn("history/animation-ragdoll-state-2026-11-14.md", current)

    def test_old_pipeline_instructions_exist_only_in_the_history(self):
        archive = (ROOT / ".specs/history/animation-ragdoll-state-2026-11-14.md").read_text(encoding="utf-8")
        active = (ROOT / ".specs/STATE.md").read_text(encoding="utf-8").split("## Handoff", 1)[1]
        self.assertIn("Active exclusive pipeline:", archive)
        self.assertNotIn("Active exclusive pipeline:", active)
        for old_runtime_id in re.findall(r"\b(?:sh_|ses_)[A-Za-z0-9]+", archive):
            self.assertNotIn(old_runtime_id, active)


if __name__ == "__main__":
    unittest.main()
