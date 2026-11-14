# Physical humanoids specification

## Problem Statement
The extraction alpha uses rigid bodies and broad capsule damage. Implement the entire user specification at `C:/Users/sandr/Documents/ChatGPT/TK/goal-tk-especificacao.txt`, read in full on 2026-11-14. That document remains the acceptance authority; this index does not narrow it.

## Goals
- Articulated voxel actors and weapon presentation in the normal raid.
- Server-authoritative anatomy, balance, wounds, ragdolls and separation.
- Reproducible evidence for all seven acceptance criteria.

## Out of Scope
| Feature | Reason |
| --- | --- |
| Stack upgrades, copied FRUKT assets/code | Explicitly prohibited |
| Deployment or merge | Not requested; later authorization covers only commits and push to feature/animacoes-mitologicas |

## Assumptions & Open Questions
| Assumption / decision | Chosen default | Rationale | Confirmed? |
| --- | --- | --- | --- |
| Art | Original procedural voxel layers | Shared reusable geometry without external assets | User permits procedural |
| Physics | KCC living control plus articulated Rapier bodies | Preserve FPS response and server authority | User permits hybrid |
| Legacy health | Retain four aggregate pools as compatibility telemetry, add seven regional pools | Preserve existing tests while replacing live hit classification | Implementation assumption |
| Tuning | Gameplay parameters in shared Rust constants | Reproducible, no new config dependency | Implementation assumption |
| Commits | English Conventional Commits on feature/animacoes-mitologicas, followed by branch push | Explicit user request after acceptance; preserve unrelated/concurrent work | Confirmed |

**Open questions:** none; tuning defaults are explicit implementation assumptions.

## User Stories
### P1: Complete physical humanoid raid
1. The renderer SHALL show articulated voxel head, neck, torso, pelvis, upper/lower limbs, hands and feet, armor and distinct sentry equipment (TK-01).
2. WHEN normal movement input changes THEN the rig SHALL animate idle, directional locomotion, crouch, jump, landing, balance, fall and recovery using authoritative velocity and support (TK-02).
3. WHEN a shot is authorized THEN the client SHALL animate the correct shooter exactly once without creating audio entities (TK-03).
4. WHEN reload or weapon selection changes THEN hands and weapon SHALL follow Arsenal state, including interruptions and arm impairment (TK-04).
5. WHEN a projectile crosses anatomy THEN the server SHALL resolve ordered posed segment and internal intersections, one armor evaluation and pass-through gaps (TK-05).
6. WHEN regional muscles or organs are damaged THEN the server SHALL apply distinct lateral support, handling, brain, heart, liver and stomach consequences (TK-06).
7. WHEN support or impulse exceeds balance THEN the server SHALL decide correction, fall, recovery and death transfer to limited articulated physics (TK-07).
8. WHEN a segment separates THEN both clients SHALL observe one physical fragment, absent connected volume and an exposed wound (TK-08).
9. IF snapshots are lost THEN subsequent state SHALL converge pose, wounds, fragments and bodies without duplicate shot impulses (TK-09).
10. WHEN a new raid begins THEN anatomy and physics SHALL reset while extraction, debrief and loadout flow remain playable (TK-10).
11. The validation SHALL run fmt, workspace check/tests, normal playtest and two rendered UDP clients with new logs, captures and performance comparisons under target/playtest (TK-11).
12. The delivery SHALL document controls, tuning, visual replacement and acceptance evidence in the game guide and implementation evidence report (TK-12).

## Edge Cases
- IF the magazine is empty or firing is blocked THEN the client SHALL not flash or recoil.
- IF a segment is already absent THEN it SHALL not take connected damage or separate twice.
- IF armor stops a shot THEN internal organs SHALL remain intact.
- IF a ray crosses a limb gap THEN it SHALL continue to the next collision.
- IF recovery lacks support or clearance THEN the actor SHALL remain down.

## Requirement Traceability
| Requirement ID | Story | Phase | Status |
| --- | --- | --- | --- |
| TK-01 | P1 | Complete | Verified — acceptance-11 and independent validation.md |
| TK-02 | P1 | Complete | Verified — acceptance-11 and independent validation.md |
| TK-03 | P1 | Complete | Verified — acceptance-11 and independent validation.md |
| TK-04 | P1 | Complete | Verified — acceptance-11 and independent validation.md |
| TK-05 | P1 | Complete | Verified — acceptance-11 and independent validation.md |
| TK-06 | P1 | Complete | Verified — acceptance-11 and independent validation.md |
| TK-07 | P1 | Complete | Verified — acceptance-11 and independent validation.md |
| TK-08 | P1 | Complete | Verified — acceptance-11 and independent validation.md |
| TK-09 | P1 | Complete | Verified — acceptance-11 and independent validation.md |
| TK-10 | P1 | Complete | Verified — native second deploy and separated-anatomy cleanup regression |
| TK-11 | P1 | Complete | Verified — 64 tests, seven gates, performance comparison, independent rerun |
| TK-12 | P1 | Complete | Verified — guide/Mermaid/evidence index, independent PASS and completion gate |

## Success Criteria
All seven original acceptance criteria have fresh reproducible evidence. Missing evidence remains pending, not inferred from compilation.
