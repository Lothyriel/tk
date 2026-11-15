# Voxel barriers

## Problem Statement
Interior cover cannot currently be broken. Shots will remove individual solid cells and open persistent holes in the authoritative world.

## Out of Scope
Terrain editing, structural collapse, dynamic rubble, loot crate destruction, perimeter/floor damage and persistence across server sessions.

## Assumptions & Open Questions
| Assumption | Chosen default | Rationale |
| --- | --- | --- |
| Targets | Map blocks 5 through 8 | Interior wood panels and concrete cover; preserve arena and objectives |
| Resolution | Cells at most 0.5 m on each axis, tiled exactly | Bounded collider and snapshot costs |
| Durability | Wood 20 HP, concrete 100 HP; projectile damage subtracted once per cell impact | Wood breaks readily; concrete needs sustained fire |
| Lifetime | Restore HP and solidity on raid start or return to lobby | Each raid starts with fresh cover |
Open questions: none. Defaults are local implementation choices for this first destructible-cover delivery.

## User Stories
As a player, I can shoot holes in interior cover and shoot or move through cleared space.

Acceptance Criteria:
1. When interior cover is spawned, the system SHALL tile its original bounds with deterministic voxel IDs and retain solid arena walls, floor and loot crates (VX-01).
2. When a projectile hits a living voxel, the server SHALL subtract damage once, remove its collider at zero HP, preserve neighbors, and ignore destroyed cells for later shots even in the same tick; concrete SHALL stop the destroying shot and wood SHALL retain existing thickness-based penetration (VX-02).
3. When a snapshot is received, the client SHALL apply the complete destroyed-cell bitmask (22 bytes for 176 cells), hiding removed cells and restoring them when absent; every snapshot SHALL include this state for late arrival and packet-loss recovery, with protocol 8 (VX-03).
4. When a raid starts or returns to lobby, the server SHALL restore all voxel HP and colliders and clear the destroyed-ID list (VX-04).

## Requirement Traceability
| ID | Implementation | Evidence | Status |
| --- | --- | --- | --- |
| VX-01 | Shared map cells, server colliders, client meshes | common/src/map.rs:144; validation.md | verified |
| VX-02 | Authoritative projectile damage | server/src/tick/tests.rs:2084; validation.md | verified |
| VX-03 | Full snapshots and client visibility | server/src/tick/tests.rs:2253; client/src/sync/mod.rs:422; validation.md | verified |
| VX-04 | Raid lifecycle reset | server/src/tick/tests.rs:2084; validation.md | verified |
