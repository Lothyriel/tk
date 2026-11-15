# Weapon configuration replication

`common::replication` defines the configuration contract. Transport integration
is in progress; the running game still uses protocol 10 until both endpoints
are migrated together.

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
physical state. These are contract-level tests; real endpoint/UDP acceptance and
the final snapshot/shot overflow measurements remain part of TAR-17 integration.
