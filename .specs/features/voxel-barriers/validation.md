# Voxel barriers validation

## Validation: PASS (scoped feature acceptance)

Date: 2026-11-15. Verifier: independent `/root/verify_voxels`, separate from the implementation author. Diff: uncommitted voxel-barrier changes against `b845547`, limited to the map, wire state, damage, reset, client presentation and their tests. No commit or publishing authority was inferred.

All four feature acceptance criteria are covered by passing headless tests and implementation inspection. This scoped verdict does not certify the full workspace gate, rendered appearance, real UDP delivery or artistic acceptance.

## Spec-anchored evidence

| Requirement | Expected outcome | Assertion / implementation evidence | Verdict |
| --- | --- | --- | --- |
| VX-01 | Exactly 176 deterministic cells tile interior blocks with maximum 0.5 m edges; arena/objectives remain solid | `common/src/map.rs:146` asserts `cells.len() == BARRIER_CELL_COUNT`; `common/src/map.rs:152` asserts sequential IDs; `common/src/map.rs:154` asserts half sizes in `(0, 0.25]`; `common/src/map.rs:172` asserts each interior block's cell volume equals its original volume and exterior blocks contain no cells. `server/src/tick/mod.rs:138` retains non-destructible block colliders and `server/src/tick/mod.rs:148` retains loot colliders. Existing real-Rapier floor/wall tests remain passing in the independent quick run. | Covered |
| VX-02 | 40-damage concrete hits produce 60, 20, then 0 HP; neighbors stay at 100; even destroying concrete stops the round | `server/src/tick/tests.rs:2092` defines the expected HP sequence; `server/src/tick/tests.rs:2095` asserts actual health equals it; `server/src/tick/tests.rs:2099` asserts neighbor health 100; `server/src/tick/tests.rs:2103` asserts zero surviving projectiles; `server/src/tick/tests.rs:2112` asserts the collider disabled component. | Covered |
| VX-02 | Removed cells are skipped immediately, wood penetration costs 4.8 energy once, and clearing a panel permits movement | `server/src/tick/tests.rs:2191` asserts first round energy 25.2, `server/src/tick/tests.rs:2195` asserts second same-tick round energy 30; `server/src/tick/tests.rs:2200` asserts exactly one destroyed ID. `server/src/tick/tests.rs:2214` asserts solid-panel blockage and `server/src/tick/tests.rs:2224` asserts character Z passes below -5.5 after clearing all 32 panel cells. `server/src/tick/tests.rs:2240` asserts one normal weapon input spends one round; `server/src/tick/tests.rs:2245` asserts one cell removed. | Covered |
| VX-03 | Every production snapshot carries complete state for a late peer even after a lost snapshot; protocol 8 | `server/src/tick/tests.rs:2284` asserts decoded production snapshot destroyed IDs equal `[cell.id]` after late connection and discarding outgoing packets; `server/src/tick/tests.rs:2288` asserts protocol 8. This test exercises the production snapshot system and real Renet packet encoding in memory. | Covered |
| VX-03 | Actual receive replaces complete state, rejects stale state, and presentation hides/restores individual cells | `client/src/sync/mod.rs:463` asserts session IDs equal `[2,7]` after sequence 3, remain `[2,7]` after stale sequence 2, and clear on sequence 4. `client/src/render/mod.rs:954` asserts removed cell Hidden; `client/src/render/mod.rs:958` asserts neighbor Inherited; `client/src/render/mod.rs:975` and `client/src/render/mod.rs:979` assert both restored after reset. | Covered |
| VX-03 | Packed state stays inside the existing snapshot budget | `common/src/map.rs:147` asserts mask roundtrip for first/intermediate/last IDs; `common/src/data.rs:102` asserts decoded wire IDs; `common/src/data.rs:160` preserves the original `packed.len() < 60_000` physical snapshot assertion with all destruction bits set. | Covered |
| VX-04 | Both lobby return and fresh raid start restore HP/colliders and clear removed IDs | `server/src/tick/tests.rs:2128` asserts Lobby; `server/src/tick/tests.rs:2129` asserts empty IDs, `server/src/tick/tests.rs:2135` and `server/src/tick/tests.rs:2139` assert both cells 100 HP; `server/src/tick/tests.rs:2143` asserts collider restored. After independently injected lobby damage, `server/src/tick/tests.rs:2157` asserts Active, `server/src/tick/tests.rs:2158` asserts 100 HP, `server/src/tick/tests.rs:2162` asserts collider enabled and `server/src/tick/tests.rs:2163` asserts empty IDs. | Covered |

Tests map to the four specified requirements; no existing assertions were weakened, disabled or deleted. Shared protocol types remain in common, authority in server, and visibility in client. Client rendering reuses meshes by cell dimensions and shared materials. No terrain editor, dynamic rubble or unrelated gameplay was introduced.

## Discrimination sensor

Two behavior mutations were run only in an isolated source copy at `/tmp/tk-voxel-verify-uajxyduc`, with the shared build cache. Both compiled successfully and were killed by assertion failures, not compiler errors.

| Mutation | Focused check | Outcome |
| --- | --- | --- |
| `server/src/tick/simulation.rs:283`: change destroyed-cell predicate `health <= 0.0` to `health < 0.0` | `python3 scripts/verify.py --package server --filter voxel_barriers_wood_penetration_same_tick_holes_and_movement` | Killed, exit 101. Second same-tick round incorrectly loses energy instead of crossing the hole freely. |
| `server/src/tick/mod.rs:381`: replace snapshot's actual destroyed mask with an empty mask | `python3 scripts/verify.py --package server --filter voxel_barriers_production_snapshots_recover_loss_and_late_join` | Killed, exit 101. `server/src/tick/tests.rs:2284` received `[]` instead of `[72]`. |

Sensor logs: `/tmp/tk-voxel-verifier-mutant1.log` and `/tmp/tk-voxel-verifier-mutant2.log`. No survivors. Scratch removed after testing. The final real-tree porcelain matched the captured baseline exactly, and SHA-256 checks of all production/test Rust sources also matched. Authorized author changes during the earlier snapshot-budget repair preceded this final isolation baseline; the sensor did not modify the real tree.

## Executed checks and limitations

- Independent `python3 scripts/verify.py --feature voxel-barriers` in the refreshed scratch: 20 tooling tests, spec validator with zero warnings/errors, 44 common and 49 server tests passed. Log: `/tmp/tk-voxel-verifier-final-quick.log`.
- Author's final workspace test run passed 39 client and 44 common tests; the server run passed 48 and failed the unchanged timing-budget test under overlapping verification load. Its measured p95 was 15.043 ms against 7.8 ms. That run remains recorded as unsuccessful in `/tmp/tk-voxel-tests-final.log`; it is not relabeled.
- The same server timing case passed in the independent 49-test quick run and subsequently passed alone through the author-run verification entry point, without changing its assertion, in `/tmp/tk-voxel-perf-recheck.log`. Overlapping CPU load is the likely explanation, an inference from the differing run conditions. Ordinary rendered performance was not measured.
- The full gate passed formatting and workspace check, then stopped at the existing `common/src/combat/anatomy.rs:194` `clippy::too_many_arguments` diagnostic. The unchanged nine-argument function was confirmed at baseline `b845547`. Full-gate workspace tests were not reached. Log: `/tmp/tk-voxel-full.log`. This report does not waive that lint or report full as passed.
- An intermediate all-destroyed-ID snapshot exceeded the original byte budget. The author corrected it to a 22-byte bitmask; the original threshold remains intact and the independent common suite passes.
- No game window, GPU, listening network socket or video recorder was opened. Renet tests use packet buffers in memory; real UDP delivery, visible hole appearance and artistic acceptance remain unverified.

There are no remaining gaps in the four scoped headless feature criteria. Full workspace certification remains limited by the existing lint, and visual/real-network acceptance remains separate.

## Final combined serial verification

After the blood-effects review, the author forced recompilation from unchanged real client/server source to avoid shared Cargo mutation artifact reuse and ran workspace tests serially without a competing verifier suite. All 138 tests passed: 45 client, 44 common, 49 server, zero ignored. Log: `/tmp/tk-combined-tests-final.log`. This includes the unchanged physics timing-budget test. The earlier overlapped timing failure remains recorded above. The combined full gate again passed formatting/workspace check and stopped at the same baseline anatomy Clippy diagnostic (`/tmp/tk-combined-full.log`); it remains unpassed.
