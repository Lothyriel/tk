# Physical cover destruction

## Problem Statement
Shots currently delete half-metre cover cells and leave impact quads suspended in the cleared space. Cover needs localized bullet damage and unsupported sections that fall under gravity.

## Out of Scope
Destruction of the arena floor/perimeter or loot crates, explosions, fluid/dust simulation, imported art, player damage from rubble and movement prediction. Concurrent snapshot-rate work is preserved separately.

## Assumptions & Open Questions
| Assumption | Chosen default | Rationale |
| --- | --- | --- |
| Bullet openings | Rifle 6 mm, SMG 9 mm, pistol 11 mm nominal bore, with a small surrounding chip | Game-scale bore sizes instead of half-metre openings |
| Material | Wood perforates; concrete chips and stops the impacting round | Preserve the existing penetration distinction |
| Structural strength | Wood cells 160 HP; concrete cells retain 100 HP | A single wood bullet must not delete its entire structural section |
| Support | Face-connected cells connected to the floor remain supported | Deterministic coarse fracture structure with actual dynamic rubble |
| Rubble | At most 64 chunks, 20 s lifetime, real server Rapier gravity/contact; no player damage | Bound physics/network cost and avoid controller/ragdoll instability |
| Sparse damage | At most 128 cuts globally and 96 remaining boxes per cell; saturating a cell detaches it rather than restoring damage | Small geometry/replication budget |
| Networking | Separate recoverable full destruction state, unchanged full actor snapshot budget | Existing physical actor snapshot already reaches 59,808 bytes |
| Test migration | Replace obsolete coarse wood deletion/protocol expectations; preserve their other assertions | User explicitly requests smaller holes and physical breakup |
Open questions: none. Art and visible motion require a later rendered review.

## User Stories
As a player I can chip and shoot through cover at the impact point, cut away its supports, and see unsupported chunks fall and settle without floating bullet marks.

Acceptance Criteria:
1. When a destructible cell is removed or detached, the server SHALL remove every mark anchored to it while preserving marks on other surfaces; raid reset SHALL clear marks and anchors (DC-01).
2. When a normal round penetrates wood, its first opening SHALL be centered at the hit and have the specified nominal bore width instead of deleting the structural cell; a subsequent ray through the bore SHALL pass while a ray 3 cm beside it SHALL still hit solid wood; ammunition and wood penetration cost SHALL remain authoritative (DC-02).
3. When concrete is hit below its structural failure threshold, it SHALL retain solid material behind a localized surface chip and stop that round; structural failure SHALL remove its static collider without charging penetration or hitting a target behind it on the same shot (DC-03).
4. When a cell fails or its face-connected path to floor support is removed, the server SHALL detach it exactly once, remove its static obstruction and create real dynamic rubble; supported neighbors SHALL remain static, rubble SHALL fall, contact the floor without escape, settle with bounded late drift, and never become character-controller support (DC-04).
5. While destruction is replicated, clients SHALL reconstruct the same surviving cut geometry, removed cells and rubble IDs/poses after loss or late arrival; obsolete states SHALL not restore old geometry; no destruction message SHALL exceed 16,000 bytes and the existing maximum actor snapshot SHALL remain below 60,000 bytes (DC-05).
6. When a raid resets, cuts, rubble and destruction histories SHALL clear and the original cover geometry/colliders SHALL return; active rubble SHALL never exceed 64 and SHALL expire after 20 seconds (DC-06).

## Requirement Traceability
| ID | Implementation | Evidence | Status |
| --- | --- | --- | --- |
| DC-01 | server impact anchors and cover lifecycle | server/src/tick/tests/destruction.rs; T1 quick gate | implementing |
| DC-02 | shared cut geometry, server damage, client meshes | tasks.md T2 evidence; engine/mesh regressions | implementing |
| DC-03 | material damage and collision | tasks.md T2 evidence; concrete chip/backstop regression | implementing |
| DC-04 | server support graph and Rapier rubble | Pending | pending |
| DC-05 | shared destruction protocol and client sync | tasks.md T2 evidence; wire/budget/receive and loopback checks; rubble producer pending | implementing |
| DC-06 | raid reset and bounded lifetimes | Pending | pending |
