# Animation and ragdoll Tasks

## Execution Protocol

Use tlc-spec-driven and game-state. Execute inline under the user's instruction to proceed. AD-012 now explicitly authorizes incremental English Conventional Commits and push to feature/animacoes-mitologicas; preserve/exclude preexisting AGENTS.md/.opencode and ignored evidence. A fresh independent verifier runs after implementation. Artistic acceptance remains separate from committing a validated implementation checkpoint.

**Design**: `.specs/features/animation-ragdoll/design.md`
**Status**: In Progress

## Test Coverage Matrix

Guidelines: `AGENTS.md`, existing co-located Rust tests and `scripts/playtest.py`.

| Code Layer | Required Test Type | Coverage Expectation | Location Pattern | Run Command |
| --- | --- | --- | --- | --- |
| Shared curves/rig | unit | Outcomes for anchors, continuity, stance, empty/tactical actions | common/src/**/*.rs | cargo test -p common |
| Client presentation | unit + native | Feedback bounds, duplicate rejection, native visual review | client/src/**/*.rs | cargo test -p client |
| Server physics | integration | Initial pose/momentum, direction/contact matrix, recovery, rest, lifecycle | server/src/tick/tests.rs | cargo test -p server |
| UDP raid | e2e | Normal and loss peer complete raid and action/physical assertions | scripts/playtest.py | python scripts/playtest.py --clients 2 |

## Gate Check Commands

| Gate Level | When to Use | Command |
| --- | --- | --- |
| Quick | Unit implementation | cargo test --workspace -j 4 |
| Full | Native integration | python scripts/playtest.py --clients 2 --output target/playtest/animations-dual |
| Build | Phase closure | cargo fmt --all -- --check; cargo check --workspace; cargo test --workspace -j 4 (separate invocations) |

## Execution Plan

### Phase 1: Recorded weapon slice

T1 → T2 → T3 → T4

### Phase 2: Body continuity

T5 → T6 → T8 → T7

## Task Breakdown

### T1: Native baseline recorder
**What**: Record timestamped real framebuffers from the unchanged raid, encode both clients and retain settings/metrics. Supporting integration: playtest plugin and Python encoder/harness.
**Where**: `client/src/playtest/video.rs`
**Depends on**: None
**Requirement**: ANIM-11
**Done when**:
- [x] Baseline dual UDP run passes and both MP4s decode; real frames/sidecars retained.
**Tests**: native integration
**Gate**: Full

T1 evidence: `animations-before-safe-04/dual/result.txt` PASS, 3346/3338 real framebuffer samples, 126.98/126.83 s, zero tail trim, both `video-evidence.json` full FFmpeg decode PASS. Source export is exact 705d035 presentation plus driver/recorder. Settings, PNGs and timestamps retained; contact indexes contain all 17 action points on each client.

### T2: Shared weapon action vocabulary
**What**: Define weapon-specific art anchors and reload curves, with tests of contacts/endpoints/empty actions.
**Where**: `common/src/weapon_animation.rs`
**Depends on**: T1
**Requirement**: ANIM-01, ANIM-02
**Done when**:
- [x] Curves differ by weapon and empty state, continuous endpoints and grip/muzzle tests pass.
**Tests**: unit
**Gate**: Quick

T2 unit evidence (initial run): `cargo test -p common weapon_animation -- --nocapture`: 2 passed, 0 failed. `original_rigs_have_distinct_muzzles_and_support_contacts` asserts muzzle ordering and pistol two-hand proximity; `reload_contacts_endpoints_and_empty_release_follow_each_weapon` asserts exact grip/magazine return, magazine-hand distance below 1 mm during contact, adjacent curve bounds, and empty-only bolt release. Full workspace gate and traceability closure remain pending.

T2 workspace gate subsequently passed: 14 client + 33 common + 21 server = 68 tests, zero failures, original assertions retained. T3 prototype is integrated, with native frames in `target/playtest/animations-m1911-01/`; full continuous route review remains open. Early extended baseline `animations-before-continuous-03` exposed sentry damage during the long showcase and failed the original weapon assertion after a fall; no assertion was weakened. The revised driver walks to safe staging first. `record_animation_baseline.py` exports immutable 705d035 presentation with only the ordinary-input recorder overlaid, using unique Cargo package names, so the revised route can be compared fairly after presentation work has started.

### T3: M1911 continuous first-person slice
User refinement: include held-RMB ADS with sight alignment, natural movement cadence, stable FPS framing and a readable empty-slide manipulation. Candidate10 models were accepted as a first delivery, but motion/reload/FOV were explicitly rejected; do not mark artistic acceptance complete from the passing native suite.

Preliminary independent review after the102-test run found local ADS/weapon lowering dependent on rig presence (wrong at saturated-budget fallback) and an empty-MP5 handle snapping when authoritative reload ended. Current source now gates local aim by physical mode and preserves the filtered handle after interruption. Two new regressions cover fallback Fallen/Recovering with actual lens/weapon visibility and locked-handle continuity. They await the next gate/native freeze; no existing assertion changed. A composed-render/contact visual review remains mandatory.
**What**: Build the original M1911 with articulated parts, gloves and filtered pose transitions; integrate shared curves and confirm in a native continuous route.
**Where**: `client/src/render/weapons.rs`
**Depends on**: T2
**Requirement**: ANIM-01, ANIM-02, ANIM-03, ANIM-04
**Additional requirement**: ANIM-13, ANIM-14
**Done when**:
- [ ] M1911 native sequence reviewed at real speed and adjacent frames; contact, slide, magazine and flash tests pass.
**Tests**: unit + native integration
**Gate**: Full

### T4: MP5 and M4A4 shared model presentation
**What**: Extend the same cached weapon component to original primary models and third-person articulated actions, preserving own anchors/timing.
**Where**: `client/src/render/weapons.rs`
**Depends on**: T3
**Requirement**: ANIM-01, ANIM-02, ANIM-03, ANIM-04
**Done when**:
- [ ] Each primary has distinct geometry and action evidence in both perspectives; visual muzzle follows model.
**Tests**: unit + native integration
**Gate**: Build

T4 shared-contact/geometry tests passed in the 70-test workspace gate (15 client, 34 common, 21 server). Original trigger-arm regression was fixed with an outward elbow pole, not a changed assertion. World weapons now use the same four part meshes, materials, magazine/handle curves and muzzle anchor. PoseState grew persistent empty-reload/contact state and protocol is 3; rebuild both binaries. Frozen native candidate `animations-body-candidate-03` is running independently of subsequent source changes.

Later evidence: continuous candidates03/06/07 completed both reciprocal weapon sequences and the full raid. Current protocol is **4**, including optional packed actual physical poses. Candidate08 supersedes their presentation for final review; historical protocol3 evidence does not validate protocol4 by itself.

Current revision supersedes those historical states: protocol **5** adds ADS and five shared weapon part meshes. Candidate11's initial ADS continuous dual/loss raid passed (3411/3409real samples;128.60/128.59s, full decode), but primary stock/sleeve framing defects required post11 corrections. Latest102-test workspace suite passes24client+41common+37server with none failed/ignored. Candidate12 native validation is in progress; T3/T4 remain open for actual visual review, including user artistic acceptance.

### T5: Weight transfer and supported locomotion
**What**: Refine shared pose weight/feet and server support inputs, with planted foot and finite-joint tests.
**Where**: `common/src/humanoid.rs`
**Depends on**: T4
**Requirement**: ANIM-05
**Done when**:
- [ ] Native start/brake/turn/strafe/crouch/jump/land/limp route reviewed; stance tests pass.
**Tests**: unit + native integration
**Gate**: Build

### T6: Articulated momentum and directional contact
**What**: Preserve per-segment pose/motion and localized force across handoff; tune anatomical joints, directional living fall and supported recovery; include deterministic environment integration tests.
**Where**: `server/src/tick/physical.rs`
**Depends on**: T5
**Requirement**: ANIM-07, ANIM-08, ANIM-09, ANIM-10
**Done when**:
- [ ] Pose/motion inheritance and floor/wall/corner/step direction matrix pass; living recovery and death rest reviewed natively.
**Tests**: integration + native
**Gate**: Build

T6 progress: `animations-live-candidate-07/gates/` records fmt/check and84 passing tests (16client/37common/31server). Same-ID living fall/get-up, physical hit volumes, death mid-getup, normal-shot separation reuse, unsupported recovery denial, get-up by wall, original headroom/prone-root assertions and all eight strict death contact scenarios pass. Native07 completed full dual UDP/loss with matching live-recovery.json (15sameIDs), but frame review found own torso/camera clipping and absent-shin observer aim. Both are fixed in candidate08; a new visibility test brings the next complete suite to85. Final frozen visual/death-matrix validation remains open.

Subsequent T6 evidence: candidate10 continuous recovery and four healthy native death cases passed, with no own-torso camera clipping in the inspected get-up interval. Protocol5 human-cadence changes pass all eight contact integration scenarios using6additional solver iterations; original damping/rest/load bounds retained. A new frozen protocol5 healthy native matrix remains required.

### T7: Frozen acceptance evidence and game guide
**What**: Run required gates with frozen source; compare baseline performance, index timestamps and criteria, update guide/Mermaid and request independent verification.
**Where**: `docs/ANIMATION_RAGDOLL.md`
**Depends on**: T8
**Requirement**: ANIM-06, ANIM-11, ANIM-12
**Done when**:
- [ ] fmt/check/workspace tests/single UDP/dual loss UDP pass; evidence index complete and independent verifier passes.
**Tests**: e2e + independent outcome review
**Gate**: Build

### T8: Point-blank muzzle clearance without capsule redirection
**What**: Repair the user-approved close-range origin policy: trace eye-to-muzzle clearance through the same posed-anatomy/world collision path as ordinary projectile travel, then continue the unchanged muzzle ray if clear. Keep the original native Injury route and fatal-shot deadline.
**Where**: `server/src/tick/simulation.rs`, server-only projectile state in `mod.rs`, co-located `tests.rs`
**Depends on**: T6
**Requirement**: ANIM-15
**Done when**:
- [ ] Normal hip/ADS shots hit close posed targets without coarse-capsule redirection; real pre-muzzle actors/concrete are not bypassed, gaps remain empty and wood attenuation is once; original tests/native assertions pass.
**Tests**: integration + native
**Gate**: Build

## Diagram-Definition Cross-Check

| Task | Depends On | Diagram Shows | Status |
| --- | --- | --- | --- |
| T1 | None | None | Match |
| T2 | T1 | T1 | Match |
| T3 | T2 | T2 | Match |
| T4 | T3 | T3 | Match |
| T5 | T4 | Prior phase | Match |
| T6 | T5 | T5 | Match |
| T8 | T6 | T6 | Match |
| T7 | T8 | T8 | Match |

## Test Co-location Validation

All tasks include the tests/native evidence of their modified layer. T7 integrates existing gates without weakening or replacing per-component tests. Artistic outcomes require actual capture inspection, not a green build alone.
