# Developer Standardization Tasks

## Execution Protocol

Use tlc-spec-driven, commits-incrementais and game-state. The user authorized this implementation and publishing the selected standards; continue on the existing branch. Four cohesive tasks run inline. After the final task commit, dispatch an independent Verifier with an isolated-scratch discrimination sensor. No CI, toolchain, edition or gameplay changes.

**Status**: In Progress

Design is inline in spec.md: vendor the existing skill, use a standard-library Python command orchestrator and preserve historical memory in a separate archive. No additional architecture document is needed.

## Test Coverage Matrix

Guidelines: AGENTS.md, existing Python standard-library scripts and the TLC execution contract.

| Code Layer | Required Test Type | Coverage Expectation | Location Pattern | Run Command |
| --- | --- | --- | --- | --- |
| Skill snapshot | structural | 18 source files, documented local delta, licensing, valid Python | .opencode/skills/tlc-spec-driven | Source comparison and Python syntax checks |
| Verification CLI | unit + CLI smoke | All modes, exact commands, failures, input boundaries, root resolution | scripts/tests/test_verify.py | python -m unittest discover -s scripts/tests -v |
| Workflow docs | structural | Local commands resolve; contribution and publication rules agree | AGENTS.md, CONTRIBUTING.md | python scripts/verify.py --mode docs --feature developer-standardization |
| Memory and scope | structural | Archive/decisions match baseline; excluded surfaces and Mermaid unchanged | .specs, docs/GAME_STATE.md | Baseline comparisons and independent review |

## Gate Check Commands

| Gate Level | When to Use | Command |
| --- | --- | --- |
| Snapshot | T1 before runner exists | Compare copied sources to installed snapshot, parse Python, run local validate_spec.py and validate_tasks.py |
| Quick | T2 CLI implementation | python -m unittest discover -s scripts/tests -v |
| Docs | T3/T4 documentation | python scripts/verify.py --mode docs --feature developer-standardization |
| Build | Final integration | python scripts/verify.py --mode full --feature developer-standardization |

## Execution Plan

### Phase 1: Portable developer workflow

```text
T1 -> T2 -> T3 -> T4
```

## Task Breakdown

### T1: Vendor the current TLC skill
**What**: Preserve the complete current source snapshot, upstream attribution/licensing and the known local implementation-reference delta.
**Where**: `.opencode/skills/tlc-spec-driven/`
**Depends on**: None
**Requirement**: DEV-01
**Done when**:
- [x] All 18 source files match the installed snapshot and local validators run; source/license documentation records the exact upstream revision and local delta.

T1 gate: all 18 copies matched the installed bytes, all five Python scripts parsed, both structural validators exited 0; spec has no warnings. The tasks validator reports one granularity warning for T3's two documentation entry points, retained as one cohesive workflow publication rather than artificial per-file commits.
**Tests**: structural source and syntax checks
**Gate**: Snapshot

### T2: Add the headless verification command
**What**: Implement the quick/full/docs orchestrator and co-located Python tests derived from DEV-03 through DEV-08.
**Where**: `scripts/verify.py` (tests in scripts/tests)
**Depends on**: T1
**Requirement**: DEV-03, DEV-04, DEV-05, DEV-06, DEV-07, DEV-08
**Done when**:
- [ ] Mode commands, filters, validation, first-failure propagation, missing executables, root resolution and shell-free subprocess execution pass unit tests and real CLI smoke checks.
**Tests**: unit + CLI smoke
**Gate**: Quick

### T3: Publish workflow and contribution rules
**What**: Make the local TLC flow and verification commands discoverable, with English Conventional Commits and explicit publication boundaries.
**Where**: `CONTRIBUTING.md` (workflow entry in AGENTS.md)
**Depends on**: T2
**Requirement**: DEV-02
**Done when**:
- [ ] Both guides resolve local validators, explain quick/full/docs and retain headless/no-default-video rules without installing hooks.
**Tests**: structural documentation checks
**Gate**: Docs

### T4: Reconcile project memory
**What**: Archive the old handoff, preserve decisions, replace the active snapshot and review the game guide without changing gameplay.
**Where**: `.specs/STATE.md` (historical archive and guide review)
**Depends on**: T3
**Requirement**: DEV-09, DEV-10
**Done when**:
- [ ] Prior handoff/decisions are preserved, current scope and historical-only permissions are explicit, excluded surfaces and Mermaid match baseline, and final integration passes before independent verification.
**Tests**: structural baseline comparisons + integrated runner
**Gate**: Build

## Diagram-Definition Cross-Check

| Task | Depends On | Diagram Shows | Status |
| --- | --- | --- | --- |
| T1 | None | None | Match |
| T2 | T1 | T1 | Match |
| T3 | T2 | T2 | Match |
| T4 | T3 | T3 | Match |

## Test Co-location Validation

T1/T3/T4 use structural/documentation checks, not game runs. T2 includes its Python tests in the same commit as the runner. The final Build gate uses the real existing Cargo commands; unit mocks certify orchestration only, not game behavior.
