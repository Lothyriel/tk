# LESSONS - auto-maintained by scripts/lessons.py

> Machine-owned. Do NOT hand-edit. Changes are overwritten on the next `lessons.py` write.
> Canonical state lives in `.specs/lessons.json`. Edit lessons only via the script.
> promote_threshold=2 distinct features · window_days=45 · quarantine_threshold=2

## Confirmed (load these at Specify/Design)

Corroborated across multiple features. Safe to apply as guidance.

_none_

## Candidates (under observation - do NOT load as guidance yet)

Seen once or not yet corroborated. Tracked, not trusted.

### L-001 - Assert effect positions against independent posed-world coordinates instead of using the production position helper as the expected value.
- signal: `surviving_mutant` · recurrence: 1 feature(s) · scope: `client/render` · harmful: 0
- features: blood-effects
- evidence: blood-effects/validation.md source-shift sensor at client/src/render/blood.rs:93 (client/render)
- last seen: 2026-11-15T06:05:13Z

### L-002 - After scratch mutations share Cargo target outputs, force recompilation from unchanged real source before accepting subsequent working-tree test evidence.
- signal: `gate_fail` · recurrence: 1 feature(s) · scope: `verification` · harmful: 0
- features: blood-effects
- evidence: blood-effects/validation.md Shared Cargo artifact follow-up (verification)
- last seen: 2026-11-15T06:05:56Z

## Quarantined (failed when applied - ignore)

A confirmed lesson that recurred alongside failure. Kept for the maintainer to review.

_none_
