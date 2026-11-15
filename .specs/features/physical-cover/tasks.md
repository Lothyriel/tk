# Physical cover tasks

Use `tlc-spec-driven`, `tk-verification`, `tk-contribution` and `game-state` under AGENTS.md. Current user request authorizes implementation, local commits and final push; shutdown follows successful delivery. No worker delegation is needed for three tasks. Independent final verification remains required.

## Test Coverage Matrix

Derived from AGENTS.md and existing common map/data, server engine fixture and client sync/render tests.

| Code Layer | Required Test Type | Coverage Expectation | Location Pattern | Run Command |
| --- | --- | --- | --- | --- |
| Shared geometry/protocol | unit | Exact bore dimensions, subtraction volume, limits and wire roundtrip/budget | common/src/map; common/src/data.rs | python scripts/verify.py --package common |
| Authoritative destruction | integration | All material/support/reset paths through actual Bevy/Rapier, same-tick shots and bounded lifetime | server/src/tick/tests | python scripts/verify.py --package server --filter destruction |
| Client geometry/sync | unit/integration | Exact cut geometry, sequence/loss/late-arrival state and rubble identity | client/src/render; client/src/sync | cargo test -p client destruction |

## Gate Check Commands

| Gate Level | When to Use | Command |
| --- | --- | --- |
| Quick | Focused iteration | python scripts/verify.py --package server --filter destruction |
| Full | Integration unit | cargo test --workspace -j 4 -- --test-threads=1 |
| Build | Final delivery | python scripts/verify.py --mode full --feature physical-cover |

The known preexisting Clippy blocker must be reported if still present, never hidden by a weakened lint/test command. Supplemental tests complete coverage when the entry point stops before tests.

## Execution Plan

### Phase 1: Physical cover

```text
T1 -> T2 -> T3
```

## Task Breakdown

### T1: Bind impact marks to supporting cover

**Where**: server/src/tick/mod.rs
**Integration**: projectile impact call sites and raid reset; co-located engine regressions; guide/spec progress.
**Depends on**: None
**Tests**: integration
**Gate**: Quick
**Done when**:
- [x] DC-01 is supported by exact same-cell removal, other-surface preservation and reset assertions.
- [x] Existing projectile/ammunition tests remain intact.
**Commit**: fix(destruction): remove impact marks with their supporting cover

### T2: Carve and replicate bullet-sized openings

**Where**: common/src/map/destruction.rs
**Integration**: shared channel/schema, server cell/projectile geometry, client receive/mesh paths, updated obsolete coarse-only expectations and corresponding unit/integration tests. These compile-coupled producers/consumers form one deliverable.
**Depends on**: T1
**Tests**: unit/integration
**Gate**: Full
**Done when**:
- [ ] DC-02 and DC-03 exact dimensions, adjacent solid material, same-tick pass-through, penetration and ammo assertions pass.
- [ ] DC-05 wire/cut-state roundtrip and maximum message budgets pass without increasing the actor snapshot.
**Commit**: feat(destruction): carve and replicate bullet-sized cover damage

### T3: Release unsupported cover into server physics

**Where**: server/src/tick/destruction.rs
**Integration**: support graph, lifecycle/snapshot wiring, client rubble presentation and co-located headless engine/client regressions; final guide/Mermaid/state review and independent validation.
**Depends on**: T2
**Tests**: unit/integration
**Gate**: Build
**Done when**:
- [ ] DC-04 supported/unsupported separation, stable IDs, falling/floor contact/late drift and controller exclusions pass.
- [ ] DC-05 rubble late-arrival/sequence assertions and DC-06 count/lifetime/reset assertions pass.
- [ ] Full gate result and unexecuted rendered/transport criteria are recorded honestly.
**Commit**: feat(destruction): simulate and replicate unsupported cover rubble

## Cross-checks

| Task | Depends on | Diagram | Test co-location |
| --- | --- | --- | --- |
| T1 | None | first | integration in same unit |
| T2 | T1 | T1 -> T2 | shared/server/client tests in same compiling unit |
| T3 | T2 | T2 -> T3 | physics/client tests in same unit |

Each task has one user-visible deliverable. Integration files are explicit because shared serialization and its producers/consumers must compile together; no test-only follow-up task defers required coverage.

## T1 evidence

| Criterion | Assertion evidence | Outcome | Necessary coverage |
| --- | --- | --- | --- |
| DC-01 marks and unrelated surfaces | server/src/tick/tests/destruction.rs:37 `assert_eq!(state.impact_marks.len(), 2)`; line 38 checks the literal floor position; line 44 checks only anchor 0 remains | Supported-cell marks removed, others retained | DC-01 |
| DC-01 reset | server/src/tick/tests/destruction.rs:54 `assert!(state.impact_marks.is_empty())`; line 55 `assert!(state.impact_cells.is_empty())` | Both empty | DC-01 |

Adequacy: all T1 outcomes have exact assertions, the integration test maps only to DC-01, and existing four coarse-cover regressions still pass. Quick gates: 1 destruction and 4 voxel tests passed, none ignored. No test was weakened or removed.

Concurrent workspace note: while T1 was being staged, the 60 Hz session committed shared server anchor declarations, the destruction-test module declaration and the guide sentence in `9403702`. Preserve that commit; T1 commits its remaining impact/reset call sites, fixture and feature records without rewriting history.
