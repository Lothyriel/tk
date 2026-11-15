# Developer Standardization Tasks

## Execution Protocol

Use tlc-spec-driven, commits-incrementais and game-state. The user authorized this implementation and publishing the selected standards; continue on the existing branch. Four cohesive tasks run inline. After the final task commit, dispatch an independent Verifier with an isolated-scratch discrimination sensor. No CI, toolchain, edition or gameplay changes.

**Status**: Done

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
| Delivery | T4 user-approved existing-Clippy exception | python scripts/verify.py --mode docs --feature developer-standardization, baseline comparisons and separate cargo test --workspace -j 4 |

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
- [x] Mode commands, filters, validation, first-failure propagation, missing executables, root resolution and shell-free subprocess execution pass unit tests and real CLI smoke checks.

T2 gate: 12 Python tests passed. Docs mode passed both with the selected feature and when launched from outside the repository. A real quick-mode run with server/real_rapier executed 2 existing tests, both passed; no client or recorder ran. Tests were written first and initially failed because verify.py did not exist.
**Tests**: unit + CLI smoke
**Gate**: Quick

### T3: Publish workflow and contribution rules
**What**: Publish the user-approved short root guide, mother/child skill routing and contributor guide, with English Conventional Commits and explicit publication boundaries.
**Where**: `CONTRIBUTING.md` (root guide and project skill entry points)
**Depends on**: T2
**Requirement**: DEV-02
**Done when**:
- [x] The short root guide and tk-workflow/tk-verification/tk-contribution skills route analysis, implementation, tests and authorized Git work without recursion or a second TLC layer; CONTRIBUTING.md resolves local validators and explains quick/full/docs, with no-default-video safety retained in the root guide.

T3 gate: docs mode passed 18 tooling tests and both structural validators with no warnings. The three new skill IDs loaded successfully in this session. Independent read-only routing review passed all three stored scenarios; its clarifications and limits are in routing-review.md. The root guide is below the 80-line limit; no Git hooks were installed.
**Tests**: structural documentation checks
**Gate**: Docs

### T4: Reconcile project memory
**What**: Archive the old handoff, preserve decisions, replace the active snapshot and review the game guide without changing gameplay.
**Where**: `.specs/STATE.md` (historical archive and guide review)
**Depends on**: T3
**Requirement**: DEV-09, DEV-10
**Done when**:
- [x] Prior handoff/decisions are preserved, current scope and historical-only permissions are explicit, excluded surfaces and Mermaid match baseline, and the user-approved Delivery gate passes before independent verification, while full remains explicitly blocked by the preexisting Clippy failure.
**Tests**: structural baseline comparisons + integrated runner
**Gate**: Delivery

User decision (2026-11-15): retain the full runner and Clippy `-D warnings` unchanged; document the existing anatomy API lint and continue this standardization delivery without changing Rust. The Build run remains exit 101, not PASS. Run the Delivery commands separately; do not relabel separately executed workspace tests as completion of that stopped full run.

T4 gate: docs mode passed 20 tooling tests and both structural validators (zero errors/warnings). Separately executed workspace tests passed 112 tests (27 client, 42 common, 43 server; no failures/ignored tests), with no application/video launch. Direct baseline comparisons confirmed byte-exact archived STATE, all prior decisions preserved, unchanged Mermaid and no Rust/CI/Cargo/toolchain/linker changes. Evidence logs are ignored target/verification/developer-standardization-{full,workspace-tests}.log; the first remains a failure. Independent feature verification was pending at this task's commit; it is now recorded in validation.md.

## Final Verification

The independent Verifier reviewed 83bfc4a..83c02c0 and returned scoped PASS for DEV-01..10, independently passed docs/20 tooling tests and 112 separate workspace tests, and killed all three scratch-only behavior mutations with no real-tree changes. The local completion validator passed. See validation.md for exact assertion/source evidence, platform limits and the unchanged full/Clippy blocker. Done means the approved tooling delivery is complete; it does not mean full is green or prior gameplay acceptance is closed.

## Diagram-Definition Cross-Check

| Task | Depends On | Diagram Shows | Status |
| --- | --- | --- | --- |
| T1 | None | None | Match |
| T2 | T1 | T1 | Match |
| T3 | T2 | T2 | Match |
| T4 | T3 | T3 | Match |

## Test Co-location Validation

T1/T3/T4 use structural/documentation checks, not game runs. T2 includes its Python tests in the same commit as the runner. The final Build gate uses the real existing Cargo commands; unit mocks certify orchestration only, not game behavior.
