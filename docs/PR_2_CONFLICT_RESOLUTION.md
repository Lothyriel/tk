# PR #2 conflict resolution

Historical audit only. Current development uses [the Linear workflow](DEVELOPMENT.md); the old process files are retained in Git history, and this record grants no current integration permission.

## Cause and preservation strategy

The extraction-alpha commits were recreated on `main` with different commit IDs.
The complete tree of `main` at `19bff54` is identical to the PR's extraction-alpha
checkpoint `e68d43c` (`git diff e68d43c 19bff54 --exit-code` succeeds).
Git therefore reports conflicts against the older common ancestor, even though
the animation branch already contains the entire extraction-alpha baseline.

The resolution retains the evolved animation branch, including its latest
receiver-framing fix `58c1f3a`, and merges `main` without rewriting either history.
No runtime code, assets, dependencies, tests or playtest scripts change relative
to `58c1f3a`; only this audit and the game guide's latest-review note are updated.
The existing temporary gunshot silence is retained, not introduced by this merge.

## Conflicted files and retained behavior

| File | Retained content |
| --- | --- |
| Historical state ledger (removed from the active tree) | Extraction, physical-humanoid and animation progress and open acceptance gates; preserved in Git at 6a4a99b |
| `client/src/hud.rs` | Lobby/loadout/results, extraction HUD and detailed anatomy status |
| `client/src/input/mod.rs` | Equipment/readiness, movement, weapons, interaction and held RMB ADS |
| `client/src/playtest.rs` | Complete raid route, dual-client, weapon and physical acceptance scenarios |
| `client/src/render/mod.rs` | Shared map, cameras, ADS/FOV, articulated actors and weapon presentation |
| `client/src/sync/mod.rs` | Raid replication, ordered snapshots, interpolation, physical pieces and confirmed-shot feedback |
| `common/src/combat.rs` | Loadouts, armor, ammunition, weapon cadence/reload and extended anatomy |
| `common/src/data.rs` | Centralized serialization and full physical-snapshot round-trip tests |
| `common/src/lib.rs` | Shared protocol 5, aim input, poses, physical pieces and shot events |
| `docs/GAME_STATE.md` | Complete extraction flow plus ADS, injuries, falling/get-up and ragdolls |
| `scripts/playtest.py` | Native UDP raid runner, dual clients, snapshot loss and optional video |
| `server/src/tick/bots.rs` | Authoritative sentries and their physical action restrictions |
| `server/src/tick/mod.rs` | Authoritative raid/physics scheduling, input expiry and complete snapshots |
| `server/src/tick/raid.rs` | Lobby/countdown, loot/extraction/debrief and new-raid anatomy/physics cleanup |
| `server/src/tick/simulation.rs` | Movement, weapons, anatomical projectile hits and muzzle/cover clearance |
| `server/src/tick/tests.rs` | Extraction-alpha tests and all added ADS/anatomy/ragdoll/recovery regressions |

The remaining files merge without changes, including `common/src/raid.rs`, map
definitions, dedicated anatomy/rig/weapon modules, physics modules and assets.
The Mermaid player-flow diagram remains unchanged because gameplay is unchanged.

## Validation

- `cargo fmt --all -- --check`: passed.
- `cargo check --workspace --locked`: passed.
- `cargo test --workspace --locked -j 4 -- --test-threads=1`: all 112 tests passed
  (27 client, 42 shared, 43 server), including the unchanged 320-piece load gate.
- The first parallel test run passed 111 tests but exceeded the load gate's
  7.8 ms p95 budget at 8.94 ms while another build was active; the complete serial
  rerun after the playtest passed without changing code or relaxing the threshold.
- `python scripts/playtest.py --output
  C:/Users/sandr/AppData/Local/Temp/opencode/tk-pr-2-evidence`: passed with a real
  rendered client and UDP server through lobby, combat, loot, extraction, debrief,
  return to lobby and second deployment; five fresh 1280x800 screenshots were saved.
- `git diff 58c1f3a -- client common server scripts assets Cargo.toml Cargo.lock`:
  empty, confirming no runtime or test changes during conflict resolution.

The native run checks the normal raid route; it does not replace the outstanding
artistic, detailed framing or independent acceptance gates.
