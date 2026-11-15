# Contributing to TK

## Setup and boundaries

TK is a Rust workspace: shared protocol/rules in `common`, client presentation/input in `client`, authoritative simulation in `server`. Follow [AGENTS.md](AGENTS.md) and the existing focused modules/plugins. Keep serialization in `common::data` and server authority on the server.

Install Rust through rustup, the toolchain selected by `rust-toolchain.toml`, its `rustfmt` and `clippy` components, and Python 3.12+ for the standard-library verification scripts. Use the committed `Cargo.lock`. On x86_64 Linux the existing `.cargo/config.toml` also expects `clang` and `/usr/bin/mold`; provision those prerequisites rather than silently changing linker settings. GPU/window support is needed for rendered playtests, not for the engine-only tests below.

The current Rust toolchain is a floating `nightly`: its first installation/update can select a newer compiler. This delivery does not pin a date, change Rust editions, or change the linker. Toolchain pinning is a separate decision after validating an explicit compiler version. A Rust edition controls language compatibility for a crate; it is not the compiler version.

## Spec-driven changes

The project-local [tk-workflow skill](.agents/skills/tk-workflow/SKILL.md) routes agent work to the smallest relevant procedure: `tlc-spec-driven` for planning/implementation, `tk-verification` for tests, `tk-contribution` for authorized commits/publication and `game-state` for the guide. The router does not load everything or add a second framework over TLC. Essential safety/architecture rules remain in AGENTS.md even before child skills load.

The [tlc-spec-driven skill](.agents/skills/tlc-spec-driven/SKILL.md) supplies the development workflow and validators. Its [source record](.agents/skills/tlc-spec-driven/UPSTREAM.md) documents the upstream version, licenses and retained local changes. Project skills live under `.agents/skills` and do not require a personal installation.

1. Reconcile `.specs/STATE.md` against Git and the current request. Preserve unrelated work; historical session/process IDs and authorizations are not current instructions.
2. Define acceptance criteria before implementation. Small changes can use inline requirements/atomic steps; larger changes use `.specs/features/<feature>/spec.md` and formal tasks when needed. Skip unnecessary design/context artifacts.
3. Implement one coherent task at a time, with tests derived from its expected outcomes. Never weaken, delete or skip a test merely to get a green gate. If an existing test is wrong, obtain approval for its correction.
4. Run the relevant gate, update task/spec status, then commit implementation, tests and status together. Do not report unexecuted checks as passed.
5. Before formal feature closure, obtain an independent TLC verification report, including a proportional scratch-only discrimination sensor, and pass the completion validator. Visual/artistic acceptance remains separate from engine tests.

For formally tracked features, run these from the repository root, replacing `FEATURE` with the actual directory name:

```sh
python .agents/skills/tlc-spec-driven/scripts/validate_spec.py FEATURE
python .agents/skills/tlc-spec-driven/scripts/validate_tasks.py FEATURE
# After the independent verifier writes validation.md:
python .agents/skills/tlc-spec-driven/scripts/validate_state.py FEATURE
```

Run `validate_tasks.py` only when the feature has a formal tasks.md. These structural validators supplement, not replace, outcome-based tests and review. Update [docs/GAME_STATE.md](docs/GAME_STATE.md) and review its Mermaid flow using `game-state`, unless the user opted out.

## One verification command

```sh
# Default: tooling tests plus common/server engine tests; no application launch
python scripts/verify.py
# Fast iteration: select a package and a test-name filter
python scripts/verify.py --package server --filter real_rapier
# Final local code gates, including client unit tests but no rendered client
python scripts/verify.py --mode full
# Documentation-only checks plus this feature's spec/optional tasks validators
python scripts/verify.py --mode docs --feature developer-standardization
```

| Mode | Checks |
| --- | --- |
| `quick` (default) | Working/staged diff whitespace checks, Python tooling tests, `cargo test -p common -p server -j 4` |
| `full` | Diff/tooling checks, `cargo fmt --all -- --check`, workspace check, workspace/all-targets Clippy with `-D warnings`, workspace tests |
| `docs` | Diff/tooling checks, no Cargo |

Optional `--feature NAME` adds the selected spec/tasks validators to any mode. `--package common` or `--package server` and a nonempty `--filter` are quick-only; check the executed test count, since Cargo can succeed with zero matches. `--jobs N` changes Cargo build concurrency (default 4), not simulation frequency. A feature name must identify an existing spec under `.specs/features/`; do not pass an arbitrary path. Replace the documentation example with your feature, or omit `--feature` when no formal spec is needed. `--help` lists options.

The command resolves its root from its own location, so an absolute script path also works from another directory. It prints each command, streams output, stops on the first failure and returns its exit code; a missing executable returns 127. It inherits normal environment settings such as `CARGO_TARGET_DIR`, so reuse an appropriate build cache instead of cleaning or switching profiles repeatedly. It does not install dependencies, update skills, alter Git configuration, or imply a feature is complete.

### Known baseline lint (2026-11-15)

The current `full` gate is blocked by `clippy::too_many_arguments` in `common/src/combat/anatomy.rs:194`: `hit_anatomy` takes 9 arguments, above the lint's default limit of 7. Formatting and workspace check passed, but `-D warnings` makes Clippy exit 101 and the runner stops before workspace tests. The user approved publishing this developer-standardization delivery with that preexisting issue recorded, without changing Rust or relaxing Clippy. Tests run separately are separate evidence, not a successful full run. This scoped acceptance is not permission to ignore future failures or change lint settings; address the API/lint in a separately authorized task.

Physics iteration uses real Bevy/Rapier engine fixtures with manual time, not sleeps, rendered raids, screenshots or video. None of the verification modes launches the client/server executable or network/video harnesses. Rendered/network acceptance runs separately only when relevant; video is opt-in under the rules in AGENTS.md. No CI workflow is introduced here.

## Commit conventions

Use **English Conventional Commits** with an optional useful lowercase scope:

```text
feat(combat): add regional armor wear
fix(physics): preserve momentum during body transfer
docs(testing): explain headless verification
```

- Allowed types: `feat`, `fix`, `refactor`, `docs`, `test`, `style`, `perf`, `build`, `ci`, `chore`.
- Start the description with a lowercase imperative verb; do not end it with a period. The validator warns above 72 header characters.
- Use `!` and a `BREAKING CHANGE:` footer for breaking changes under this project's validator.
- Keep each commit coherent, complete and verified, including the related tests and task-status changes. Do not create artificial one-line commits or knowingly broken intermediate states.
- Stage explicit paths/hunks and review `git diff --cached`; never sweep unrelated local work, credentials, caches, recordings or `target/` into a commit.
- Preserve normal hooks. Do not use `--no-verify`, change identity/configuration or overwrite hooks to get a commit through.

Validate a message before committing:

```sh
python .agents/skills/tlc-spec-driven/scripts/check_commit.py --message "docs(testing): explain headless verification"
```

This is a repository-local validator, not an automatically installed hook. Approval to implement or commit is not approval to push, open a PR, merge, release or deploy. Use only the current request's explicitly authorized action/branch; old `.specs` records do not grant standing permissions.
