# 60 Hz snapshot trial validation

## Validation: FAIL

Scoped behavioral checks pass. The required full gate stops at an existing Clippy failure; visible smoothness and real UDP performance remain unverified. The user explicitly requested a local trial and committing pending changes after being informed of these limitations.

Scope: changes against camera commit `6a4a99b`. This is a standalone review and isolated sensor under TLC's no-subagent fallback: the side conversation prohibits subagents, so this report does not claim independent agent verification. No destruction work is included.

## Acceptance evidence and test adequacy

| AC | Required result | Concrete assertion | Result |
| --- | --- | --- | --- |
| SR-01 | Fixed timestep stays 1/128 s | `server/src/tick/tests.rs:70`: `assert_eq!(...timestep(), Duration::from_secs_f64(1.0 / 128.0))` | Behavioral PASS |
| SR-01 | 60 produced and decoded snapshots per simulated second | `server/src/tick/tests.rs:100`: sequence difference equals literal `60`; `server/src/tick/tests.rs:104`: `assert_eq!(timestamps.len(), 60)`; lines 95–96 assert one serialized player with ID 7 | Behavioral PASS |
| SR-01 | Each interval is two or three physics ticks | `server/src/tick/tests.rs:107`: `(ticks - 2.0).abs() < 0.0001 || (ticks - 3.0).abs() < 0.0001` | Behavioral PASS |
| SR-02 | Every warm render-frame motion step within 1 mm at walk/run and 60/165 FPS, with loss/jitter | `client/src/sync/motion.rs:228`: `max_error < 0.001`; both propagated camera children match root x at line 213 and eye height at line 214 | Behavioral PASS |
| SR-02 | Literal 50 ms 3D interpolation delay and endpoint clamping | `client/src/sync/motion.rs:241`: at receive time 10.07, distance to literal midpoint `(1,1.5,-0.5)` is below `0.00001`; lines 248–249 assert the history endpoints | Behavioral PASS |
| SR-02 | Look, authoritative state, stopping, discontinuities and sequence rejection preserved | `client/src/sync/motion.rs:202`: rotation equals local look; line 272: session position equals newest confirmed position; lines 284/289: bounded stopping and final endpoint; lines 332/337: reset position and one history sample; lines 351/355: obsolete snapshots change neither root nor session | Behavioral PASS |

The one added server test maps to SR-01. The four existing motion tests map to SR-02. The previous 100 ms assertion was updated to the trial's new 50 ms requirement with the same tolerance and literal midpoint; no assertion was removed or weakened. No speculative tests were added. Both expected cadence and delay have literal outcomes independent of the shared production constant.

The shared interval controls server snapshot cadence and local buffer duration. Simulation remains 128 Hz, and no serialized shape, protocol ID, movement rule or remote interpolation algorithm changes. The guide and unchanged Mermaid player flow were reviewed.

## Commands and measurements

- Before implementation, `python3 scripts/verify.py --package server --filter snapshots_60hz` executed the new test and failed meaningfully with actual `30` versus required `60`: `/tmp/tk-60hz-before.log`.
- After implementation, the same focused entry point passed one server test plus 20 Python tooling tests: `/tmp/tk-60hz-server.log`.
- `cargo test -p client local_camera_motion -- --test-threads=1 --nocapture` passed four tests; maximum frame-step error printed approximately 0.000001 m for each FPS/speed combination: `/tmp/tk-60hz-client.log`.
- `cargo test --workspace -j 4 -- --test-threads=1 --nocapture` passed **143 Rust tests**, zero ignored: 49 client, 44 common, 50 server. The common doc-test target matched zero tests and adds no evidence. Log: `/tmp/tk-60hz-workspace.log`.
- The server fixture delivered **60 snapshots and 35,040 bytes of serialized payload** in one simulated second with one lobby player. This is in-memory Renet transport, excludes packet overhead, and does not predict bandwidth for a populated raid.
- `python3 scripts/verify.py --mode full --feature snapshots-60hz` passed diff/tooling/spec checks, formatting and workspace compilation, then failed at unchanged `common/src/combat/anatomy.rs:194`, `clippy::too_many_arguments`: `/tmp/tk-60hz-full.log`. Its test stage was not reached; the supplemental workspace run above supplies test evidence without claiming the full gate passed.

## Isolated discrimination sensor

| Fault injected into scratch only | Observed outcome | Result |
| --- | --- | --- |
| Restore server snapshot threshold/modulus to 1/30 s, leaving client/shared timing at 60 Hz | Server cadence test fails actual 30 vs required 60 | Killed |
| Restore client delay to 100 ms | Literal 50 ms midpoint assertion fails | Killed |

Both unmodified scratch baselines passed first (one server test and four client tests). Both mutants exited 101 from assertion failures, not compilation failures; **2 injected, 2 killed, 0 survived**. Logs: `/tmp/tk-60hz-sensor-baseline-server.log`, `/tmp/tk-60hz-sensor-baseline-client.log`, `/tmp/tk-60hz-sensor-cadence-mutant.log`, `/tmp/tk-60hz-sensor-buffer-mutant.log`.

Only copied server/client source was mutated. Scratch package names `tk-60hz-server-sensor` and `tk-60hz-client-sensor` isolate final package artifacts while sharing unchanged dependency artifacts in the existing target directory. Common source was copied unchanged and never mutated. No stash or real-source restoration was used. The scratch was removed. HEAD, complete porcelain and all six scoped file hashes matched before/after; evidence captures are `/tmp/tk-60hz-sensor-85tp1ito/before.json` and `after.json`. Subsequent spec/status/report edits were outside that completed isolation interval.

No surviving mutant, uncovered numerical criterion or new test failure remains. Existing lint debt and lack of visual/UDP acceptance keep formal completion open. No new lesson is recorded for the already documented baseline lint blocker.
