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
| Build | Final delivery | python scripts/verify.py --mode full |

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
- [x] DC-02 and DC-03 exact dimensions, adjacent solid material, same-tick pass-through, penetration and ammo assertions pass.
- [x] DC-05 wire/cut-state roundtrip and maximum message budgets pass without increasing the actor snapshot.
**Commit**: feat(destruction): carve and replicate bullet-sized cover damage

### T3: Release unsupported cover into server physics

**Where**: server/src/tick/destruction.rs
**Integration**: support graph, lifecycle/snapshot wiring, client rubble presentation and co-located headless engine/client regressions; final guide/Mermaid/state review and independent validation.
**Depends on**: T2
**Tests**: unit/integration
**Gate**: Build
**Done when**:
- [x] DC-04 supported/unsupported separation, stable IDs, falling/floor contact/late drift and controller exclusions pass.
- [x] DC-05 rubble late-arrival/sequence assertions and DC-06 count/lifetime/reset assertions pass.
- [x] Full gate result and unexecuted rendered/transport criteria are recorded honestly.
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
| DC-01 marks and unrelated surfaces | server/src/tick/tests/destruction.rs:117 `assert_eq!(state.impact_marks.len(), 2)`; line 118 checks the literal floor position; line 124 checks only anchor 0 remains | Supported-cell marks removed, others retained | DC-01 |
| DC-01 reset | server/src/tick/tests/destruction.rs:134 `assert!(state.impact_marks.is_empty())`; line 135 `assert!(state.impact_cells.is_empty())` | Both empty | DC-01 |

Adequacy: all T1 outcomes have exact assertions, the integration test maps only to DC-01, and existing four coarse-cover regressions still pass. Quick gates: 1 destruction and 4 voxel tests passed, none ignored. No test was weakened or removed.

Concurrent workspace note: while T1 was being staged, the 60 Hz session committed shared server anchor declarations, the destruction-test module declaration and the guide sentence in `9403702`. Preserve that commit; T1 commits its remaining impact/reset call sites, fixture and feature records without rewriting history.

## T2 evidence

| Criterion | Exact assertion evidence | Outcome / necessity |
| --- | --- | --- |
| DC-02 bore width and adjacent solid | common/src/map/destruction.rs:158 `assert_eq!(cut.half_size[0] * 2.0, expected)` and line 159 for height, with literal 6/9/11 mm values; lines 162/172 assert through-bore miss and 3 cm adjacent hit | Exact dimensions and spatial discrimination; DC-02 |
| DC-02 same-tick penetration and ammo | server/src/tick/tests/destruction.rs:23-26 assert 3 projectiles with energies 25.2,25.2,30; existing normal-shot fixture asserts magazine before-1 and remaining wood health126 | Authority and same-tick geometry; DC-02 |
| DC-02 rendered opening | client/src/render/destruction.rs:104 `assert!(!ray_hits(&carved, Vec3::Z))`; line105 asserts adjacent triangle hit | Actual mesh triangles, not helper agreement; DC-02 |
| DC-03 chip/backstop | common/src/map/destruction.rs:216 exact 15 mm chip depth; server/src/tick/tests/destruction.rs:54 health74, line59 projectile count0 and line68 solid-back ray hit; retained concrete destruction test preserves breaking-shot stop | DC-03 |
| DC-05 state and budget | common/src/map/destruction.rs:253 `<16_000` and lines255-260 exact wire fields; client/src/sync/cover.rs:76-87 exact accepted phase/mask/cuts/rubble despite old/duplicate sequences | DC-05 |

Reverse mapping: all three shared geometry/wire tests, two new engine tests, client mesh and client receive tests map to DC-02/03/05; migrated coarse tests retain ammo/penetration/controller/reset/late-arrival assertions. No unrelated coverage was removed. T2 workspace gate: 151 Rust tests passed (51 client,47 common,53 server), zero ignored. A focused real loopback UDP example also delivered authoritative input and the maximum 5,688-byte cover message under protocol9. Existing worst physical actor snapshot remained below60,000 bytes. Logs: `/tmp/tk-destruction-t2-workspace.log`, `/tmp/tk-destruction-udp.log`. Adequacy: all T2 outcomes have exact independent values and sufficient meaningful assertions.

## T3 engineering result

The final full gate passed 286 Rust tests (94 client,119 common,73 server) and41 Python tooling tests, including strict Clippy, with no test changes that weaken outcomes. The preexisting opt-in real-UDP test stays separately invoked; it is the sole ignored default case. New engine cases exercise 32 unsupported cells, stable IDs, actual falling, floor-corner bounds, late drift, controller exclusion, maximum64 rubble,20-second expiry, shared producer poses/rotations/scales after lost updates, reset and sparse-geometry saturation. The new client fixture proves exact first pose/scale, gradual motion, identity across debrief, and mesh removal on lobby/expiry. The old static-layer fixture explicitly removes rubble to isolate its unchanged next-layer assertion; a new test separately proves concrete rubble stops that shot until cleared.

No new rendered review, video or artistic acceptance is asserted. The remaining independent closing review audits the exact AC/line mapping and isolated discrimination evidence. This historical artifact retains task traceability through the integration; it does not recreate the removed STATE workflow.

| T3 criterion | Exact assertion evidence | Spec-defined outcome | Reverse mapping |
| --- | --- | --- | --- |
| Support and real falling | server/src/tick/tests/destruction.rs:159 `assert_eq!(pieces.len(), 32)`; line173 asserts upper y < start_y-0.15 after64 ticks; lines165/169 assert other panel health160 and enabled collider | 32 released, >15 cm fall within0.5 s, neighboring panel static | DC-04 |
| Contact, identity and settling | server/src/tick/tests/destruction.rs:196 checks unchanged entity per cell; line206 asserts corner y>0.075; line220 asserts maximum_late_drift<0.03 | Stable IDs, no floor escape, <3 cm late drift | DC-04 |
| Controller support | server/src/tick/tests/destruction.rs:197/198 assert group4 and no group3 filter; line237 asserts root height in0.95..1.08 after placing player above the settled pile | Debris does not raise the controller | DC-04 |
| Bounds and expiry | server/src/tick/tests/destruction.rs:259 asserts64; line268 asserts <=64 each tick; line276 asserts0 after2561 ticks; line283 asserts176 cells remain removed | At most64; expire by20s, no regrowth | DC-06 |
| Replicated actual pose | server/src/tick/tests/destruction.rs:340/341/342 assert exact position/rotation/scale versus current ECS bodies after discarded packets; line347 requires at least3 recovered full updates | Matching authoritative full state after loss/late arrival | DC-05 |
| Reset | server/src/tick/tests/destruction.rs:355 requires0 rubble; lines362/363 require empty cuts/removals; lines374-376 require max health, released=false and enabled colliders | Full original cover reset | DC-06 |
| Client state and cleanup | client/src/render/destruction.rs:260-262 assert literal pose/scale; line278 requires x strictly between4 and6; lines282/291 preserve Entity; lines307/348 require zero owned mesh assets after removal | Identity, gradual presentation, correct scale and no leaked mesh | DC-05/DC-06 |
| Rubble ballistic support | server/src/tick/tests/destruction.rs:389 neighbor health100 and line393 projectile count0 while rubble remains; line413 neighbor health60 after explicit rubble removal | Real concrete fragment remains bullet-stopping, no floating mark | DC-03/DC-04 |
| Sparse saturation | server/src/tick/tests/destruction.rs:470 health0; line477 solids<=96; line501 unchanged128 cuts and502 next cell removed | Bounded geometry fractures without restoring prior holes | DC-02/DC-06 bounds |

Every new T3 test maps to the criteria above, and every T3 outcome has independent literal/state assertions. The actual mesh-triangle test and serialized producer comparisons avoid a shared-helper-only oracle. Final real UDP gates also passed: protocol15 input plus5,944-byte maximum cover state, and the existing solo/dual/late-join weapon test without loss and with20% loss. Logs: `/tmp/tk-rubble-cover-udp.log`, `/tmp/tk-rubble-weapon-udp.log`. No existing deadline or assertion was weakened. T3 is implemented and verified; independent final review follows its commit.

## Final independent review

Engineering PASS for DC-01..DC-06, with23 independent focused Rust cases and3/3 isolated behavior mutants killed. Report: [validation.md](validation.md). No implementation gap remains. Final sources were not changed by the review; no game, video or rendered acceptance was run. The upstream Clippy cleanup was retained during the conflict resolution, so the final full gate is green rather than the earlier historical blocked result.
