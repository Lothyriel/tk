# TK agent guide

## Start here

Activate `tk-workflow` for project work. It selects only the relevant local skills; do not load every skill for every request or turn analysis into implementation.

Use the Rust toolchain selected by `rust-toolchain.toml` and the committed `Cargo.lock`. On x86_64 Linux, install `clang` and `/usr/bin/mold`. Python 3.12+ is used by verification and playtest tools.

| Skill | Owns |
| --- | --- |
| `tk-workflow` | Routing, scope and resume checks; no second execution framework |
| `tlc-spec-driven` | Proportional requirements, implementation tasks and independent final verification |
| `tk-verification` | Verification command, real engine-only physics tests and scoped visual/network exceptions |
| `tk-contribution` | Safe incremental English Conventional Commits and explicitly authorized publishing |
| `game-state` | Current game guide and Mermaid review after project changes |

All project skills live in `.agents/skills/`. Activate them by ID; do not depend on personal paths or fetch updates during a task. If a required skill is unavailable, report the blocker. The TLC snapshot's origin, licenses and retained local changes are in [UPSTREAM.md](.agents/skills/tlc-spec-driven/UPSTREAM.md).

## Project map and architectural rules

This is a three-crate Rust workspace; root `Cargo.toml` owns shared dependency versions.

| Area | Entry points and responsibilities |
| --- | --- |
| `common/` | `src/lib.rs`: shared ECS/network types, constants and plugin; `src/data.rs`: centralized rkyv serialization; combat, humanoid, map, raid and weapon-animation modules |
| `client/` | `src/main.rs`: wiring; `input/`: input; `sync/`: input sending/snapshot application; `render/` and `hud.rs`: client presentation |
| `server/` | `src/main.rs`: headless wiring; `tick/`: authoritative fixed-step simulation, collisions, combat, raid and state broadcasts; `tick/tests.rs`: engine fixtures |

- Shared protocol/data and genuinely shared systems belong in `common`; client UX/render/input in `client`; gameplay authority/world rules in `server`. Do not duplicate protocol types or place rendering/server authority in the wrong crate.
- Route all network encoding/decoding through `common::data::{encode, decode}`. Extend shared types first, then update the server producer/consumer and client producer/consumer together. Preserve compatible serialization traits and verify both binaries build.
- Input flows from client collection through serialized messages to server validation/simulation; authoritative snapshots/events return to client sync/presentation. Keep this ownership intact.
- Follow focused modules with a local `Plugin` registering systems in `build()`. Keep `main.rs` for wiring, reuse existing ECS resources/components where they fit, and keep the server headless/minimal with only needed plugins.
- Match surrounding Rust style. Do not refactor unrelated code or introduce abstractions merely for hypothetical reuse.

## Always-on safety and verification rules

- Preserve unrelated local/staged work. Never reset/discard it or include it without authorization. Stage explicit paths/hunks; preserve normal Git hooks and identity.
- Use English Conventional Commits, such as `fix(input): correct lean direction`.
- Integrate branches by rebasing the topic branch onto the target, then fast-forwarding the target to the rebased commit. Never create merge commits or use a non-fast-forward merge.
- Derive tests from acceptance criteria. Do not weaken, delete or skip tests just to pass. Commit verified coherent tasks with related tests and task/spec status updates together.
- **Physics/gameplay iteration is headless-first. Do not open the game or run rendered harnesses by default. Video capture is opt-in, not a normal test artifact.** Use assertions, numerical measurements and logs; see `tk-verification` for exceptions and fixture details.
- The single verification entry point is `python scripts/verify.py`: default/quick runs tooling plus common/server tests; `--mode full` runs workspace formatting/check/Clippy/tests; `--mode docs` runs diff/tooling checks without Cargo. None launches the game or records video. Use the smallest relevant gate during iteration and full code checks once before delivery; zero matched tests is not evidence.
- Rendered/real-network/artistic acceptance is separate. A build or headless PASS cannot certify it. Verify relevant startup/transport behavior separately when changed; report unexecuted criteria honestly.
- Independent final TLC verification uses isolated scratch mutations, never the user's real working tree. A tooling PASS does not replace its report or formal feature-completion gate.
- Reconcile `.specs/STATE.md` with current Git/task evidence before resuming. Historical sessions, process IDs and publishing permissions are not current instructions; a merge is not proof of completed acceptance.
- Commit approval does not authorize push, PR, merge, release or deploy. Follow only the current request's authorized action and destination.

## Game documentation

For every project implementation/change, activate `game-state`, review [docs/GAME_STATE.md](docs/GAME_STATE.md) and its Mermaid diagram, and update the guide after the change and before the final response. Keep the diagram stable when gameplay is unchanged and record the review scope. An explicit one-request opt-out expires after that request; a session pause remains until the user resumes updates.
