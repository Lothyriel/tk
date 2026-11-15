# Blood effects

## Problem Statement
Replicated tissue wounds, bleeding and dismemberment need visible blood feedback for the player, other players and sentries.

## Out of Scope
Damage balance, injury rules, protocol changes, healing, wall decals and fluid simulation.

## Assumptions & Open Questions
| Assumption | Chosen default | Rationale |
| --- | --- | --- |
| Source | Existing authoritative trauma/wounds/missing/blood/bleed rate | Effects must not grant or invent damage |
| Presentation | Red voxel droplets and floor splashes | Matches procedural art without new assets |
| Bounds | 512 droplets lasting up to 3 seconds; 256 stains lasting 60 seconds | Bounded rendering cost |
| Detached limbs | Drip for 6 seconds after newly observed separation | Finite cosmetic blood without changing anatomy |
Open questions: none.

## User Stories
As a player I see wounds spray, severed limbs bleed, and bleeding actors leave dripping trails.

Acceptance Criteria:
1. When a known actor gains tissue trauma or a new wound, the client SHALL emit an eight-droplet burst at the affected posed segment; newly missing limb roots SHALL emit 24 droplets at the stump; unchanged snapshots and initial late-arrival state SHALL not replay bursts (BL-01).
2. While an active living actor has positive bleed rate and blood and fresh snapshots, the client SHALL drip from wounds at a bounded 2–12 droplets per second for local players, remote players and sentries; healthy, dead or stale actors SHALL not emit ongoing actor drips (BL-02).
3. When a newly detached piece is observed, it SHALL drip at four droplets per second for six seconds at its replicated position, without replaying a spurt for an existing late-arrival piece (BL-03).
4. When droplets fall to the yard floor, the client SHALL replace them with red stains; active droplets SHALL remain at most 512 and stains at most 256; droplets SHALL expire after three seconds and stains after 60 seconds; raid changes or lobby return SHALL clear effects and histories (BL-04).

## Requirement Traceability
| ID | Implementation | Evidence | Status |
| --- | --- | --- | --- |
| BL-01 | client/src/render/blood.rs | client/src/render/blood.rs:341; client/src/render/blood.rs:390; validation.md | verified |
| BL-02 | client/src/render/blood.rs | client/src/render/blood.rs:414; validation.md | verified |
| BL-03 | client/src/render/blood.rs | client/src/render/blood.rs:441; validation.md | verified |
| BL-04 | client/src/render/blood.rs | client/src/render/blood.rs:470; client/src/render/blood.rs:525; validation.md | verified |
