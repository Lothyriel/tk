---
name: implementar
description: Explicit TK implementation entry point. Use when the user invokes implementar to select an active Linear card or implement a supplied card ID.
---

# Implementar

Use `tk-linear` for task selection/context and `tk-development` for execution.
With no card ID, list every active TK card across all pages and **await the user's selection** before changing status, branch or code.
With a supplied ID, load its context and check readiness, then implement through `tk-development`.

If skill activation by ID is unavailable, read [tk-linear](../tk-linear/SKILL.md) and [tk-development](../tk-development/SKILL.md).
