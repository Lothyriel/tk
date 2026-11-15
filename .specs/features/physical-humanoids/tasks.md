# Physical humanoids tasks

Status: T1–T7 complete. Acceptance-11 passed all seven gates on unchanged source (64 tests, normal and dual rendered UDP raids); independent review PASS, three behavior mutants killed, none survived. A subsequent user request authorizes logical English Conventional Commits and push to feature/animacoes-mitologicas; no merge/deploy. Concurrent video-recording work is excluded.

## Test Coverage Matrix
Guidelines: AGENTS.md; existing common combat tests, server Rapier ECS tests, client HUD tests and native playtest.
| Code Layer | Required Test Type | Coverage Expectation | Location Pattern | Run Command |
| --- | --- | --- | --- | --- |
| Shared anatomy/pose | unit | Outcomes for each region, organ, gap, armor and absent segment | common/src/** | cargo test -p common |
| Server simulation | integration | Real projectile, support, reset and physics transitions | server/src/tick/** | cargo test -p server |
| Client presentation | unit + native | Dedup, pose and weapon state plus captures | client/src/** | cargo test -p client |

## Gate Check Commands
| Gate Level | When to Use | Command |
| --- | --- | --- |
| Quick | Shared changes | cargo test -p common |
| Full | Integration | cargo test --workspace -j 4 |
| Build | Complete slice | cargo fmt --all -- --check; cargo check --workspace; cargo test --workspace -j 4 |

## Execution Plan
T1 → T2 → T3 → T4 → T5 → T6 → T7

## Task Breakdown

### T1: Shared posed rig
**Status**: Complete — acceptance-11 shared pose/gap/stance tests and native pose captures; independently verified.
**Where**: `common/src/humanoid.rs`
**Depends on**: None
**Tests**: unit
**Gate**: Quick
**Done when**: Stable segment IDs, articulated poses and ordered ray intersections pass gap/stance tests. TK-01, TK-02, TK-05.

### T2: Regional anatomy
**Status**: Complete — acceptance-11 regional/muscle/organ/armor/separation tests; independently verified.
**Where**: `common/src/combat.rs`
**Depends on**: T1
**Tests**: unit
**Gate**: Quick
**Done when**: Seven regions, muscles, organ effects, blood and single separation pass armor and lateral impairment tests. TK-05, TK-06, TK-08.

### T3: Authoritative physical state
**Status**: Complete — acceptance-11 live projectile/physics/cleanup tests and dual UDP scenario; independently verified.
**Where**: `server/src/tick`
**Depends on**: T2
**Tests**: integration
**Gate**: Full
**Done when**: Live projectiles use posed anatomy, balance/physics is authoritative and persistent snapshots/reset work. TK-05..10.

### T4: Voxel actor presentation
**Status**: Complete — acceptance-11 cached mesh test and independently reviewed remote/first-person captures.
**Where**: `client/src/render`
**Depends on**: T3
**Tests**: unit + native
**Gate**: Full
**Done when**: Layered rig, wounds, armor, remote interpolation and environment pass native visual inspection. TK-01, TK-02, TK-08, TK-09.

### T5: Confirmed weapon feedback
**Status**: Complete — acceptance-11 dedup/recoil/camera tests, confirmed-flash captures, reload cycles and zero-audio runtime assertions; independently verified.
**Where**: `client/src`
**Depends on**: T4
**Tests**: unit + native
**Gate**: Full
**Done when**: Silent firing, deduplicated flash/recoil, handling/reload and camera feedback pass. TK-03, TK-04.

### T6: Reproducible raid validation
**Status**: Complete — acceptance-11 seven gates passed on unchanged source with 64 tests; baseline performance comparison saved and independently reviewed.
**Where**: `scripts`
**Depends on**: T5
**Tests**: integration + native
**Gate**: Build
**Done when**: Both real UDP clients and fixtures provide evidence for original criteria 1..6, including performance. TK-11.

### T7: Delivery guide and independent review
**Status**: Complete — guide/Mermaid and evidence/tuning index finalized; independent validation.md PASS, all seven criteria covered and completion validator passed.
**Where**: `docs`
**Depends on**: T6
**Tests**: independent verification
**Gate**: Build
**Done when**: Guide/Mermaid, tuning and evidence index are accurate; fresh verifier checks all original criteria. TK-12.
