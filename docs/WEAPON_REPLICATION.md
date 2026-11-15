# Weapon configuration replication

Protocol 11 integrates `common::replication` with both endpoints. Rebuild client
and server together. Weapon configuration uses reliable ordered channel 3 in
each direction, with a 2 MiB queue and 100 ms retransmission. Existing continuous
input/connectivity channels remain independent.

Shots additionally use server-to-client reliable ordered channel 4 (2 MiB,
100 ms retransmission). This channel does not block configuration or input.

## Identity and bounded publication

An immutable `(epoch: u64, handle: u32)` identifies a configuration, including
owner, equipment slot, physical weapon instance, `u64` revision and the bounded
attachment tree. It contains IDs and placements, never client-supplied weapon
statistics or transforms. The receiver resolves against its own catalog.

The catalog fingerprint is FNV-1a over the exact bundled RON bytes. It detects
content mismatch, including changes to definitions not included in a packet;
it is not an authentication hash. A content change requires matching binaries.

Configuration/command bytes are limited to 8,192 before aligned archive decoding.
Trees have 1–32 nodes, nonzero node/instance IDs and 1–64-byte socket names.
The catalog resolver additionally checks depth (8), uniqueness, parent links,
interfaces, dependencies, occupied space and all derived properties.

The publisher reuses the handle for unchanged instance/revision/ownership.
The receiver commits the tree and resolved properties atomically. A repeated
identical packet is a no-op; a reused handle or inconsistent instance/revision
is rejected. Old revisions remain individually addressable for in-flight
snapshots, so arrival order cannot replace a newer tree under its handle.
Unknown references return no configuration and allocate no placeholders.

Both sides retain at most 1,024 configurations per epoch. Exhaustion is an
explicit error, never eviction of possibly referenced state. The publisher must
advance the epoch and bootstrap clients before issuing references in that epoch.
Duplicate epoch announcements preserve valid entries; older epochs are rejected.
Epoch exhaustion also fails without clearing state. Reconnect creates a fresh
client cache and connection-specific command ledger.

The server registers both slots of each actor, including bots, and advances a
per-peer reliable publication cursor. A late join receives the bounded history;
unchanged configurations are not sent again. Queue pressure postpones unsent
entries. Raid changes and returning to the lobby advance the epoch. Cache
saturation starts a new epoch and republishes the current equipment set.

Snapshots carry one epoch and a four-byte handle per actor. Grouping scalar
fields/flags removes padding, so the handle does not increase the actor's fixed
archive size. The receive boundary validates owner, slot, instance and definition
against the cache. It holds only the latest snapshot until all referenced
configurations arrive. Configuration arrival alone can unblock it; no subsequent
snapshot is required. Until then, presentation retains the last accepted world
(subject to the existing stale timeout), and continuous inputs keep flowing.
The resolved cache is available for the modular visual integration in TAR-23.

## Authoritative commands

A configuration command includes epoch, monotonically increasing request ID,
slot, instance, expected revision and the replacement tree. The authenticated
connection selects the Arsenal; no arbitrary target player is supplied.
The caller supplies the authoritative lobby/ready lock. The Arsenal enforces
manipulation locks, compatibility and ownership of physical magazines.

One constant-size ledger per connection retains the latest request/result.
Identical duplicates return that result even after a lock changes; conflicting
reuse is rejected. Earlier IDs are stale and never execute. Failed commands also
consume their ID, so they cannot become valid later by replaying the same packet.
This contract requires ordered reliable command delivery. Epoch mismatch does
not advance the request high-water mark. IDs never wrap.

## Current verification

The common fixtures cover roundtrip and resolution for M4A1/M1911/MP5,
unaligned transport slices, malformed/oversized archives, exact structural
bounds, invalid trees/catalogs, reordered revisions, duplicate/reset behavior,
bounded cache exhaustion, CAS, forged ownership, locked commands and conserved
physical state. Native receive tests reconstruct an interrupted reload when its
configuration arrives after the final snapshot. A production ECS/Renet fixture
exercises publication, duplicate commands, independent continuous input, packet
loss, late join and epoch reset. These are headless in-memory transports; real
UDP acceptance is an explicit socket-opening gate:

```sh
cargo test --locked -p server replication_udp_solo_dual_loss_and_late_join -- --ignored --nocapture
```

It uses production ECS systems, Renet, Netcode and native kernel UDP on ephemeral
loopback ports. Manual 128 Hz ticks advance simulation and transport clocks
without sleeps. Each run starts solo, configures a weapon, sends duplicated CAS
commands and ordinary fire inputs, then joins a second client during a tactical
reload. Both clients reconstruct the magazine-removal commit. A forged ownership
command is rejected. The matrix runs without loss and with every fifth outgoing
and incoming client datagram discarded before Netcode (20% in each direction).
This gate passed for M4A1 and MP5 with both primary and M1911 configurations.
It is opt-in to keep default unit checks socket-free, and is required for this
protocol's delivery. It proves transport/state convergence, not rendered quality.

The existing stress fixture (32 actors, 320 pieces, 256 impacts, 224 shots) now
measures 54,376 bytes, or 59,816 bytes with all 32 physical rigs, below 60,000.
Each actor's fixed archive is 544 bytes. The current one-component M4A1/M1911/MP5
configuration payload is 96 bytes and the CAS command is 80 bytes, before the
server-message envelope and Renet/UDP framing.

## Shot budget, overflow and replay

Every newly confirmed shot is queued on the reliable stream in batches of at
most 128; recent shots also ride the 30 Hz snapshot for low latency. At first
publication to a connection, a reliable bootstrap sets the first valid shot ID,
excluding earlier history. IDs remain monotonic across raids/configuration
epochs; exhausting `u64` refuses firing before consuming ammunition.

`pack_snapshot` preserves all mandatory state and returns displaced shot records
explicitly until the encoded snapshot is **strictly below 60,000 bytes**.
The server queues the reliable copies before packing and diagnoses overflow.
Saturated reliable queues disconnect the affected peer with the first unsent ID
in the log; events are never silently skipped. If mandatory state alone exceeds
the budget, publication fails with its measured size and disconnects clients
rather than dropping physical state.

Client deduplication uses an 8,192-ID sliding set, not just a high-water mark.
Thus a newer snapshot shot does not suppress a delayed reliable shot. The casing
consumer uses the same gap-aware rule; recoil remains coalesced to the newest
shot per actor/frame. Duplicate/bootstrap repeats do not clear history. IDs below
the late-join baseline, previous-raid events and effects older than 150 ms are
consumed without playback. Reliable batches wait for the appropriate raid/clock.
A pending receive cap of 8,192 events fails explicitly instead of dropping data.
Snapshot arrival time is retained while waiting for configuration, so a delayed
configuration cannot make old effects appear fresh.

The production Renet overflow fixture delivers all 2,000 events, verifies the
snapshot bound, proves no historical late-join replay, and checks queue-pressure
disconnection. Projection fixtures keep 32 actors, all physical rigs, 320 pieces
and 256 impacts while increasing shot cadence to the 128 Hz simulation ceiling.
The snapshot/overflow union must contain every ID exactly once.

Measured projections with that full physical state (half-second shot history):

| RPM per actor | Shots | Raw bytes | Snapshot bytes | Reliable-only shots |
| --- | --- | --- | --- | --- |
| 900 | 240 | 60,584 | 59,960 | 13 |
| 1,200 | 320 | 64,424 | 59,960 | 93 |
| 2,400 | 640 | 79,784 | 59,960 | 413 |
| 7,680 | 2,048 | 147,368 | 59,960 | 1,821 |

A 128-shot reliable batch measures 6,168 bytes before transport framing.
