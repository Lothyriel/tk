# Author a voxel prop

Start with the [fictional light beacon](../assets/voxel/beacon_authoring.json).
It is an original digital prop, not a gameplay item. It shows a root, a
movable visual tile, a support-hand marker and a -Z visual guide. Make a copy
with a new stable asset ID to author another prop. No plugin is needed to
build or test it.

## Source → bake → catalogue → preview

1. Define metre-scale cuboids in each part's local frame. +X is right, +Y up,
   and -Z forward. Keep rigid pivots and sockets in normalized xyzw form.
   Preserve material groups across LODs; a moving piece reuses near geometry.
2. With Python 3.12+, run `python scripts/bake_voxels.py`, then
   `python scripts/bake_voxels.py --check`. Commit the JSON source, its baked
   JSON and the generated `assets/baked/logical_rigs.ron`. Do not hand-edit
   generated files. Compare `VOXEL` cost lines and the manifest; the beacon
   uses 48 vertices, 24 triangles, 2 submissions and 1,440 B of buffers.
3. Bind presentation roles to logical sockets, not duplicate camera offsets.
   The beacon test parses this exact `Geometry` value using the catalogue
   loader; it does not register a playable definition or a gameplay profile:

   ```ron
   (authored: Some((
       asset: "beacon_authoring",
       anchors: [(SupportGrip, "palm.support"), (Sight, "view.guide")],
       sockets: [],
   )))
   ```

   A role such as `Sight` supplies a reference frame, not automatic camera
   alignment. For playable content, choose a supported shared profile,
   specify its pose/event behavior and declare the intended visual roles
   through existing catalogue and presentation bindings. If no profile fits,
   document the missing behavior before adding code; don't patch in a private
   camera offset.
4. From a clean checkout, run
   `python scripts/verify.py --package common --filter beacon_authoring` and
   `python scripts/verify.py --package client --filter beacon_authoring`.
   The latter builds both local-view and world instances in a windowless app
   using the production assembler and cache. Check that they share handles
   and publish the same socket frames. This is not a pixel-level check.
5. After coordinating the render workload, open the beacon in the existing
   item-review bench. It does not connect to a server:

   ```sh
   TK_ITEM_REVIEW_FOCUS=beacon_authoring cargo run --locked -p client -- --item-review
   ```

   Compare left (world render layer) against right (view-model render layer).
   Both use the same baked asset and assembler at unit scale with matched
   review cameras. Press **1**, **2**, or **3** to turn both copies; **F12**
   writes a static `item-review.png` in the working directory. Save each
   image under a different name. This bench does not test network replication
   or an equipped gameplay pose.
6. For an integrated, playable asset, confirm the same catalogue selection on
   two real clients, toggle F8 and compare local and remote pose, authored
   contacts, sight framing and moving parts. Use F9 to compare LODs. Use the
   coordinated [native review route](VOXEL_PIPELINE.md#native-rig-inspection)
   for existing playable content. The beacon stays outside the gameplay
   catalogue; this guide does not turn it into a playable item.

### If validation fails

| Failure | Where to look |
| --- | --- |
| Duplicate asset, part, or socket ID | Read the source path and field in the baker error; assign unique IDs and update intended references. |
| Catalogue binding names an absent socket | Read the rig and socket names in the binding error. Compare the source `sockets` and regenerate; do not add a second explicit offset. |
| Stale bake or logical rig | Run the baker again and check the generated diffs. `--check` never rewrites files. |

## Copy into an authoring card

Copy the beacon JSON for the fillable asset, and fill in this checklist before
setting acceptance:

- **Identity and rights:** display name, stable IDs, source and provenance,
  permitted references and licenses.
- **Scale and finish:** metres/axes, silhouette, palette and material families.
- **Rig:** part tree, pivots and movable visual pieces; support-hand contact,
  view/ADS guide, profile, pose and event expectations (or why not applicable).
- **Cost:** near/far triangles, part/material submissions, cached buffer
  bytes and target budgets; justify any exceptions.
- **Review:** local and remote views, neutral/action poses, viewport sizes,
  LOD and material readability; distinguish test evidence from visual approval.
- **Delivery record:** build or commit, commands, elapsed authoring and review
  time, which changes were data/art/code, and any remaining exceptions.

See the [pipeline reference](VOXEL_PIPELINE.md#authoring-contract) for schema
details and hard limits.
