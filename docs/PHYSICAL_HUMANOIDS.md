# Physical humanoids

## Design and ownership

The game is a local/LAN alpha with server-authoritative movement, combat and recovery. Loadouts are free; secured totals are session memory, not persistent accounts.

| Area | Responsibility |
| --- | --- |
| `common/src/humanoid.rs` | 17 stable segments, articulated poses, dimensions, posed ray boxes and sorted intersections |
| `common/src/combat/anatomy.rs` | Seven regions, muscles, organs, blood/vigor, armor-gated injury and one-time descendant separation |
| `server/src/tick/physical.rs` | Ground support, impulse correction, fall/action gating, recovery clearance, Rapier pieces/joints and limits |
| `server/src/tick/live_body.rs` | Surviving articulated falls, actual-pose readback and same-ID get-up |
| `client/src/render/actors.rs` | Cached voxel shells, wounds/interiors, equipment, interpolated roots/poses and replicated pieces |
| `client/src/render/feedback.rs` | Confirmed-shot feedback, bounded recoil, reload/swap and camera presentation |

Keep render, damage and network segment IDs aligned. Missing anatomy removes its hit volume; separation occurs once. Destroyed trigger-arm capacity blocks authoritative firing/reload. Bilateral leg penalties combine by minimum so a second injury cannot improve movement.

The character controller excludes debris from support/autostep queries. Actor/debris collision groups avoid self-collision explosions. Production simulation is 128 Hz with approximately 30 Hz snapshots; replicated pieces are bounded and expire. Consult code/constants before changing caps or protocol.

## Authoring and tuning

Mesh units are metres. A segment is centred on its shared bone centre; local +Y points toward its proximal joint and actor forward is -Z.
Keep `Segment::index()` stable. Change `Segment::half_size()` and organ offsets alongside silhouettes so damage and presentation agree.
Replace cached `VoxelAssets` handles rather than allocating meshes per actor/frame.

Tune anatomy/penetration in `common/src/combat/anatomy.rs`, gait in the humanoid/locomotion modules, support/recovery/joints in the physical modules and recoil/action feedback in the client.
Four aggregate health pools remain compatibility telemetry; live projectile damage uses regional anatomy.
Procedural meshes, poses, damage and tuning are original Rust work. FRUKT was inspected as a behavioral/structural reference for ground probes, limb joints, muscle integrity and separation, not copied or shipped.

## Inspection

Use headless Bevy/Rapier fixtures first; see [the verification skill](../.agents/skills/tk-verification/SKILL.md).
For a requested manual tissue review, deploy two clients without vest/helmet and target the **actor's** left/right side, mirrored when facing them.
Check partial limps, support loss/recovery, separation, organ damage and armor protection through ordinary input. Stop after a partial organ hit to observe progressive effects rather than destroying the entire region.

`scripts/validate_humanoids.py` is an explicit **rendered** acceptance suite, including solo/dual UDP routes, not a headless shortcut.
`scripts/benchmark_baseline.py` exports immutable source and records provenance without resetting the user's tree. `scripts/compare_performance.py` compares matching workloads; do not run them concurrently on the same GPU/port.

## Evidence boundary

The original acceptance-11 report recorded 64 tests, normal raid and two real UDP clients, plus a bounded 320-piece load. Historical performance showed higher rendering/bandwidth costs; it was not a WAN or 32-rendered-client guarantee.
Those point-in-time results are preserved in [Git at 6a4a99b](https://github.com/Lothyriel/tk/blob/6a4a99b/docs/PHYSICAL_HUMANOIDS.md), including failed attempts and source/evidence hashes. They are not current measurements.

Later animation, recovery and matched-performance acceptance remains tracked through [ANIMATION_RAGDOLL.md](ANIMATION_RAGDOLL.md) and its Linear cards. A passing engine test does not certify artistic quality or camera comfort.
