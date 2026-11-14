# Physical humanoids: implementation and acceptance evidence

Status: complete for the local playable alpha. Acceptance-11 passed all seven command gates on unchanged source with 64 tests, normal raid and two rendered UDP clients. Independent verification passed all 12 requirements and seven original criteria; three isolated behavior mutants were killed, none survived. See [the independent report](../.specs/features/physical-humanoids/validation.md).

## Reference boundary

The FRUKT dump was available locally and was inspected as a behavioral/structural reference, not copied into TK. `COMTransformControl.cs` exposes cached ground hits and ground-distance/angle controls; the corresponding native dump contains `Physics.Raycast` (line 518). `LimbPhysics.cs` exposes colliders, a configurable joint and a rigid body. `LimbDismembermentModule.cs` references `VoxelMeshSeparationModule`; its native dump calls the separation module (lines 454, 469 and 475). `DefaultMuscle.cs` exposes a muscle-force/integrity multiplier. These observations informed TK's own support probes, tissue strength and voxel separation.

All TK meshes, rig poses, damage rules and tuning are original procedural Rust code. No Unity assets, demo code or recovered native implementation are shipped. Stack dependency versions are unchanged.

## Reproduction

```powershell
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace -j 4
python scripts/playtest.py
python scripts/playtest.py --clients 2 --output target/playtest/dual-review
python scripts/validate_humanoids.py
python scripts/compare_performance.py --before target/playtest/baseline-metrics-01 --after target/playtest/acceptance-NN/normal --load-log target/playtest/acceptance-NN/physical-load.log --output target/playtest/acceptance-NN/performance.json
```

The two-client run uses normal `ClientInput` over UDP. It adds an idle/walk/run/strafe/crouch/jump/landing/stop sequence before looting, primary magazine exhaustion, rejected empty triggers, interrupted reload, pistol shot/reload and full primary reload. It completes extraction/debrief/new deploy before a cooperative anatomical shooting sequence. The leader receives a right-leg wound; the peer receives a left-leg and arm wound, separation, fall/recovery and fatal shots. The peer discards every third snapshot. Stable wound/organ/piece IDs are compared at the end. Audio entity count is asserted zero every rendered frame. The remote inspection camera hides only its first-person overlay during anatomical review; it does not change server position, aim, damage or simulation.

`scripts/benchmark_baseline.py --output target/playtest/baseline-metrics-NN` exports immutable HEAD into an isolated directory and adds only tick/snapshot/frame logging, then runs the original normal raid route. It never checks out, resets, stashes or replaces working-tree source or a running player's binary. Its `baseline.json` records the revision. Do not run native benchmarks concurrently on the same port/GPU.

## Implemented systems

- `common/src/humanoid.rs`: 17 stable segments, original articulated two-bone arm/leg poses, directional planted/swing phases, crouch/jump/landing/support pose, posed OBB ray tests and sorted intersections.
- `common/src/combat/anatomy.rs`: seven regions, regional muscles, brain/heart/liver/stomach volumes, blood/vigor, armor-gated internal injury and one-time descendant separation. Internal intersections follow the ray across region boundaries, with tunable tissue penetration (80 energy units/metre); a thorax entry can reach the liver behind it. Four old aggregate health pools remain only for compatibility telemetry and legacy rule tests; live projectiles use regional anatomy.
- `server/src/tick/physical.rs`: two-foot ground probes, impulse correction, authoritative fall/action blocking, clearance-gated recovery and velocity/pose transfer to bounded Rapier bodies/joints. Pieces live 35 seconds and are capped at 320. Actor/debris collision groups avoid self-collision explosions and owner/visibility filters ignore debris.
- `client/src/render/actors.rs`: reusable stepped voxel shells, carved wound shells, muscle/bone/organ interiors, layered equipment, weapon silhouettes, interpolated remote roots/poses and persistent physical pieces.
- `client/src/render/feedback.rs`: shooter/shot-ID confirmed flash, capped spring recoil, bolt, reload magazine/support hand, swap and camera feedback. Firing loads/spawns no audio; original `.ogg` files remain.
- `server/src/tick/metrics.rs`: complete fixed-tick timing; snapshots and native driver log sizes/frame timing. Simulation remains 128 Hz and snapshots approximately 30 Hz. Incompatible archives use protocol 2.

## Authoring and manual inspection

Mesh units are metres. A segment mesh is centred on its shared bone centre; local +Y points toward its proximal joint and actor forward is -Z. Keep `Segment::index()` stable and replace cached `VoxelAssets` handles, not meshes per actor/frame. Change `Segment::half_size()` and organ offsets alongside a silhouette change so damage and presentation stay aligned. `voxel_mesh` owns shell/cavity detail, `organ_mesh` owns the four original internal silhouettes, and the wardrobe details add gloves, soles, pads, pouches, face and equipment. First-person placement is centralized in `view_model_translation`; barrel tips still use the unchanged ballistic offsets.

To inspect manually, deploy two clients with no vest/helmet for clear tissue tests, then aim at the **actor's** left or right side (mirrored when facing them). One pistol hit demonstrates a partial limp; repeated hits to the same shin separate shin/foot at 78 accumulated tissue damage. The peer can walk to show support loss and recovery. A Ball rifle hit through an unarmored brain destroys it immediately. Heart is slightly actor-left in the chest; liver is actor-right in the upper abdomen; stomach is actor-left/lower. Stop firing after a partial organ hit to observe progressive blood/vigor effects instead of destroying the entire region. Use AP when testing penetration against equipped protection; a blocked low-penetration thorax hit must not injure the heart. Normal wooden panels and loot crates retain their old penetration cost; concrete remains blocking.

Tune regional maxima, organ multipliers, blood thresholds, tissue penetration and separation in `common/src/combat/anatomy.rs`. Tune gait in `common/src/humanoid.rs`, support/recovery/joints/caps in `server/src/tick/physical.rs`, and recoil/flash/reload-hand feedback in `client/src/render/feedback.rs`. Destroyed trigger-arm capacity now blocks server firing/reload; lost support-hand presentation disappears. The old aggregate speed penalty is combined with the regional penalty by minimum, so injuring a second leg cannot improve movement.

## Acceptance evidence index

Final evidence root: `target/playtest/acceptance-11/`; paths beginning with `dual/` below are relative to that root. `gates.json` records seven successful command gates and `source-fingerprint.json` binds the run to unchanged source. The independent audit in `target/playtest/verifier-01/audit.json` confirmed the final source hashes and that all 34 original test bodies remain unchanged. Previous runs remain intact as historical evidence.

| Original criterion | New reproducible evidence |
| --- | --- |
| 1. Visual/locomotion | Compare `target/playtest/baseline-metrics-01/raid.png` with `target/playtest/acceptance-11/normal/raid.png`. `dual/pose-{idle,walk,run,strafe,crouch,jump,land,stop}.png` shows first person; the same filenames in `dual/peer/` show the full remote rig. Both cameras use matching MSAA. The stop frame records deceleration; idle separately records zero velocity. |
| 2. Weapons | `dual/weapon-*.png` and `dual/peer/weapon-*.png`, `dual/confirmed-rifle-flash.png`, `dual/peer/confirmed-smg-flash.png` and both `confirmed-pistol-flash.png`; both `client.log` files print `WEAPON EXERCISE PASS`. Runtime assertions check zero audio entities every frame, including player/sentry combat. Unit tests cover confirmed-shot dedup, swap timing and bounded recoil. |
| 3. Injury/physics | `dual/injury-{healthy,lateral-limp,arm,correction,fall,recovery,separation,ragdoll}.png` and matching peer captures; `PHYSICAL PLAYTEST PASS` in both logs. Server tests assert correction without falling, fall/recovery, headroom gating, prone floor clearance and death/debrief ragdoll transfer; client tests bound the crouched-fall camera above the floor. |
| 4. Anatomy/ballistics | Final `tests.log` records real projectile tests for both sides, standing/crouched/fallen anatomy, gaps, one-time armor, wood/concrete and brain/heart/liver/stomach/muscle effects. `dual/injury-arm.png`, `dual/injury-separation.png` and especially the close `dual/injury-recovery.png` show damaged surface/interior and missing segments; rig/anatomy tests assert absent-volume removal and one-time separation. |
| 5. Multiplayer | `dual/result.txt` records two native rendered clients plus UDP server and 50 extra frames. Both logs have identical `CONVERGENCE` missing/wound/organ and 17 unique piece-ID records after the peer discards every third snapshot. Client assertions compare actual render entities with replicated IDs. Native second-deploy assertions cover cleanup after sentry corpses; the `next_deploy_restores_separated_anatomy_and_clears_fragments_corpses_and_effects` server regression additionally covers an actual separated player's death/debrief/new deploy. No third native raid is claimed. |
| 6. Regression/performance | Final `{fmt,check,tests,normal-raid,dual-raid,physical-load,snapshot-budget}.log`, both `result.txt` files and `performance.json`. All 64 tests passed (12 client, 31 common, 21 server); fresh independent fmt/check/tests also passed in `target/playtest/verifier-01/`. The normal route preserves equipment/ammo/loot/extraction/debrief/second deployment. See measured comparison below. |
| 7. Delivery | `docs/GAME_STATE.md` contains controls, current mechanics, launch steps and reviewed Mermaid; this document contains reproduction, tuning, original-asset provenance and criterion-by-criterion evidence. Independent verdict: PASS in `.specs/features/physical-humanoids/validation.md`; completion validator passed. |

## Measured performance

`target/playtest/acceptance-11/performance.json` compares the same original solo route against immutable HEAD baseline `baseline-metrics-01`. Steady frame windows exclude the cold first window on both versions, not slow individual frames.

| Metric | Before | After |
| --- | --- | --- |
| Median window mean tick | 0.505 ms | 0.500 ms |
| Worst window p95 tick | 0.772 ms | 0.773 ms |
| Observed maximum tick | 3.650 ms | 3.134 ms |
| Median warm-window mean frame | 4.652 ms | 6.028 ms |
| Worst warm-window p95 frame | 6.088 ms | 6.783 ms |
| Maximum normal snapshot | 480 bytes | 2,280 bytes |

The additional deterministic 320-active-piece Rapier test measured mean 1.464 ms, p95 1.980 ms and max 2.172 ms per update, below the approximately 7.8 ms tick budget. The full bounded replicated anatomy/fragment stress snapshot was 50,504 bytes, below the 60,000-byte test bound. Warm frame mean increased 29.6% and maximum normal snapshot 4.75 times: voxel detail and persistent state have real rendering/bandwidth costs. Native dual evidence includes 34 active corpse pieces (tick p95 0.892 ms, snapshot 3,512 bytes); its leader warm frame windows reached p95 12.815 ms. These are separate local workloads, not a WAN or 32-rendered-client guarantee. Subjective art quality/camera comfort remain alpha-level judgments, not proven by numeric assertions.

## Resolved validation findings

Failed attempts remain evidence, not passes. `voxel-first` hit a Windows executable lock; isolated harness outputs fixed it without stopping an unknown existing server. `voxel-second` exposed the old broad-capsule aiming assumption; the driver now solves the actual offset-barrel ray. Early dual screenshot review exposed mismatched MSAA trails; cameras now match. Acceptance-04 caught a headroom-query bug; overlap queries fixed it. Acceptance-05 exposed inaccurate close-range scripted aiming; analytical parallax and near/far tests fixed only the input driver. Acceptance-06 correctly rejected edits made during its run; acceptance-07 and acceptance-10 passed frozen-source guards.

Later regressions cover prone collision/recovery, crouched-fall camera height, organs behind an adjacent entry region, bilateral leg penalties, destroyed trigger-arm functionality and ragdoll creation when the last death enters debrief. The first instrumented baseline also exposed Cargo path-package artifact aliasing when a source export shared the real target directory; the helper now builds in isolation. No user source, branch or unknown running process was reset. At acceptance time the feature was uncommitted; a subsequent user request authorizes its commit series and push to `feature/animacoes-mitologicas`, without merge or deployment. Each staged code snapshot passed fmt/check/tests in an isolated copy (38, 38, 63 and 64 tests); logs are retained in `target/playtest/commit-gates/`. Concurrent video-recording changes are not part of this accepted source snapshot.
