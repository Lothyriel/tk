# Physical cover design

Keep the 176 stable structural cells. Damage is sparse inside each cell: shared axis-aligned box subtraction produces identical solid geometry for client meshes and server compound colliders. Wood bores cut through the struck cell along the dominant impact axis. Concrete chips remove a shallow local volume before cumulative cell failure; this is a stepped game approximation, not a material fracture solver.

Server projectile queries use the current cell collider directly, alongside the ordinary Rapier world query, so two shots in one tick see the first shot's hole before the Rapier backend synchronizes. Static colliders are rebuilt after cuts for authoritative character collision. Per-cell geometry and global cuts are bounded; saturation fractures a section rather than restoring an old hole.

An impact-ID-to-cell map makes old marks expire with their support. Destructible impacts that open geometry do not leave a flat quad across the void. Surviving cut faces provide the opening instead.

A precomputed face-neighbor graph anchors cells touching the floor. After structural failure, a flood fill finds unsupported cells. Detach the actual remaining compound shape once per cell, disable the static collider, and spawn server Rapier rubble with modest impulse, friction, damping and continuous collision detection. Rubble collides with the map and other rubble, but is excluded from player-controller support. A separate 64-body/20-second budget keeps it bounded.

The actor snapshot is already close to its 60 kB limit. Replicate persistent cuts and bounded rubble poses in a separate destruction snapshot on a dedicated unreliable channel, with sequence rejection, periodic full refresh and raid identity. Changed/moving destruction sends at a bounded rate; static unchanged cover uses a slower refresh so late joiners recover. Clients rebuild meshes only when cuts change and interpolate rubble poses each frame. Full state avoids delta-baseline dependency after loss.

Three cohesive tasks run sequentially, with co-located tests and local commits. Shared protocol producers/consumers change in one compiling unit. Existing coarse-only tests migrate to the new explicit requirements; unrelated tests remain unchanged. Engine fixtures stay headless. Full Clippy's known baseline failures remain visible, and rendered/real-network acceptance is reported separately.
