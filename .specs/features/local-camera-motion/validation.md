# Local camera motion validation

## Validation: FAIL

The camera behavior passes the scoped headless acceptance checks. Feature completion remains blocked by the required workspace Clippy gate, which fails on unchanged code. Rendered smoothness and real UDP delivery remain unverified.

**Date:** 2026-11-15
**Verifier:** independent sub-agent, author != verifier
**Diff:** uncommitted working tree against `7311e2e1b970654de065b686765cf37fcb46222a` on `main`
**Scope:** `client/src/sync/mod.rs`, new `client/src/sync/motion.rs`, feature `spec.md`, and `docs/GAME_STATE.md`. No commit or publication authority.

## Task completion

The medium feature has no formal `tasks.md`; its three implicit steps are implementation, regression coverage and guide review. Implementation and four new tests are present. The guide describes the 100 ms display delay and outstanding visual/network acceptance at `docs/GAME_STATE.md:157` and `docs/GAME_STATE.md:173`. Its gameplay diagram is unchanged. Final verification is blocked as described below. After the sensors completed, the parent updated requirement traceability to `verified` for the headless behavior, retaining an explicit overall acceptance blocker, and updated `.specs/STATE.md`. This verifier changed only this report.

## Spec-anchored acceptance evidence

| Criterion | Required outcome | Assertion evidence | Scoped result |
| --- | --- | --- | --- |
| CM-01 | Walk 3.25 m/s and run 5.5 m/s, approximately 30 Hz snapshots on a 128 Hz timeline; root and two child cameras advance at 60/165 FPS, each warm frame step within 1 mm despite one dropped update and 0..8 ms jitter | `client/src/sync/motion.rs:152` selects both FPS and speeds; lines 163-177 produce the tick timeline, drop sequence 23 and add jitter. `client/src/sync/motion.rs:205` computes `abs(x - previous - speed / fps)` and `client/src/sync/motion.rs:228` asserts `max_error < 0.001`. `client/src/sync/motion.rs:213` asserts both children's propagated x equals root x within 0.1 mm; line 214 checks eye height. | PASS |
| CM-02 | Confirmed 3D path sampled 100 ms behind the first receipt anchor; authoritative session and local look preserved; no path overshoot or outage extrapolation | `client/src/sync/motion.rs:241` asserts the literal 3D midpoint `(1.0, 1.5, -0.5)` at receive time 10.12 after samples at server 1.0/1.04 and receive 10.0/10.04; lines 248-249 assert prehistory/long-outage endpoints. `client/src/sync/motion.rs:202` asserts root rotation equals the changing local look. `client/src/sync/motion.rs:272` asserts session position equals the latest delivered authoritative position. `client/src/sync/motion.rs:284` asserts all sampled stopping/outage x values in `[0,1]`; line 289 asserts final `(1,1,0)`. `client/src/sync/motion.rs:216` additionally asserts displayed x never exceeds the latest authority during moving trajectories. | PASS |
| CM-03 | First receipt and each specified discontinuity snap immediately and clear old history; old/duplicate network sequences cannot change target | `client/src/sync/motion.rs:303` exercises first receipt, alive change, >4 m displacement, physical-mode change, participant-status change, raid ID change, return to lobby, >0.25 s gap, and timeline restart. `client/src/sync/motion.rs:332` asserts exact immediate confirmed root position in every case; line 337 asserts exactly one buffered sample. `client/src/sync/motion.rs:346` supplies an old and duplicate sequence; lines 351 and 355 assert unchanged root and authoritative session respectively. | PASS |

All three ACs have precise outcomes and assertions matching them. No spec-precision gap was found. Constant-speed maximum step error was printed as **0.000001 m** for all four FPS/speed combinations, below the required 0.001 m. Camera evidence uses actual Bevy transform propagation through two children, without rendering; the production cameras are children of the same local root at `client/src/render/mod.rs:163` and `client/src/render/mod.rs:168`.

### Necessary and sufficient test mapping

| New test | Necessary coverage | Sufficiency and limits |
| --- | --- | --- |
| `local_camera_motion_is_frame_paced_at_walk_and_run_with_loss_and_jitter` | CM-01; CM-02 local yaw and moving-path upper bound | Exercises production sync plugin, serialized Renet messages, manual frame time and hierarchy propagation. Measures every warm frame, including the dropped-update interval. Sufficient for the numerical trajectory matrix; does not establish perceived visual smoothness. |
| `local_camera_motion_samples_the_confirmed_3d_path_with_a_100ms_delay` | CM-02 exact delay, non-axis-aligned path and endpoint clamping | Literal midpoint is independent of the implementation's delay constant. Covers interpolation in x/y/z and both ends of the path. Complements the integration fixture. |
| `local_camera_motion_stops_at_confirmed_position_and_keeps_session_authoritative` | CM-02 stop, prolonged outage, newest session state | Checks bounds each frame, final confirmed endpoint, and exact session state after each receive. Sufficient for the specified hold/no-extrapolation behavior. |
| `local_camera_motion_resets_history_on_discontinuities_and_rejects_old_sequences` | CM-03 reset matrix and sequence rejection | Checks both displayed position and cleared history, plus obsolete session rejection through the production receive path. Threshold boundary equality at exactly 4 m/0.25 s is verified statically by strict `>` comparisons at `client/src/sync/motion.rs:30`, rather than by a boundary test. |

Every new test maps to an AC. Existing tests and assertions were not removed or weakened. Source test counts against HEAD: client **45 -> 49**, common **44 -> 44**, server **49 -> 49**; total **138 -> 142**.

## Gates and actual executions

- Required project gate, run by the author and independently inspected here: `python scripts/verify.py --mode full --feature local-camera-motion` (default four Cargo jobs), log `/tmp/tk-camera-full.log`. Diff checks, all 20 Python tooling tests, spec validator, workspace formatting and workspace check passed. Workspace all-targets Clippy with `-D warnings` exited **101** at `common/src/combat/anatomy.rs:194`, `clippy::too_many_arguments` (9/7). The same function is present in HEAD. The entry point stopped there, so its workspace test stage did not run. This is a failed required gate, not a waiver.
- Supplemental author-run serial workspace tests, independently inspected log `/tmp/tk-camera-workspace-tests.log`: **142 passed, 0 failed, 0 ignored**, comprising client 49, common 44, server 49. Common doc-tests contained zero tests and do not add evidence. These tests do not convert the failed full gate into a pass.
- Supplemental client Clippy log `/tmp/tk-camera-client-clippy.log`: 13 diagnostics outside changed files; no sync diagnostic. This also failed and is not substituted for workspace Clippy.
- Independent isolated baseline: `CARGO_TARGET_DIR=/home/jx/repos/tk/target cargo test -p tk-camera-sensor local_camera_motion -- --test-threads=1 --nocapture`: **4 passed, 0 failed, 0 ignored, 45 filtered**, exit 0.
- Independent real-tree confirmation after scratch cleanup: `cargo test -p client local_camera_motion -- --test-threads=1 --nocapture`, log `/tmp/tk-camera-verifier-real.log`: **4 passed, 0 failed, 0 ignored, 45 filtered**, exit 0. The filtered tests belong to the explicit focused run; all 49 client tests ran in the separate workspace suite.
- No rendered client, real socket transport harness, screenshot, or video was launched. No zero-matched test run is counted as evidence.

## Discrimination sensor

Lightweight depth: three independent behavior-level mutations, applied one at a time to copies under `/tmp/tk-camera-verifier-_kbnwck5/scratch`. Each run used the same four-test command as the isolated baseline. Original scratch source was restored between faults.

| Mutation | Original location | Observed assertion failure | Outcome |
| --- | --- | --- | --- |
| Set 100 ms interpolation delay to zero | `client/src/sync/motion.rs:8` | `client/src/sync/motion.rs:228`: maximum step error 0.17434895 m; `client/src/sync/motion.rs:241`: literal delayed 3D midpoint mismatch. Two failed, two passed, exit 101. Log `/tmp/tk-camera-sensor-delay.log`. | Killed |
| Suppress history clearing by changing `if reset` to `if false` | `client/src/sync/motion.rs:33` | `client/src/sync/motion.rs:332`: reset case 2 displayed `(0.5,1,0)` instead of `(1,1,0)`. One failed, three passed, exit 101. Log `/tmp/tk-camera-sensor-reset.log`. | Killed |
| Accept duplicate snapshots by changing sequence `>` to `>=` | `client/src/sync/mod.rs:140` | `client/src/sync/motion.rs:351`: root became `(100,1,0)` instead of unchanged `(9,1,0)`. One failed, three passed, exit 101. Log `/tmp/tk-camera-sensor-sequence.log`. | Killed |

**Sensor:** 3 injected, 3 killed, 0 survived. These samples establish discrimination for the tested faults, not exhaustive mutation coverage.

### Artifact and working-tree isolation

The scratch workspace copied the current sources, committed lockfile/toolchain and Cargo configuration. Only its client **package name** changed to `tk-camera-sensor`; this gives client compilations their own artifact namespace while reusing unchanged third-party dependency artifacts. Baseline compilation printed the scratch source path and executable `/home/jx/repos/tk/target/debug/build/tk-camera-sensor/7a87b74c1f412bc2/out/tk_camera_sensor-7a87b74c1f412bc2`; it compiled only `tk-camera-sensor`, taking 6.12 seconds. Mutant executions used that namespace. The post-sensor real test executed `target/debug/build/client/5c2b12eb11b36e49/out/client-5c2b12eb11b36e49`, the distinct real-source namespace previously compiled by the author's final workspace run. No mutated common/dependency source was used. A further forced real rebuild was unnecessary because scratch artifacts never occupied the `client` package namespace.

The scratch directory was deleted. No stash, reset, real-source mutation, or source timestamp edit was used. Before and after sensors, HEAD, the complete porcelain text and the following SHA-256 hashes were identical. Captures are `/tmp/tk-camera-verifier-_kbnwck5/before.json` and `after.json`.

```text
 M client/src/sync/mod.rs
 M docs/GAME_STATE.md
?? .specs/features/local-camera-motion/
?? client/src/sync/motion.rs
```

| File | SHA-256 before = after |
| --- | --- |
| `client/src/sync/mod.rs` | `0eeabcc2a95b374832c7aa1866f5864ca704b3c336330a3814eda35a8957a8e5` |
| `client/src/sync/motion.rs` | `959d546a1b103302aa751fed0069f3cc5969e21e6e221d0dda619e42ad8a2553` |
| `.specs/features/local-camera-motion/spec.md` | `9bc9d9f248507cdca12887b4b692900ac798559bbfa8238d471d28d9591cd391` |
| `docs/GAME_STATE.md` | `b92675b027298ba37bdcbd6170f8b4be79b95a7367b6ec1eb036fef6e6a4a7d3` |

The only later real-tree addition by the verifier is this report, inside the already untracked feature directory. After the isolation check and scratch cleanup, the parent separately updated spec traceability and `.specs/STATE.md`; those later authorized documentation changes do not invalidate the captured before/after sensor comparison. Production source and game-guide hashes remained unchanged.

## Scoped static review

The production change stays in client presentation. `client/src/sync/mod.rs:177` records confirmed local positions before `client/src/sync/mod.rs:245` stores the newest authoritative players. `client/src/sync/motion.rs:39` anchors the clock once per continuous stream, avoiding receive-jitter re-anchoring. `client/src/sync/motion.rs:52` clamps interpolation to adjacent confirmed endpoints; line 58 holds the final sample during outages. `client/src/sync/motion.rs:69` changes translation alone, preserving local look rotation. The receive/present ordering is established at `client/src/main.rs:37`. History is bounded at `client/src/sync/motion.rs:42`.

No protocol, authoritative movement, remote interpolation, rendering configuration, or unrelated code was changed. The small history type and focused module fit existing ownership and plugin patterns. The production root/camera hierarchy supports the headless fixture's assumptions. Physical head animation remains outside this feature's scope. Guidelines reviewed: `AGENTS.md`, `tk-workflow`, `tk-verification`, TLC `validate.md` and `coding-principles.md`. This static review and scoped tests do not bypass the failed closing gate.

## Ranked gaps and follow-up

1. **Required full gate fails on baseline Clippy.** Restore a green workspace lint baseline in separately authorized scope, then rerun `python scripts/verify.py --mode full --feature local-camera-motion`. Preserve existing assertions and warnings-as-errors. No unrelated lint fix or allowance was made here.
2. **Manual smoothness acceptance remains open.** The spec expressly leaves visible smoothness to manual playtesting. Review ordinary walk/run near cover at high frame rate, including the tradeoff of 100 ms movement display latency. Headless transform assertions cannot establish perceived smoothness. Real UDP delivery also remains unverified; serialized in-memory Renet packets prove the receive path, not OS transport behavior.

No feature implementation defect, surviving mutant, missing numerical AC assertion, or spec-precision gap was found. CM-01/02/03 are supported by the scoped evidence, while formal completion stays blocked. `python3 .agents/skills/tlc-spec-driven/scripts/validate_state.py local-camera-motion` was run and exited 1 because this report correctly carries a FAIL verdict; the completion gate remains closed. Lesson signal `gate_fail` exists; no lesson was written because this verifier was authorized to write only `validation.md`, and the parent judged duplicate guidance about existing lint debt out of scope.
