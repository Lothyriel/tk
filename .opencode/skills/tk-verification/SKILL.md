---
name: tk-verification
description: Selects and runs TK verification gates and real Bevy/Rapier headless physics tests. Use for testing, regression coverage, physics/ragdoll tuning, validation and /goals verification. Keeps rendered/network checks scoped and video opt-in; does not certify visual quality from engine tests.
---

# TK verification

## One command

From the repository root, use Python 3.12+ and the existing Rust toolchain:

```sh
python scripts/verify.py
python scripts/verify.py --package server --filter real_rapier
python scripts/verify.py --mode full
python scripts/verify.py --mode docs --feature FEATURE
```

- Default/`quick`: working/staged diff checks, Python tooling tests, common/server engine tests. `--package common|server` and a nonempty `--filter` narrow quick mode; confirm the run executes matching tests, since zero matches is not evidence. Discover names with `cargo test -p server -- --list` when needed.
- `full`: tooling/diff checks, fmt check, workspace check, all-targets workspace Clippy with warnings denied, workspace tests. It runs client unit tests, not the client application. Run once for final code validation, not after every small solver adjustment.
- `docs`: diff/tooling checks, no Cargo. Use for documentation-only edits.
- Optional `--feature NAME`: validate that feature's spec and tasks if present, using project-local TLC validators. Replace FEATURE with the actual feature name, or omit this option when no formal spec exists. This does not run the final completion validator or prove a feature complete.

All modes print commands, stream output and stop at the first failure. See CONTRIBUTING.md for arguments and setup. None launches a game, network harness or recorder. Reuse the debug build/cache, including caller-supplied `CARGO_TARGET_DIR`; do not clean or switch profiles/target directories repeatedly. Missing prerequisites are blockers, not reasons to weaken checks.

## Real physics without a window

For authoritative physics/gameplay, start with the smallest relevant test in `server/src/tick/tests.rs` or `common`. Reuse `app()`, `physical_fixture()`, `steps()` and `held_steps()` instead of duplicating physics. The fixture uses `MinimalPlugins`, `TransformPlugin`, `common::Plugin` (real Rapier) and production `tick::Plugin`; Renet resources/messages are in memory, with no listening transport or GPU initialization.

Keep `TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / 128.0))` and the production 128 Hz fixed schedule. Advance `app.update()`/`steps()` as fast as the CPU allows, with bounded ticks and no sleeps or real-time countdowns. Do not increase the timestep, reduce solver quality, disable collisions or mock physics for speed. Use `held_steps()` for sustained input so normal stale-input expiry does not change the scenario.

Use explicit initial conditions and assert real ECS outcomes: finite transforms/velocities, floor/wall contacts, anatomical limits/gaps, settling and late drift, stable IDs/counts, damage and recovery. Sample the relevant time window for stability claims, not just one final frame. Use justified tolerances rather than assuming cross-platform bit identity. An injected fixture injury/impulse can isolate a solver regression, but does not prove the complete firing or networking path.

The existing rest/run, floor/wall/corner/step matrix is available as:

```sh
python scripts/verify.py --package server --filter ragdoll_running_and_resting_contact_matrix
```

After the focused regressions pass, broaden coverage as needed and run the final full gate on the completed code. Do not replace spec-defined expected outcomes with current implementation output, weaken assertions or relabel historical failures.

## Rendered/network exceptions and video

Engine tests do not certify camera/FOV, animation, clipping, interpolation, artistic quality or real UDP delivery/loss. For changed boot/networking, verify relevant startup/transport behavior separately; explain the criterion before launching the smallest necessary scenario. `scripts/playtest.py`, `scripts/playtest_physical.py` and `scripts/validate_humanoids.py` include rendered clients, so they are not headless shortcuts. Reserve full raid/dual-client acceptance for the relevant final gate or an explicit request, not the inner physics loop.

Video is opt-in, including final rendered acceptance. For physics, use assertions, measurements and logs, not video/framebuffer dumps. Do not add `--video` or invoke recording, encoding, frame-index or contact-sheet helpers just because older evidence used them. Prefer minimal screenshots for static visual checks. Record continuous footage only on explicit user request or when a specific visual/temporal acceptance criterion genuinely needs it; state the reason and keep capture scoped. Measure ordinary performance without capture overhead.

Keep required visual/network criteria unverified until actually checked. TLC owns independent final verification and scratch-only mutations; select proportional gates here without adding a second verification pipeline.
