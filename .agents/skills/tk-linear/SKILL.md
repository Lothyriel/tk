---
name: tk-linear
description: Use for TK planning, Linear cards, task selection, progress comments or resuming a card. Investigate code and alternatives, prepare actionable cards through Linear MCP, and keep decisions and evidence in Linear. Planning does not implement.
---

# TK Linear

Use the Linear MCP and team **Tarkov-killer** (key **TAR**). Discover the available tool schemas; do not invent parameters or fall back to a second tracker. Read [AGENTS.md](../../../AGENTS.md) for project boundaries.

## Plan a proposal

1. Understand the desired player/system outcome. Inspect relevant code, tests and durable docs; identify current behavior, ownership, dependencies and constraints. Research external APIs only when needed.
2. Search related cards before creating anything. Read matching descriptions, comments and pertinent attachments; follow pagination. Reuse an existing card when it represents the same work.
3. Compare plausible approaches and explain the recommendation and tradeoffs briefly. Ask about product decisions that materially change the result; continue independent investigation while waiting. Do not silently invent acceptance criteria for unresolved choices.
4. Use [the card template](references/card-template.md). Describe observable outcomes and how to verify them; keep extensive file lists or implementation ideas in the collapsible **Agent notes** section.
5. Prefer one card for a coherent result. Split independently deliverable outcomes into separate cards; use a parent and `parentId` subtasks when they contribute to a larger shared result. Give each child its own criteria, record dependencies and retain integration acceptance on the parent. Do not create a subtask per file or command.
6. Create/update the cards autonomously. Assign **every new card, including parents/subtasks, to project TK** (Linear name `tk`, identifier `P-TAR-1`) in the TK team. Confirm the project if the reference becomes ambiguous. Fetch available issue labels and apply at least one relevant existing label (for example Bug, Feature or Improvement); avoid inventing a new taxonomy when an existing label fits.
7. Set an explicit **priority**, based on impact, dependency/unblocking value and current goals: `2` High for important blockers/high-impact work; `3` Medium for normal planned work; `4` Low for optional polish. Reserve `1` Urgent for genuine immediate incidents or explicit user direction, not merely a new request. Do not leave planned cards at `0`/None. Explain the reason briefly in the card; infer a sensible priority autonomously when evidence supports it and revisit if the user corrects it.
8. Confirm team statuses before assigning them; normally use Backlog or Todo for planning. Mark unresolved questions in the description so a draft is not mistaken for ready work. When updating an existing card, preserve deliberate metadata unless the current request calls for changing it; fill missing project/labels/priority for cards prepared by this workflow.
9. Return card links, project, labels, priority, recommendation and any decisions still needed. **Stop at planning**: do not edit implementation, create a Git branch, commit or push from a planning-only request.

If MCP is unavailable, report the connection/authentication blocker and provide a draft in chat. Do not claim it was saved or start substantial implementation without the card.

## Select work (`implementar` without an ID)

- List **all** non-archived issues in the TK team, with no assignee, cycle or project filter unless the user requests one. Include backlog, unstarted, started and any other open statuses.
- Fetch every page while `hasNextPage` is true, using the returned cursor. Exclude completed, canceled and duplicate status types; resolve team statuses if results omit their type. Do not infer openness from a title or issue number.
- Present a compact list/table with ID, title, status, priority and link; indicate project and parent/child relationships when useful. The creation default is project TK, but the selection menu still includes all active team cards unless the user explicitly narrows it. If results span many pages, still retrieve all before calling the list complete.
- Ask the user to select a card and wait. Do not pick the first, highest priority or already-started card yourself. Listing alone does not change status, branch or code.
- If there are no active cards, offer `planejar` and await the proposal; do not fabricate a selection. A directly requested small chat hotfix remains allowed through `tk-development`.

## Load a selected card

Read its full description (list previews can be truncated), all relevant comments across pages, parent/subtasks, blocking relations and pertinent attachments. Inspect referenced code and current branch/commits rather than trusting historical “running” claims.

Check that substantial work has a clear objective, scope, testable acceptance, verification and resolved blocking choices. A title or empty template is insufficient. Explain missing criteria and prepare/refine the card before implementation; ask for unresolved decisions and hand back the plan.

An explicit selected ID skips the menu, not readiness checks. For a closed, archived or unrelated-team ID, explain its state and ask what the user intends before reopening or implementing it.

Once a ready card is selected and implementation starts, `tk-development` claims it with `assignee: "me"` (the authenticated MCP user) and the team's in-progress status. Read back the result. Do not derive identity from Git/OS or claim cards during planning/listing. Claim only the selected task, not its entire family.

## Keep context useful

- The description is the **current agreement**; update it when a decision changes scope or acceptance. Comments explain what changed and why, including rejected alternatives worth remembering.
- Comment at meaningful discoveries, decisions, blockers, verified increments and handoff. Do not post a transcript of tools or repeat unchanged status.
- A delivery comment names the outcome, branch/commit, checks and result, limitations and next step. Link evidence; mark local-only artifact paths as local, not accessible uploads.
- On interruption, leave the last verified commit, remaining work and exact blocker/next action. Resume by reconciling that note against Git and current code.
- Use team-supported statuses. Mark Done only when all applicable criteria are evidenced; a push or green headless suite does not close pending visual/network acceptance. Parent completion requires its own integration criteria and relevant children.
- Keep changes targeted: update only intended fields and use patches for long descriptions. Preserve other people's comments and labels; use incremental label operations.
