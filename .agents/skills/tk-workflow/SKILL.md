---
name: tk-workflow
description: Routes TK project work to the smallest relevant local skills. Use for project analysis, planning, implementation, verification, documentation, commits and publishing requests, including /goals. Preserves analysis-only scope and does not itself authorize code changes or publication.
---

# TK workflow

This is a router, not another development methodology. Keep the user's objective and authorization; load only the procedures needed for it. Do not load all skills or call this router recursively.

## Choose the route

| Request | Load / action |
| --- | --- |
| Explanation or analysis only | Inspect only relevant code/docs and answer; do not implement, test, commit or publish unless separately requested |
| Feature planning or implementation | Activate `tlc-spec-driven`; it owns requirements, sizing, atomic tasks and independent verification |
| Run tests, fix physics, or choose a validation gate | Activate `tk-verification`; it owns headless fixtures, command modes and visual/network exceptions |
| Create commits or publish | Activate `tk-contribution` only when those actions are authorized |
| Change the project or prepare a game guide | Activate `game-state`, unless the user opted out; review the guide and Mermaid after the change |

Routes can compose: for a physics bugfix use TLC + verification, then contribution if commits are authorized, and game-state for the documentation review. For a documentation-only edit, use docs verification, contribution only if requested, and game-state unless opted out. Do not turn a simple question into a feature pipeline.

## Before acting

Read the root AGENTS.md and only the relevant portion of CONTRIBUTING.md. For resumed work, compare `.specs/STATE.md` with the real branch, status, commits and task evidence; propose the reconciled next step before edits. Preserve unexplained work and report blockers instead of discarding it. Archived handoffs and old permissions do not authorize current actions.

Activate child skills by their exact IDs; supporting files and scripts resolve relative to their loaded skill directory. The project-local TLC snapshot is authoritative for this repository; do not download updates or depend on a personal installation. If a required skill cannot load, report the blocker. Do not substitute a different workflow silently.

## Keep ownership clear

TLC owns execution and its independent final Verifier; this router does not add another worker/verifier layer. `tk-verification` selects the cheapest valid evidence, but does not waive required final acceptance. `tk-contribution` owns Git safety and contribution rules, not authority to publish. `game-state` owns the game guide, not permission to launch the game.

Apply TK's verification tiers to TLC's generic gate templates: documentation-only work uses docs checks rather than a Rust rebuild. Run artifact validators only for artifacts the selected scope actually creates (spec when formal, tasks when present); small inline changes do not require empty files. This adapts gate selection, not TLC's test integrity or independent final verification for an implemented TLC feature.

Report the result, checks actually run and remaining limitations. A tooling PASS is not artistic/network acceptance. Respect the root headless-first/no-default-video rules throughout.
