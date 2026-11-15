# Local camera motion

## Problem Statement
The local camera inherits a player transform replaced by roughly 30 Hz server snapshots. At 165 FPS it holds and jumps, especially when running beside nearby cover.

## Out of Scope
Client movement prediction/reconciliation, protocol changes, remote actor interpolation, physical head animation, rendering settings and movement balance.

## Assumptions & Open Questions
| Assumption | Chosen default | Rationale |
| --- | --- | --- |
| Reported lag | Positional stepping during ordinary movement | Direct snapshot assignment exists in the local camera's parent |
| Presentation delay | Three nominal snapshot intervals, 100 ms | Two intervals bridge one lost update; the third covers simulation tick, delivery and render-frame jitter |
| Authority | Interpolate confirmed positions, never extrapolate | Preserve server collision and gameplay decisions |
| Discontinuities | Reset on first sample, raid/lobby reset, alive/status/mode change, displacement over 4 m or receive gap over 0.25 s | Avoid sweeping the view across deployment, correction or stale history |
Open questions: none. Visible smoothness remains a manual playtest criterion.

## User Stories
As a player I can run past a wall or track a crate without a 30 Hz camera staircase despite a high render frame rate.

Acceptance Criteria:
1. While authoritative walk (3.25 m/s) or run (5.5 m/s) positions arrive at approximately 30 Hz on a 128 Hz simulation timeline, local root and both child cameras SHALL advance at the render rate (60 and 165 FPS); after warmup, each constant-speed frame step SHALL match speed/frame-rate within 1 mm, including one dropped snapshot and delivery jitter up to 8 ms (CM-01).
2. While local motion is interpolated, the client SHALL sample the confirmed 3D path 100 ms behind the server timeline anchored at first receipt, preserve the latest authoritative session position and local look rotation, and keep the displayed translation within the buffered authoritative path; stopping or a prolonged packet outage SHALL not extrapolate past the last confirmed position (CM-02).
3. When first receiving a player or observing a specified discontinuity, the client SHALL immediately place the root at that confirmed position and clear old interpolation history; old/duplicate network sequences SHALL not change the displayed target (CM-03).

## Requirement Traceability
| ID | Implementation | Evidence | Status |
| --- | --- | --- | --- |
| CM-01 | client/src/sync/motion.rs | motion.rs:198, motion.rs:213, motion.rs:228; validation.md | verified |
| CM-02 | client/src/sync/motion.rs | motion.rs:241, motion.rs:248, motion.rs:249, motion.rs:272, motion.rs:284, motion.rs:289; validation.md | verified |
| CM-03 | client/src/sync/mod.rs | motion.rs:332, motion.rs:337, motion.rs:351, motion.rs:355; validation.md | verified |

Behavioral requirements have headless evidence. Full acceptance remains blocked by existing workspace Clippy errors; rendered smoothness and real UDP delivery have not been checked.
