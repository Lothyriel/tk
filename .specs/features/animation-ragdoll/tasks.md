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

Preliminary independent review after the102-test run found local ADS/weapon lowering dependent on rig presence (wrong at saturated-budget fallback) and an empty-MP5 handle snapping when authoritative reload ended. Mode-gated prediction and residual filtered handle action fixed both; their actual-lens/visibility/handle regressions passed in subsequent111-test checkpoints. The newest overhand wrist also passes a real assembled-mesh camera-facing slide-visibility guard, but needs fresh native review. Artistic acceptance remains mandatory; the separate recovery checkpoint change was explicitly approved as AD-011.
**What**: Build the original M1911 with articulated parts, gloves and filtered pose transitions; integrate shared curves and confirm in a native continuous route.
**Where**: `client/src/render/weapons.rs`
**Depends on**: T2
**Requirement**: ANIM-01, ANIM-02, ANIM-03, ANIM-04
**Additional requirement**: ANIM-13, ANIM-14
**Done when**:
- [ ] M1911 native sequence reviewed at real speed and adjacent frames; contact, slide, magazine and flash tests pass.
- [x] First-person pistol framing regression covers actual weapon/glove/visible-magazine vertices across five aspect ratios, empty/tactical reloads, centred ADS, fixed lens and continuous fall lowering; existing cuff/elbow and slide-visibility assertions remain.
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

Historical T4 checkpoint: shared-contact/geometry tests passed in the70-test gate (15client/34common/21server). An outward elbow pole fixed the trigger-arm regression without changing its assertion. That protocol3 candidate used four shared weapon parts and completed native capture as candidate03; it is retained historical evidence, not the current protocol5 implementation.

Historical protocol4 checkpoints: continuous candidates03/06/07 completed reciprocal weapon sequences and the full raid; later candidate10 passed its healthy physical matrix but the user rejected weapon motion/reload/FOV quality. Those technical passes are not final artistic acceptance.

Current protocol5 adds ADS and five shared weapon parts. Candidate16 passed111tests and the complete diagnostic dual/loss raid, including the approved18.2s actual-recovery observation and fatal shot. Both videos fully decoded (3098/3045real samples,129.64s). Its slide was clearer but the pistol cropped at the right edge; the first-person cant had rotated the entire60cm eye offset. The framing follow-up pivots about the receiver, fits cached mesh bounds in the final camera, preserves ADS centring and eases fitting away during deliberate lowering. Focused tests pass; indexed full gates and a disabled-framing negative control are retained in commit-gates/06-pistol-framing. Fresh candidate17 native review is required. T3/T4 artistic acceptance remains open.

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

Historical T6 checkpoint07 recorded fmt/check/84tests and native same-ID living recovery, but exposed own-camera clipping and absent-shin observer aim. Those were subsequently corrected. Current integration coverage retains same-ID fall/get-up/death/separation, actual hit volumes, unsupported/headroom denial, wall recovery and all eight strict floor/wall/corner/step rest scenarios. Candidate14's native surviving-recovery proof agrees on15sameIDs under loss; the newest frozen healthy death matrix remains open.

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

T7 harness checkpoint: native framebuffer capture, full-decode encoding, 17-contact/observer indexes, four-case physical runner and matched-camera/performance tooling are implemented. Candidate14's normal diagnostic dual/loss passed; candidate15's111-test suite passed but its old16.8s recovery assertion failed before naturalStanding17.04s. AD-011 authorizes actual-completion capture bounded18.2s without changing physical eligibility,1sget-up or fatal-shot deadline. The recorder/harness commit is a verified implementation checkpoint, not closure of final native/performance/artistic acceptance.

### T8: Point-blank muzzle clearance without capsule redirection
**What**: Repair the user-approved close-range origin policy: trace eye-to-muzzle clearance through the same posed-anatomy/world collision path as ordinary projectile travel, then continue the unchanged muzzle ray if clear. Keep the original native Injury route and fatal-shot deadline.
**Where**: `server/src/tick/simulation.rs`, server-only projectile state in `mod.rs`, co-located `tests.rs`
**Depends on**: T6
**Requirement**: ANIM-15
**Done when**:
- [x] Normal hip/ADS shots hit close posed targets without coarse-capsule redirection; real pre-muzzle actors/concrete are not bypassed, gaps remain empty and wood attenuation is once; original tests/native assertions pass.
**Tests**: integration + native
**Gate**: Build

## Diagram-Definition Cross-Check

T8 evidence: the pre-fix1.05m head miss is retained in candidate14/gates/point-blank-red.log; candidate14 completed the original native fatal-shot flow. Six new integration tests cover all24weapon/aim/range combinations, exact unchanged damage/ammo/muzzle, real pre-muzzle actors, narrow concrete, leg gaps and wood attenuation across both paths. The isolated commit gate reruns the complete staged workspace; final feature performance and artistic acceptance are still T7/open, not implied by T8 closure.

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
