# Physical humanoids design

`common::humanoid` owns segment IDs, pose evaluation and ray/posed-box intersections. `BodyHealth` owns persistent regional tissue, muscles, organs, blood and absent segments. The server advances gait from effective KCC motion, evaluates support and resolves anatomy before applying damage. Rapier bodies with limited joints take over on death; separated bodies have stable IDs and expiry. Snapshots carry persistent state, not only transient damage events. Client presentation shares the pose definition, interpolates remote roots, builds reusable layered voxel meshes and consumes monotonically identified confirmed shot feedback. No firing audio is loaded or spawned.

Normal gameplay and fixtures use the same input, projectile and raid systems. Baseline captures and subsequent evidence are stored separately. Existing aggregate health tests remain as compatibility checks, with new tests targeting the regional live path.
