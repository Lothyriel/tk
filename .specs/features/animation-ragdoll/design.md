# Animation and ragdoll design

Preserve common/client/server boundaries. Shared weapon presentation anchors and time curves describe original art without deciding gameplay. The client uses cached geometry and smoothed presentation state; authoritative snapshots still decide weapon, ammo and action availability. Use the same part definitions and muzzle/grip transforms for both views, with a framing transform only in first person. Separate moving magazine, slide/charging handle and support hand.

Record native framebuffers with real elapsed-time sidecars. Encode timestamp-preserving MP4s after clients exit. Baseline must precede presentation edits. Extend the ordinary-input route for continuous per-weapon action coverage and reciprocal peer observation; no damage overrides or teleports in native validation.

Locomotion remains the shared anatomical pose used for hit volumes. Add acceleration/turn weight and support-aware foot goals without moving damage away from visible pose. Test foot displacement and finite limb rotations.

For physical handoff capture previous evaluated world-space segment transforms server-side. Compute per-segment linear/angular velocities with finite differences, capped only for safety; apply localized force and torque rather than giving every limb the same delta velocity. Use anatomical joint pivots and segment-specific masses/limits. Preserve bounded piece lifetime/cap. Recovery remains server-owned with support/clearance checks. Use deterministic environment fixtures to distinguish trajectories and inspect native transitions under loss.

No sound, blood/fluids, damage tuning, remote publication or unrelated refactoring.

## Living-ragdoll closure

Surviving falls use the articulated body path, not just deaths. Persistent `PhysicsPieceData` IDs own live fall and get-up so clients never swap to an unrelated canned silhouette. Firing/reload remain blocked and the existing support/headroom guards are preserved. The support-controller pose is only the bounded-piece-budget fallback.

Bounded shared state: optional packed physical rig in `PoseState`, 17 segments with millimetre i16 position and normalized i8 quaternion components (10 bytes/segment). `evaluate_pose` returns that actual physical rig during fall/recovery, keeping the existing ordinary anatomical head-shot tests and server hit boxes aligned with the bodies. Complete 32-actor/320-piece stress with all optional poses is 59,528 bytes, below the unchanged 60 KB assertion. Protocol is 4. Original exact floating-point motion capture still initializes physical pieces; packing applies to shared pose/hit reconstruction, not to the first physical transform.

Server-only articulated-body metadata should retain the part entities, initial low control root, support/quiet measurements and optional recovery start/end transforms. On a surviving fall, create dynamic pieces from captured world pose and motion. Stop independent KCC translation while those pieces own the body; follow pelvic X/Z and keep the camera's low control root above the floor. Ground support, sufficient rest, current leg support and a clear standing space authorize recovery. Blend the existing pieces from the actual rest pose to the target shared standing pose as kinematic bodies over the existing recovery interval, then remove the pieces in the same snapshot that restores controlled visuals. Death during the fall keeps those IDs and current motion instead of spawning a second corpse. Later separations must detach existing pieces, not duplicate them.

Preserve old assertions (grounded prone root ~0.4 m, supported/headroom-gated recovery, standing return, ordinary posed head shots) and add actual live-piece continuity/recovery tests. A real blocker caused by genuinely conflicting test expectations requires explicit disclosure, not a hidden assertion change. Verify the procedural fallback only at the existing piece-budget boundary if needed; the two-client acceptance must exercise the full articulated path.

Native candidate07 proved surviving same-ID recovery under UDP loss but exposed self-camera clipping and absent-limb observer targeting. Hide living local attached physical meshes while retaining remote/detached visibility; the local camera still follows the actual head. The ordinary review driver targets the thorax once its intended segment is absent. Candidate08 freezes those fixes for visual revalidation. Knees use anatomical joint frames instead of adding their angular limits to the captured bend; four additional solver iterations stabilize the changed contact poses without changing combat rules.

## User-approved point-blank repair (ANIM-15)

Candidate13 exposed an existing origin-policy hazard: the eye-to-muzzle query uses broad character capsules, then redirects a parallel offset-aimed shot to the eye. The user authorized fixing combat, not moving the acceptance fixture away. Preserve the unobstructed muzzle, input, timing, damage and native deadline.

Add server-only pending muzzle-clearance state to a freshly fired projectile. In its first authoritative projectile update, run the eye-to-muzzle segment through the existing posed-anatomy and surface collision/penetration resolver, then continue normal travel from the original muzzle only if not stopped. Never use a movement capsule as a bullet damage volume. This avoids both coarse-capsule false positives and spawning beyond actual close actors/concrete; wood attenuation shares the same once-per-surface bookkeeping. No new network fields or protocol bump. Diagnostic logs are opt-in and include actual eye/muzzle/clearance hit data.
