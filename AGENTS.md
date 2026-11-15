# AGENT.md

## Project overview

This repository is a small Rust workspace with three crates:

- `common/`: shared gameplay data, shared Bevy plugin setup, and network serialization helpers.
- `client/`: the playable client app, including rendering, input, and client-side network sync.
- `server/`: the headless server app, including server networking and fixed-timestep game simulation.

The workspace root `Cargo.toml` owns shared dependency versions.

## Current file organization

### `common/`

Primary role: shared code used by both client and server.

Important files:

- `common/src/lib.rs`
  - shared constants like `DEFAULT_PORT`, `PROTOCOL_ID`, `PLAYER_MOVE_SPEED`
  - shared Bevy plugin setup in `common::Plugin`
  - shared ECS/resources/components like `Lobby`, `PlayerId`, `ClientInput`, `ClientData`, `ServerMessage`
- `common/src/data.rs`
  - networking serialization helpers
  - currently uses `rkyv` for encoding/decoding byte payloads sent over `bevy_renet2`

Rule of thumb:
- If a type crosses the client/server boundary, define it in `common`.
- If a system or plugin setup must exist in both binaries, put it in `common`.

### `client/`

Primary role: local player control, rendering, and consuming server state.

Important files:

- `client/src/main.rs`
  - app entrypoint
  - wires together Bevy default plugins, shared plugin, networking plugins, and local feature plugins
- `client/src/input/mod.rs`
  - local input gathering
  - updates `ClientInput` resource
- `client/src/render/mod.rs`
  - camera setup, view model/world model rendering, lighting
- `client/src/sync/mod.rs`
  - sends `ClientInput` to the server
  - receives `ServerMessage` and `Vec<ClientData>`
  - mutates ECS state from replicated/networked data

### `server/`

Primary role: headless simulation and authoritative state broadcast.

Important files:

- `server/src/main.rs`
  - app entrypoint
  - uses `MinimalPlugins` + extra required plugins for headless operation
  - wires shared plugin, server networking plugins, and server tick plugin
- `server/src/tick/mod.rs`
  - authoritative simulation loop
  - receives client input
  - updates transforms
  - broadcasts player positions and connectivity messages

## Architectural pattern already in use

The codebase currently follows this split:

1. **Shared protocol and data models live in `common`**
2. **Client-only UX/render/input logic lives in `client`**
3. **Server-only simulation and authority lives in `server`**

Networking flow today:

- client collects local input into `ClientInput`
- client serializes and sends it with `common::data::encode`
- server decodes it and applies it to player entities
- server simulates authoritative transforms
- server serializes world state / events and broadcasts them
- client decodes and applies them locally

## How to add new features

### 1. Decide where the feature belongs

Use this rule first:

- **Shared data/protocol?** → `common`
- **Client presentation/input only?** → `client`
- **Authoritative simulation / world rules?** → `server`

Examples:

- New replicated player state → add shared type to `common`, send/receive in `client::sync` and `server::tick`
- New camera or HUD behavior → `client`
- New server-side movement or gameplay rule → `server`

### 2. Extend shared protocol in `common` first

If the feature needs networking:

- add or update shared structs/enums in `common/src/lib.rs`
- derive the same serialization traits already used there
- keep serialization routed through `common/src/data.rs`

Do not create ad hoc serialization code in client/server modules when an existing `common::data::{encode, decode}` path already exists.

### 3. Follow the existing plugin/module style

Each area uses a small local `Plugin` type with systems registered in `build()`.

Follow that pattern:

- create a focused module
- expose a `Plugin`
- register systems from the crate entrypoint

Prefer adding a new focused module over stuffing more unrelated logic into `main.rs`.

### 4. Keep server authoritative

Current code treats the server as the source of truth.

When adding gameplay:

- validate or apply gameplay state on the server
- broadcast resulting state to clients
- keep client logic mostly predictive/presentational unless there is a clear reason not to

### 5. Reuse existing ECS resources/components

Before creating new resources, check whether the feature fits existing ones such as:

- `Lobby`
- `PlayerId`
- `Client`
- `ClientInput`
- `ClientData`
- `ServerMessage`

Add new types only when the existing ones are no longer a clean fit.

## Practical guidance for common feature types

### Adding a new replicated field

Example flow:

1. Add the field to the shared type in `common`
2. Update the server to populate/broadcast it
3. Update the client to decode/apply it
4. Run `cargo check` and verify both binaries still build

### Adding a new client action

Example flow:

1. Add input capture in `client/src/input/mod.rs`
2. Extend `ClientInput` in `common`
3. Update server handling in `server/src/tick/mod.rs`

### Adding a new server event

Example flow:

1. Add a new variant to `ServerMessage` in `common`
2. Emit it from the server when the authoritative event occurs
3. Handle it in `client/src/sync/mod.rs`

## Current conventions worth preserving

- Keep shared networked types in `common`
- Keep binary-specific app wiring in each crate’s `main.rs`
- Keep network byte encoding/decoding centralized in `common/src/data.rs`
- Prefer small modules with a local `Plugin` entrypoint
- Use Bevy ECS resources/components consistently instead of passing state around manually
- Keep the server headless/minimal and only add Bevy plugins it actually needs

## Things to avoid

- Duplicating shared protocol types separately in client and server
- Putting client rendering logic in `common`
- Putting server authority logic in the client
- Bypassing the shared serialization layer for network payloads
- Growing `main.rs` files into feature modules instead of using dedicated modules/plugins

## Validation expectations

After changing behavior, start with the smallest relevant headless test, then run the final checks once on the completed change:

- run `cargo check --workspace`
- run `cargo test --workspace -j 4`
- if networking or app boot changed, verify the relevant binary still starts (`cargo run --bin client` or `cargo run --bin server`)

### Headless-first physics testing (including `/goals`)

**Default to engine-only tests for physics and authoritative gameplay. Do not open the client, record video, or run a rendered playtest on each iteration.** The existing tests already execute real Bevy ECS schedules and Rapier physics without a window, rendering, GPU initialization, or a listening UDP socket.

Use this fast feedback loop:

1. Find the relevant test in `server/src/tick/tests.rs` or the shared tests in `common`. Run only that test or a focused name filter while iterating. Use `cargo test -p server -- --list` to discover names, and verify the run actually executes matching tests (zero tests is not evidence).
2. Add a headless regression when coverage is missing. Reuse `app()`, `physical_fixture()`, `steps()` and `held_steps()` rather than creating a second physics implementation. `app()` uses `MinimalPlugins`, `TransformPlugin`, `common::Plugin` (the real Rapier plugin), and the production `tick::Plugin`. It supplies Renet resources/messages in memory, not a network transport.
3. Keep `TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / 128.0))` and the production 128 Hz fixed schedule. Advance with `app.update()`/`steps()` as fast as the CPU allows, with no sleeps or real-time countdown waits. Do not increase the timestep, reduce solver quality, disable collisions, or mock the physics to make a test faster. Use `held_steps()` for sustained input so the normal stale-input timeout does not invalidate the scenario.
4. Set up explicit, reproducible initial conditions and drive the real systems. Assert outcomes from ECS state: finite transforms/velocities, ground and wall contacts, joint limits/gaps, settling and late drift, segment identity/count, damage and recovery. For stability claims, sample the relevant time window, not just one final frame. Bound tick counts and use justified numerical tolerances; do not assume bit-identical physics across platforms. Fixture-only injury/impulse setup is valid for a solver regression, but is not evidence for the complete firing/network path.
5. Reuse the debug build/cache; do not run `cargo clean`, switch profiles/target directories repeatedly, rebuild the rendered client, or rerun the entire acceptance suite for each solver adjustment. Run the broader headless suite after the focused tests pass, and the workspace checks once before delivery. Documentation-only changes need documentation/diff checks, not a full gameplay run.

Examples from the repository (all run without opening the game):

```sh
# Focused real-Rapier projectile/cover tests
cargo test -p server real_rapier -- --nocapture
# Existing rest/run, floor/wall/corner/step ragdoll matrix
cargo test -p server ragdoll_running_and_resting_contact_matrix -- --nocapture
# Broader shared-rules and server-engine regression suite
cargo test -p common -p server -j 4
```

Headless simulation does **not** certify rendered animation, camera/FOV, clipping, interpolation, artistic quality, or actual UDP delivery/loss. Only launch the smallest relevant rendered/network scenario when an acceptance criterion requires it or a headless result cannot reproduce the defect; explain that reason first. `scripts/playtest.py`, `scripts/playtest_physical.py`, and `scripts/validate_humanoids.py` include rendered clients and are not headless shortcuts. Reserve full raid/dual-client acceptance for the final relevant gate (or an explicit user request), not the inner physics loop. Preserve any required final acceptance gates and report unexecuted visual/network criteria as unverified rather than claiming headless tests cover them.

**Video capture is opt-in, not a default test artifact, including during final rendered acceptance.** For physics, use assertions, numerical measurements and text logs, not videos or framebuffer dumps. Do not pass `--video`, run recording/encoding/frame-review helpers, or generate continuous screenshots/contact sheets just to prove simulation behavior. For a static visual check, prefer the minimum relevant screenshots. Record continuous video only when the user requests it or a specific visual/temporal acceptance criterion genuinely requires it; state that criterion before recording and keep capture scoped to it. Do not record merely because a harness supports it or older validation notes used it. Measure performance without capture overhead unless capture itself is the subject of the measurement.

## Game state documentation

- For every request to implement or change the project, load the `game-state` skill. After the implementation and before the final response, follow the skill to update [the game state guide](docs/GAME_STATE.md) and its Mermaid diagram. Review both even when the change appears unrelated to gameplay.
- Skip the skill and its updates when the user explicitly opts out. A request to pause game-state updates for the session applies to subsequent requests in that session until the user asks to resume. A one-request opt-out expires with that request.
