# Animation and ragdoll Specification

## Problem Statement

The playable voxel raid has generic weapon shapes and one reload arc. Living falls use a fixed rotation and corpses discard articulated momentum. The external approved plan is `C:/Users/sandr/Documents/ChatGPT/TK/plano-tk-animacoes-ragdoll.md` (read in full).

## Goals

- Demonstrate original M1911, MP5 and M4A4 presentation in a continuous native raid, in both perspectives.
- Transfer pose and momentum to stable articulated bodies; prove environment-dependent falls and recovery.
- Preserve combat authority and the full UDP extraction loop.

## Out of Scope

| Feature | Reason |
| --- | --- |
| Visible blood and fluids | Explicitly deferred by user |
| Ammo, damage, cadence and equipment changes | Preserve existing gameplay, including the alpha pistol's 17 rounds |
| Imported reference assets | References inform timing and silhouette only; original voxel art |
| Publication outside feature/animacoes-mitologicas, merge, deploy, release | AD-012 authorizes English Conventional Commits and push only to the existing feature branch |

## Assumptions & Open Questions

| Assumption / decision | Chosen default | Rationale | Confirmed? |
| --- | --- | --- | --- |
| Near-AAA is qualitative | Native real-time and frame-by-frame visual review, not a test-count claim | Artistic quality cannot be certified by unit tests |
| Pistol capacity differs from reference M1911 | Keep 17 rounds and 1.5 s authoritative reload | User requires preserving ammo rules |
| Baseline instrumentation | Add only native framebuffer recording before presentation changes | Existing harness had screenshots, no continuous video |
| Validation execution | Inline implementation; fresh independent verifier at completion | User asks to start without next-step questions |
| Snapshot format | Bump protocol if shared state grows; rebuild both binaries | Never silently decode incompatible state |
| Contact tolerances | Trigger <1 mm, support <2 mm, planted contact drift <3 mm per fixed tick | Concrete regression bounds for the requested contact quality |
| Physical rest | By 8 s, segment linear speed <0.15 m/s and angular speed <1 rad/s, no floor escape below y=0.065 and no adjacent centres >0.75 m | Detect persistent jitter, solver launch and disconnected limbs in the yard-scale matrix |
| User visual review, 2026-11-14 | Models accepted as a first delivery; weapon movement/reload/FOV rejected for further polish | Preserve candidate10 evidence without calling artistic acceptance complete |
| ADS input | Hold right mouse; smoothly enter/leave iron sights, cancel for reload/sprint/fall/death and released cursor | Standard FPS default for the user's explicit ADS request |
| Camera framing | Default 70-degree vertical world FOV, stable 65-degree weapon projection, eased lens-space ADS zoom | Replace the previous wide 90-degree vertical default and frame-rate-dependent FOV adjustment |
| Point-blank repair | User explicitly authorized combat-origin repair after candidate13, not a stand-off fixture workaround | Check real posed anatomy and cover before muzzle launch; coarse actor capsules must not redirect a valid offset shot |
| Recovery native checkpoint | User approved actual supported completion bounded by18.2s (AD-011), keeping Standing/IDs/1sget-up/physical gates/fatal-shot deadline | Recorded candidate15 settled naturally0.695s later and stood17.04s, after the old16.8s checkpoint; do not force early recovery |

**Open questions:** none; chosen defaults are recorded above.

## User Stories

### P1: Continuous weapon handling

**User Story**: As a player, I want recognizable, weighted weapons whose hands stay in contact through actions.

**Acceptance Criteria**:

1. The client SHALL render distinct original M1911, MP5 and M4A4 voxel silhouettes, with matching first/third-person parts and grip/muzzle anchors. (ANIM-01)
2. WHEN a reload is confirmed THEN the presentation SHALL distinguish retained-ammo from empty reload and follow the authoritative remaining time. (ANIM-02)
3. WHEN draw, stop, turn, fire, reload interruption or swap occurs THEN the presentation SHALL blend from its current pose without changing server ammo/cadence/validity. (ANIM-03)
4. WHEN a confirmed shot is displayed THEN the flash SHALL use the animated visual muzzle; a rejected or duplicate trigger SHALL NOT replay feedback. (ANIM-04)
5. WHILE the right mouse button is held during valid active control THEN the client SHALL smoothly align the current weapon's iron sights and zoom without changing ammo, damage or cadence, and the server SHALL replicate the aim state and retain barrel-cover validation. (ANIM-13)
6. WHEN the player moves or turns THEN weapon motion SHALL have a restrained human cadence rather than rapid foot-phase oscillation; empty M1911/MP5/M4A4 reloads SHALL visibly manipulate their slide/charging handle/bolt release before returning to grip, while tactical reloads retain the chamber. (ANIM-14)
7. WHEN firing close to another actor THEN clearance SHALL use posed anatomy rather than its broad movement capsule, preserve the unobstructed hip/ADS muzzle ray, and resolve any real body/cover crossed before the muzzle without spawning beyond it or changing damage, ammunition or cadence. (ANIM-15)

**Independent Test**: Record M1911 continuously first; review contacts and transitions at normal speed and adjacent native frames, then repeat the standard for the primaries.

### P1: Grounded locomotion

**User Story**: As an observer, I want visible weight transfer and stable feet during locomotion and injury.

**Acceptance Criteria**:

1. WHILE an actor starts, brakes, turns, strafes, runs, crouches, jumps, lands or limps the shared rig SHALL keep finite anatomical transforms and grounded stance contacts without visible knee pops in the recorded route. (ANIM-05)
2. WHILE every third peer snapshot is dropped the client SHALL preserve bounded interpolation and coherent weapon/physical state. (ANIM-06)

**Independent Test**: Shared planted-foot tests plus full-body native peer route and snapshot-loss logs.

### P1: Human physical falls

**User Story**: As a player, I want falls to continue the character's motion and react to surfaces rather than repeat a canned location.

**Acceptance Criteria**:

1. WHEN control transfers to ragdoll THEN each present segment SHALL start at the previous authoritative posed transform with inherited root and articulated motion. (ANIM-07)
2. WHEN force direction or application point changes THEN the initial segment velocities and angular response SHALL change accordingly; no random displacement SHALL supply fall variation. (ANIM-08)
3. WHILE a corpse contacts floor, wall, corner or step the articulated solver SHALL keep finite, bounded joints and settle without persistent jitter; running and resting falls SHALL produce distinguishable coherent trajectories. (ANIM-09)
4. WHEN a living actor falls THEN recovery SHALL wait for support and clearance and preserve pose continuity; death SHALL remain continuous until physical rest. (ANIM-10)

**Independent Test**: Deterministic physical scenario matrix and native UDP fall/recovery/death evidence in both perspectives.

### P1: Acceptance evidence

**Acceptance Criteria**:

1. The delivery SHALL retain equivalent before/after videos, real-frame timestamps, launch/loadout/FOV/route details, frame/tick/snapshot measurements, formatting/check/tests and single/dual real UDP raid results under `target/playtest/`. (ANIM-11)
2. The game guide SHALL describe implemented presentation and limits and include a reviewed Mermaid player flow. (ANIM-12)

**Independent Test**: Evidence index links every external plan item; no unsupported artistic PASS.

## Edge Cases

- IF snapshots are missing THEN persistent reload/physical state SHALL reconstruct without duplicate parts or replayed flashes.
- IF a reload is interrupted by swap, fall or death THEN its presentation SHALL blend back without granting ammo early.
- WHEN FOV reaches 20 or 160 degrees THEN weapon and hand projection SHALL remain bounded and intentionally framed.
- IF support or headroom is absent THEN recovery SHALL NOT restore standing control.
- IF an actor or concrete lies between the eye and the muzzle THEN the shot SHALL NOT bypass it; clearance through a posed gap SHALL NOT hit the movement capsule, and penetrable wood SHALL consume energy once.

## Implicit dimensions

Input bounds: existing validation remains. Partial failures: harness exits nonzero and retains logs. Idempotency: shot/piece IDs retained. Auth: server authority unchanged. Ordering: shared pose and physics handoff verified. Lifecycle: existing piece cap/expiry/reset retained. Observability: native videos and metrics. External failure: inaccessible references recorded, never invented. Transitions: interruption/fall/recovery tested. Remaining production concerns are N/A because this is a local/LAN presentation change.

## Requirement Traceability

| Requirement ID | Story | Phase | Status |
| --- | --- | --- | --- |
| ANIM-01 | Weapons | T2–T4 | Implemented; final frozen visual review open |
| ANIM-02 | Reload | T2–T4 | Unit/intermediate native PASS; final review open |
| ANIM-03 | Transitions | T3–T4 | Unit/intermediate native PASS; final review open |
| ANIM-04 | Flash | T3–T4 | Unit/intermediate native PASS; final review open |
| ANIM-05 | Locomotion | T5 | Implemented; half-turn regression fixed; final native open |
| ANIM-06 | Network | T7 | Intermediate dual/loss PASS; final frozen run open |
| ANIM-07 | Handoff | T6 | Unit/integration PASS; final native open |
| ANIM-08 | Force | T6 | Unit/integration PASS; final native open |
| ANIM-09 | Contacts | T6 | Eight-case integration PASS; final native matrix open |
| ANIM-10 | Recovery | T6 | Same-ID recovery integration/native07 PASS; final frozen run open |
| ANIM-11 | Evidence | T1, T7 | Baseline/intermediate evidence retained; final gates/performance open |
| ANIM-12 | Guide | T7 | Guide/Mermaid updated; final review open |
| ANIM-13 | ADS | T3–T4, T7 | User-requested refinement; implementing |
| ANIM-14 | Motion/reload/FOV | T3–T5, T7 | User-rejected candidate10 animation quality; implementing |
| ANIM-15 | Point-blank origin | T8, T7 | Implemented; close hip/ADS/body/cover/wood/gap regressions and native14 fatal-shot flow PASS; final feature acceptance open |

## Success Criteria

- All five acceptance items of the external plan have located evidence.
- Required command gates pass without weakening original assertions.
- Visual defects are fixed from capture review; remaining limits are explicit, not labeled AAA by assertion.
