# 60 Hz snapshot trial

## Problem Statement
Test twice as frequent authoritative snapshots with a shorter local presentation buffer after correcting the camera's snapshot stepping.

## Out of Scope
Physics tick changes, delta compression, movement prediction, destruction, public network tuning and artistic acceptance.

## Assumptions & Open Questions
| Assumption | Chosen default | Rationale |
| --- | --- | --- |
| Snapshot cadence | 60 Hz, shared interval | Keep server cadence and client buffer in agreement |
| Local buffer | Three intervals, 50 ms | Halve the previous 100 ms delay while retaining loss/jitter coverage |
| Physics | Existing 128 Hz | Isolate snapshot frequency from simulation behavior |
| Testing | Headless serialized transport and trajectory tests | A local trial does not establish visible smoothness or real UDP cost |
Open questions: none. Manual comparison remains pending.

## User Stories
As a player I can try more frequent position updates and a shorter buffer without changing movement or physics rules.

Acceptance Criteria:
1. While the authoritative simulation advances 128 fixed ticks in one second after warmup, the server SHALL produce and serialize 60 snapshots, spaced by two or three simulation ticks, while its fixed timestep remains 1/128 second (SR-01).
2. While authoritative positions arrive at 60 Hz, the local root and camera children SHALL retain the existing 1 mm frame-step tolerance at 60/165 FPS with one lost snapshot and 0–8 ms delivery jitter; the confirmed 3D path SHALL be sampled with a 50 ms delay, preserving look, stopping and reset/sequence protections (SR-02).

## Requirement Traceability
| ID | Implementation | Evidence | Status |
| --- | --- | --- | --- |
| SR-01 | common/src/lib.rs; server/src/tick/mod.rs | server/src/tick/tests.rs:70, server/src/tick/tests.rs:100, server/src/tick/tests.rs:104, server/src/tick/tests.rs:107; validation.md | verified |
| SR-02 | client/src/sync/motion.rs | client/src/sync/motion.rs:202, client/src/sync/motion.rs:228, client/src/sync/motion.rs:241; validation.md | verified |

Headless behavioral criteria pass. Full acceptance remains blocked by existing Clippy failures; visible smoothness and real UDP performance remain unverified.
