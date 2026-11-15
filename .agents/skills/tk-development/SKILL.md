---
name: tk-development
description: Use to implement, fix or resume TK code, tooling or documentation changes. Gates substantial work on a ready Linear card, allows localized chat hotfixes, verifies coherent increments and automatically commits and pushes a task branch.
---

# TK development

Follow [AGENTS.md](../../../AGENTS.md). Keep implementation proportional to the requested result.

## Establish scope

1. For `implementar` with no ID, use `tk-linear` to list all active cards and await selection. With an ID, load its full context and check readiness through that skill.
2. A directly requested **small localized hotfix** may start from chat: known defect, bounded correction and clear verification, without a new subsystem or cross-cutting feature. Summarize its acceptance briefly; no mandatory card.
3. Substantial implementation requires a detailed Linear card. If none exists, or the card lacks usable criteria, stop before coding and use `tk-linear` to investigate and prepare it. Explain the missing agreement; return the plan and ask for needed decisions or an implementation request. A newly created empty card does not unlock work.
4. Inspect relevant code/tests and Git status, staged/unstaged diffs, branch/upstream and recent commits. Reconcile completed work before resuming. Preserve unrelated edits; if safe isolation is unclear, ask rather than reset/stash someone else's work.
5. Give a short execution plan in chat. No parallel local spec, status ledger, mandatory report or per-file task checklist. Read only the skills relevant to the work.

## Implement and deliver increments

- Use one branch per task (`feat/tar-123-short-name`, `fix/tar-123-short-name`, `chore/tar-123-short-name`; chat hotfix: `fix/short-name`). Reuse the matching task branch on resume. Do not implement on `main` or mix another task's branch; use safe isolation if necessary.
- Confirm the remote and starting base. Default publication target is `origin` and the task branch; explicit user restrictions, such as local-only, take precedence.
- When implementation of the selected ready card starts, assign it to the **authenticated Linear MCP user** with `assignee: "me"` and move it to the team's in-progress status. This applies on resume too; do not infer the user from Git author, OS login or historical assignee. Confirm the returned assignee/status; if claiming the card fails, report the blocker before editing. Listing or planning alone does not assign cards. For a selected subtask, claim that subtask; do not bulk-reassign siblings or its parent.
- Make the smallest coherent change with acceptance-derived tests where useful. Follow architecture and serialization ownership. Do not add tests that merely freeze document wording or count files.
- If investigation reveals substantially larger scope or missing acceptance, **stop the expansion**. Record the discovery and use `tk-linear` to refine/split cards before continuing. Do not implement speculative additions.
- Use `tk-verification` for the relevant gate. Fix failures introduced by the increment. For an unrelated baseline failure, record reproducible evidence and a follow-up card, keep the full result marked failed, and only deliver a scoped increment whose own checks pass and whose agreed criteria allow that limitation.
- Review the diff, then stage explicit task-owned paths/hunks with related tests/docs. Inspect the staged diff before committing; avoid unrelated staged changes. Commit automatically with an English Conventional Commit, e.g. `fix(sync): reset motion history after stale snapshots`. Include the reason and card reference in the body when helpful.
- Push each verified coherent commit promptly with `git push -u origin BRANCH` initially, then to its confirmed upstream. Preserve hooks and identity; do not force-push. A rejected push is a blocker to investigate, not permission to overwrite remote history.
- Post a concise Linear delivery comment linking commit/branch, relevant evidence and remaining work. For cardless hotfixes, report this in chat; create a card only if the scope warrants one.

## Finish

Run the final gate selected by `tk-verification` and audit the acceptance against actual evidence. Mark the card Done only when its criteria are met; leave pending human/visual/network acceptance explicit. Report commits, push status, verification and blockers.

Update `docs/GAME_STATE.md` only for relevant launch, control, gameplay or player-visible limitation changes; keep its Mermaid stable unless the game flow changes. Durable technical reasons belong near the code or in a concise technical doc; operational history belongs in Linear.

Do not merge, create a PR, release or deploy unless separately requested. When integration is requested, rebase onto the target then fast-forward it, without a merge commit.
