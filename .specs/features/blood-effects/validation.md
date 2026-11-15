# Blood effects validation

## Validation: PASS (scoped feature acceptance)

Date: 2026-11-15. Independent verifier: `/root/verify_voxels`, not the blood-effects implementation author. Reviewed scope: new `client/src/render/blood.rs`, its module/plugin registration, feature specification and tests in the uncommitted diff against `b845547`. The concurrent voxel implementation was reviewed separately.

All four scoped behavioral criteria are supported by headless evidence. This verdict does not certify rendered appearance, real network delivery or a clean full workspace gate.

## Spec-anchored evidence

| Requirement | Spec-defined outcome | Located assertion and implementation evidence | Verdict |
| --- | --- | --- | --- |
| BL-01 | New trauma/wounds emit eight droplets; repeated hits spray again; new missing limb roots emit 24 droplets at the stump without descendant duplication | `client/src/render/blood.rs:350` asserts one burst; `client/src/render/blood.rs:351` asserts count 8; `client/src/render/blood.rs:361` asserts repeated-hit count 8; `client/src/render/blood.rs:373` asserts exactly one 24-drop stump emission. `client/src/render/blood.rs:378` checks that the chosen source segment is its parent. | Covered |
| BL-01 | Burst position follows the actual posed segment in world space | `client/src/render/blood.rs:406` asserts emitted position is within 0.001 m of literal `(12.925, 4, 2)` for a packed forearm pose at `(1,2,3)`, root `(10,2,3)` and 90-degree yaw. This numeric oracle is independent of production `source()`. | Covered after sensor fix |
| BL-01 | Unchanged snapshots and initial late-arrival state do not replay injury bursts | `client/src/render/blood.rs:356` and `client/src/render/blood.rs:358` assert no emissions for repeated/unchanged snapshots; `client/src/render/blood.rs:383` asserts a late-arriving already-injured actor emits no burst. | Covered |
| BL-02 | Positive bleeding living actors drip at bounded 2–12/s; stale, dead, healthy or bloodless actors stop | `client/src/render/blood.rs:423` asserts four drops for one second at bleeding 1; `client/src/render/blood.rs:424` asserts single nonburst droplets from the wound; `client/src/render/blood.rs:427`, `client/src/render/blood.rs:429`, `client/src/render/blood.rs:432` and `client/src/render/blood.rs:436` assert zero emissions for stale/dead/nonbleeding/bloodless actors. `client/src/render/blood.rs:434` asserts the 12/s cap. The same assertions run with both bot values. `client/src/render/blood.rs:124` iterates all replicated players without a local-player exclusion; local and remote actors follow the same path. | Covered |
| BL-03 | Newly observed detached pieces drip four/s at replicated coordinates for six seconds and do not replay late-arrival effects | `client/src/render/blood.rs:458` asserts four drops after one second; `client/src/render/blood.rs:459` asserts all positions equal literal `(4,0.2,3)` and are nonburst. `client/src/render/blood.rs:464` asserts no emission after the six-second lifetime and `client/src/render/blood.rs:466` asserts no late-arrival replay. `client/src/render/blood.rs:195` bounds piece age and `client/src/render/blood.rs:196` defines four/s. | Covered |
| BL-04 | Floor contacts become stains lasting 60 seconds; expired airborne droplets cannot revive at floor contact; airborne lifetime is three seconds | `client/src/render/blood.rs:477` asserts first contact; `client/src/render/blood.rs:478` asserts stain state; `client/src/render/blood.rs:479` asserts literal 60-second remaining life; `client/src/render/blood.rs:481` and `client/src/render/blood.rs:482` assert expiration at 60 seconds. `client/src/render/blood.rs:497` and `client/src/render/blood.rs:498` prevent expired-drop revival. `client/src/render/blood.rs:508` through `client/src/render/blood.rs:510` assert a high airborne droplet expires after literal three seconds without becoming a stain. | Covered |
| BL-04 | At most 512 droplets and 256 stains; lobby return and raid changes clear effects and histories | Production blood plugin runs in a MinimalPlugins ECS fixture. `client/src/render/blood.rs:568` asserts literal 512 active drops; `client/src/render/blood.rs:572` asserts zero drops after bounded advancement; `client/src/render/blood.rs:573` asserts literal 256 stains; `client/src/render/blood.rs:576` asserts zero stains after lobby return. `client/src/render/blood.rs:517` through `client/src/render/blood.rs:521` assert reset on lobby/raid changes and empty actor/piece histories. | Covered |

All six focused tests derive from these requirements. The implementation reads existing authoritative anatomy/snapshots and changes no damage, health, physics authority or network schema. Existing rendering materials/meshes are reused across emissions; bounds and lifetimes prevent unbounded effect entities. Collision is intentionally limited to the yard floor, consistent with the specification's exclusions for wall decals and fluid simulation.

## Discrimination sensor and fix loop

Three distinct compiled behavior mutations ran in an isolated source copy, never in the real tree:

| Mutation | Focused test outcome | Final verdict |
| --- | --- | --- |
| Initialize injury history with zero trauma/wounds/missing instead of current authoritative values (`client/src/render/blood.rs:130`) | `blood_bursts_follow_new_trauma_and_separation_without_snapshot_replay` fails its late-arrival no-replay assertion. Log `/tmp/tk-blood-verifier-mutant1.log`. | Killed |
| Remove the expired-droplet guard from `advance` (`client/src/render/blood.rs:229`) | `blood_floor_contact_and_raid_reset_preserve_finite_lifetimes` fails when an already expired droplet becomes a fresh stain. Log `/tmp/tk-blood-verifier-mutant2.log`. | Killed |
| Shift every actor blood source 100 metres horizontally (`client/src/render/blood.rs:93`) | Initially survived all five tests, exposing a shared-helper expected-value oracle. The author added the explicit packed-rig/world-coordinate test. Repeating the exact mutation now fails at `client/src/render/blood.rs:406`: actual `(112.925,4,2)` instead of `(12.925,4,2)`. Logs `/tmp/tk-blood-verifier-mutant3.log` and `/tmp/tk-blood-verifier-mutant3-recheck.log`. | Killed after one fix iteration |

The final independent baseline command `cargo test -p client blood_ -j 4` passed all six tests, zero ignored, in the refreshed isolated copy. Log `/tmp/tk-blood-verifier-baseline-final.log`. The focused client command supplements the project's verification entry point because its quick mode only accepts common/server packages; it does not substitute for the final full gate.

Before report writing, real-tree porcelain exactly matched the final pre-sensor baseline; SHA-256 hashes of all client/common/server Rust production and test files also matched. The initial surviving-mutant fix was an authorized author test change preceding that final isolation baseline. The scratch directory `/tmp/tk-blood-verify-1ilz3m_q` was removed. No production code/test file was changed by this verifier. All three final mutants were killed by meaningful assertion failures, not compilation errors.

## Gate scope and limitations

The final combined serial workspace run passed 138 tests: 45 client, 44 common and 49 server, zero ignored. Log: `/tmp/tk-combined-tests-final.log`. It ran after forcing recompilation of unchanged real source, with no competing verifier suite. The combined full gate passed formatting and workspace check, then stopped at the preexisting `common/src/combat/anatomy.rs:194` `clippy::too_many_arguments` diagnostic; its workspace-test step was not reached. Log: `/tmp/tk-combined-full.log`. This feature does not modify that function and does not waive the lint.

No rendered client, GPU context, listening transport, screenshot or video capture was used. Droplet visibility, blood art, splatter framing and real-network artistic behavior remain unverified. No commit or publishing permission was inferred.

## Grounded lesson

The initial source-shift survivor demonstrated that using the production coordinate helper to compute expected coordinates can hide incorrect world-space placement. Record this project-local effect-position assertion lesson through the TLC lessons script, with the source-shift sensor above as grounding.

## Shared Cargo artifact follow-up

After source-isolated sensors, the author's first real-tree workspace run reused the client artifact containing the source-shift mutant from the shared `CARGO_TARGET_DIR`. The independent coordinate assertion exposed the cached artifact as `(112.925,4,2)` even though the real source contained no shift. This unsuccessful run is retained in `/tmp/tk-combined-tests.log`; it is not evidence of a real-source behavior failure or a successful final gate.

The author forced recompilation from unchanged real source by refreshing its timestamp; the ensuing serial run passed all 45 client and 44 common tests. The author also forced real server source recompilation to prevent reuse of voxel sensor artifacts. File contents remained unchanged. This reveals that scratch source isolation also needs artifact isolation or an explicit real-source rebuild before accepting subsequent root test evidence. The final serial run is recorded in `/tmp/tk-combined-tests-final.log`; all 49 server tests also passed, bringing the final serial run to 138 passing tests with zero ignored.

Two candidate lessons were recorded by the TLC script: L-001 for the independent effect-position oracle and L-002 for shared Cargo mutation artifact reuse. Both are grounded in the incidents above; neither is promoted to confirmed guidance yet.
