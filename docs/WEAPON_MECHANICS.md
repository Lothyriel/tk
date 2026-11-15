# Closed-bolt mechanical state

`MechanicalState` in `common/src/catalog/mechanics.rs` models retained
magazines, a separate chamber, bolt state, selector and manipulation commits.
`WeaponInstance` owns this state; `Arsenal` handles inputs and cadence, and the
server feeds the same action clock to snapshots and pose presentation.

## Identity and conservation

Four magazines have durable, nonzero `MagazineInstanceId`s supplied by the
owning simulation. The runtime allocator never wraps; restoring instances raises
its high-water mark before creating more magazines. Persisted worlds must restore
all instances before spawning new ones. Each stores definition, ammo, capacity and rounds. Restored
state must pass `validate` against the owning registry/resolved weapon before
simulation: duplicate/unknown IDs, incompatible interfaces/platform/caliber,
capacity overflow, malformed phases/clocks and inconsistent locations fail.

The standalone weapon retains all four objects. `inserted` references one ID;
every other ID is in its reserve. Removal changes this reference; it does not
clone an object or destroy its remaining rounds. Later inventory integration
changes the location adapter of these same IDs.

Initial ammunition is four magazine capacities. One round moves from the
inserted magazine into the chamber, so initial loaded count equals capacity,
not capacity + 1. A tactical reload can produce capacity + 1 only by retaining
that real chamber round while inserting a full reserve magazine.

`chamber + sum(all magazine rounds) + fired rounds` is conserved. There is no
loose-ammo top-up or emergency drop in this core.

## Actions and commits

Each accepted manipulation increments a per-weapon `u64` action ID; overflow
rejects a new action. Kind, phase, elapsed mechanical seconds, total duration
and target magazine ID are persistent. Durations must be finite, 0.01–60 s.
`advance` receives authoritative mechanical time (simulation delta multiplied
by handling rate); presentation must use the same clock rather than raw wall
time or ammunition-based phase guesses.

Reload selection chooses the compatible retained non-inserted magazine with
most rounds, breaking ties by smallest instance ID. Tactical reload does
nothing if it cannot improve the seated magazine. An interrupted empty reload
with a populated magazine already inserted resumes feeding.

| Phase | Normalized interval | Commit at phase end |
| --- | --- | --- |
| Remove | 0–0.25 | clear inserted reference; retain old magazine |
| Insert | 0.25–0.70 | seat selected magazine ID |
| Feed (empty only) | 0.70–0.90 | move one real round to chamber and close bolt |
| Finish, empty | 0.90–1 | permit subsequent input |
| Finish, tactical | 0.70–1 | permit subsequent input without cycling chamber |

Interrupted removal/insertion/feed never rolls back committed state. Resuming
without a seated magazine begins at Insert. Cancelling after Feed leaves the
chamber populated; cancelling before Feed leaves it empty. A single large
simulation step executes every crossed commit in order. Clock comparisons use
the accumulated time to avoid storing a phase exactly at its uncommitted end.

Equip and Unequip commit the equipped flag only at their end. Fire is blocked
through these phases. Unequip replaces reload/inspect without undoing their
commits. Inspect changes no ammunition, cannot start during another action,
and yields to fire, reload or unequip. Fire does not interrupt reload.

## Selector and firing

Automatic profiles cycle Auto → Safe → Semi → Auto. Semi-only profiles cycle
Semi → Safe → Semi. Safe never fires; Semi consumes a fresh trigger edge; Auto
uses held trigger. The outer Arsenal owns edge deduplication and cadence.
Sequence comparisons reject duplicates and reordered edges with wrapping `u32`
serial arithmetic. Edges received during blocked actions are consumed, not queued
for an unexpected later shot.

A confirmed shot consumes the chamber round, then feeds from the inserted
magazine if available. Otherwise the bolt remains open and a subsequent
trigger cannot invent a chamber round. This core intentionally models only the
existing closed-bolt families; new mechanisms require their own concrete slice.

## Gameplay integration

Switching starts Unequip (0.2 mechanical seconds), changes the active slot at
its commit, then starts Equip (0.35 seconds). A fresh semi trigger is required
after equip; held automatic fire resumes only after equip completes. Cancellation
preserves both weapons' pools and the committed equip flags.

`I` requests Inspect (1.5 seconds); `B` cycles the selector. The HUD displays
the selector, chamber and current action/phase. Inspect's detailed pose layer
belongs to the handling animation integration.

Server gates cancel actions on death, fall/recovery, absent trigger-arm capacity
and input older than 0.5 s. Raid reset replaces the equipment. Handling scales
mechanical time by `0.45 + 0.55 * anatomy.handling()`, so commits and the replicated
fraction slow together. `reload_remaining` remains a derived compatibility field,
never a source of commits.

Protocol 10 carries a compact `MechanicalSnapshot`: last action ID, explicit kind/
phase, normalized fraction, remaining mechanical time, chamber/bolt/magazine,
selector and equipped state. It omits the physical pool and attachment tree.
Local/remote reload presentation reads the explicit variant and fraction instead
of inferring empty reload from loaded rounds. Removed magazines stay absent after
interruption. Both binaries must be rebuilt together.

## Evidence boundary

Tests cover all three profiles, interruption immediately before/after commits,
retained magazine identity, deterministic selection, safe/semi/auto, inspect/
equip priority, empty pools, serialized intermediate states, clock partitioning
and 10,000 generated operations with independent conservation checks.
Arsenal tests cover timed switching, cancelled stages, duplicate/reordered inputs
and retained pools. ECS/Rapier fixtures cover cancellation around magazine commits
and shared injury timing. Native drivers and real UDP/rendered evidence are
separate from these headless checks.
