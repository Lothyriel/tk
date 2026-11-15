---
name: tk-contribution
description: Applies TK contribution, atomic English Conventional Commit and authorized publishing rules. Use when the user authorizes commits or requests push/publication, or when an approved TLC implementation includes local commits. Does not infer Git authority from analysis, tests, old decisions or historical handoffs.
---

# TK contribution

Use [AGENTS.md](../../../AGENTS.md) as the source for commit conventions and publication boundaries. Do not import conventions from unrelated projects.

1. Confirm the current request authorizes the action and destination. Planning/analysis alone does not authorize commits; local implementation/commit approval does not authorize push, PR, merge, release or deploy. Archived decisions are not standing permissions. Ask when publication scope is unclear.
2. Inspect branch, status, staged/unstaged diffs, new files and recent commits. Preserve unrelated user work; do not silently include or unstage preexisting staged content. Isolate safely or stop if ownership cannot be separated.
3. Follow TLC's atomic-task lifecycle when active: implement with co-located tests, pass the relevant `tk-verification` gate, update task/spec status before the commit and include those status updates in the same commit. Otherwise make the smallest useful coherent commits, without artificial per-file fragmentation.
4. Use clear English Conventional Commit messages. Preserve normal Git hooks and identity; never bypass hooks or change signing/configuration.
5. Stage explicit paths/hunks and review `git diff --cached` and its whitespace check. Exclude secrets, personal configuration, caches, recordings and `target/`. Do not use broad add/reset/checkout operations to sweep or discard work. For a verbatim third-party snapshot, inspect/document any upstream whitespace exception rather than silently rewriting its source.
6. Commit each verified unit and confirm the SHA/status. Push only when authorized, to the requested branch, without force-push. Do not add a PR, merge, release or deployment to a push request.

Report actual commits, checks, publishing outcome and remaining work. This skill supplies a procedure, never permission.
