# TLC spec-driven source and updates

- Project: [Tech Leads Club agent-skills](https://github.com/tech-leads-club/agent-skills).
- Author: Felipe Rodrigues ([felipfr](https://github.com/felipfr)).
- Skill version: **3.3.0**.
- Upstream revision: [`691ed56173c07c7b0f4c83d9408f337e5aa19cac`](https://github.com/tech-leads-club/agent-skills/commit/691ed56173c07c7b0f4c83d9408f337e5aa19cac).
- Upstream directory: `packages/skills-catalog/skills/(development)/tlc-spec-driven/`.
- Snapshot recorded: 2026-11-14, from the installation already used for TK.

The snapshot originated with 18 source files (SKILL.md, 12 references and 5 Python scripts), copied from the existing installation. The local TK version removes the optional `scripts/check_commit.py` validator and its commit-hook instructions. The installed `SKILL.md` and `references/implement.md` also have local changes retained in this snapshot:

- Update task/spec completion before the atomic commit and include status in that commit.
- Explicitly require authorization for remote or destructive operations.
- Renumber the feature-level verification step and its cross-references accordingly.
- Remove the optional commit-message validator and hook guidance at the user's request.

The other source files remain unchanged from the adopted snapshot. Git history records the exact source and local differences; this is not a claim that the entire directory is unmodified upstream. Personal installer metadata, download timestamps and Python caches are excluded.

## Licensing and attribution

The upstream SKILL.md declares **CC-BY-4.0**. Attribution is to Felipe Rodrigues / Tech Leads Club, with source and modification details above. License: <https://creativecommons.org/licenses/by/4.0/>; legal text: <https://creativecommons.org/licenses/by/4.0/legalcode>. Upstream software source is **MIT**, subject to the dual-licensing note reproduced in LICENSE. These notices do not change the license of TK itself.

## Updating

Updates are deliberate, reviewed repository changes, never downloads during verification. Select an explicit upstream revision, check source/license changes, compare all source files and reconcile the retained local changes rather than dropping them silently. Record the new revision, version and differences here, run the validators and tooling tests, then commit the snapshot as a coherent unit. Do not include `.skill-meta.json`, caches or personal configuration. The project-local skill is stored under `.agents/skills`; no personal path or model configuration is required.
