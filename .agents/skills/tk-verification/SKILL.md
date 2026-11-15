---
name: tk-verification
description: Use for TK tests, regression coverage, physics/ragdoll tuning, performance and acceptance verification. Select proportional central-runner checks and real Bevy/Rapier fixtures; distinguish headless evidence from visual and network acceptance.
---

# TK verification

## One command

From the repository root, use Python 3.12+ and the existing Rust toolchain:

```sh
python scripts/verify.py
python scripts/verify.py --package server --filter real_rapier
python scripts/verify.py --package client --filter local_camera_motion
python scripts/verify.py --mode full
python scripts/verify.py --mode docs
```

- Default/`quick`: working/staged diff checks, Python tooling tests, common/server engine tests. `--package client|common|server` and a nonempty `--filter` narrow quick mode; confirm the run executes matching tests, since zero matches is not evidence. Discover names with `cargo test --locked -p server -- --list` when needed.
- `full`: tooling/diff checks, fmt check, workspace check, all-targets workspace Clippy with warnings denied, workspace tests. It runs client unit tests, not the client application. Run once for final code validation, not after every small solver adjustment.
- `docs`: diff/tooling checks, no Cargo. Use for documentation, skills and Python tooling edits; add focused tooling checks when the change needs them. Rust changes require a final `full` run.

All modes print commands, stream output and stop at the first failure. See AGENTS.md for setup and use `python scripts/verify.py --help` for arguments. None launches a game, network harness or recorder. Reuse the debug build/cache, including caller-supplied `CARGO_TARGET_DIR`; do not clean or switch profiles/target directories repeatedly. Missing prerequisites are blockers, not reasons to weaken checks.

## Real physics without a window

For authoritative physics/gameplay, start with the smallest relevant test in `server/src/tick/tests.rs` or `common`. Reuse `app()`, `physical_fixture()`, `steps()` and `held_steps()` instead of duplicating physics. The fixture uses `MinimalPlugins`, `TransformPlugin`, `common::Plugin` (real Rapier) and production `tick::Plugin`; Renet resources/messages are in memory, with no listening transport or GPU initialization.

Keep `TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / 128.0))` and the production 128 Hz fixed schedule. Advance `app.update()`/`steps()` as fast as the CPU allows, with bounded ticks and no sleeps or real-time countdowns. Do not increase the timestep, reduce solver quality, disable collisions or mock physics for speed. Use `held_steps()` for sustained input so normal stale-input expiry does not change the scenario.

Use explicit initial conditions and assert real ECS outcomes: finite transforms/velocities, floor/wall contacts, anatomical limits/gaps, settling and late drift, stable IDs/counts, damage and recovery. Sample the relevant time window for stability claims, not just one final frame. Use justified tolerances rather than assuming cross-platform bit identity. An injected fixture injury/impulse can isolate a solver regression, but does not prove the complete firing or networking path.

The existing rest/run, floor/wall/corner/step matrix is available as:

```sh
python scripts/verify.py --package server --filter ragdoll_running_and_resting_contact_matrix
```

After focused regressions pass, broaden only for new changes or unresolved concerns and run the final full gate on completed Rust code. Derive expected outcomes from acceptance, not current implementation output. Do not weaken assertions or relabel historical failures.

Keep test oracles independent of the production helper being checked. If scratch fault injection is useful for a risky change, isolate both source and build artifacts; never mutate the user's tree or reuse a mutant's compiled artifact as real-tree evidence. No mandatory mutation or separate report is required for every task.

## Rendered/network exceptions and video

Engine tests do not certify camera/FOV, animation, clipping, interpolation, artistic quality or real UDP delivery/loss. For changed boot/networking, verify relevant startup/transport behavior separately; explain the criterion before launching the smallest necessary scenario. `scripts/playtest.py`, `scripts/playtest_physical.py` and `scripts/validate_humanoids.py` include rendered clients, so they are not headless shortcuts. Reserve full raid/dual-client acceptance for the relevant final gate or an explicit request, not the inner physics loop.

Video is opt-in, including final rendered acceptance. For physics, use assertions, measurements and logs, not video/framebuffer dumps. Do not add `--video` or invoke recording, encoding, frame-index or contact-sheet helpers just because older evidence used them. Prefer minimal screenshots for static visual checks. If a temporal criterion requires footage, explain why and obtain the user's opt-in before recording. Measure ordinary performance without capture overhead, matched to the same environment/settings and without competing GPU/port workloads.

Report commands/scenarios, results, matched test counts, environment and limitations in the selected Linear card through `tk-linear` (or chat for a cardless hotfix). Keep required visual/network criteria unverified until actually checked. A baseline failure remains a failed gate: distinguish it from task regressions and link the follow-up rather than masking it.
