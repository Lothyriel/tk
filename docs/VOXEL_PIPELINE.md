# Declarative voxel asset pipeline

## Format decision and limited proof

Use versioned JSON cuboids with per-part pivot-local coordinates and named
materials. Bake offline with Python's standard library:

```sh
python scripts/bake_voxels.py
python scripts/bake_voxels.py --check
python scripts/verify.py --mode docs
```

Sources live in `assets/voxel/`; committed generated buffers and manifests live
in `assets/baked/`. The client Cargo build runs the same `--check` gate; malformed
sources and stale bakes fail compilation with file/field diagnostics. Python
3.12+ is already a project prerequisite; `PYTHON` can select its executable for
Cargo. This build step checks artifacts without modifying the working tree.
The central runner discovers the baker acceptance tests in every mode.

The comparison was limited to `pipeline_fixture.json`: receiver, magazine,
bolt, four material families and two sockets. It is an original engineering
exemplar, not approved M4 art.

| Choice | Geometry | Hierarchy/pivots | Runtime/build implications |
| --- | --- | --- | --- |
| JSON cuboids (selected) | Continuous box boundaries; stepped unions, no grid-induced scale change | Explicit parent IDs and normalized quaternion/translation pivots; continuous socket poses | Small text diffs; bounded offline union/meshing; direct material-family/PBR data |
| MagicaVoxel `.vox` with scene extensions | `XYZI` has byte-sized integer cells and palette IDs | `nTRN/nGRP/nSHP` preserve hierarchy, but `_t` is integer and `_r` encodes signed axis permutations | Needs axis/unit conversion and continuous pivot/socket sidecar; `MATL` needs mapping to project surface families |

Sources: [base format](https://github.com/ephtracy/voxel-model/blob/master/MagicaVoxel-file-format-vox.txt)
and [scene/material extension](https://github.com/ephtracy/voxel-model/blob/master/MagicaVoxel-file-format-vox-extension.txt).
The comparison does not claim `.vox` lacks hierarchy or materials. A future
importer could target this source contract, but there is one executable bake
route today and no second importer to maintain.

A centre-sampled occupancy probe over each part of the original baseline
(before adding the LOD test's 2 mm receiver ridge) measured:

| Cell | Occupied cells | XYZI records only (4 B/cell) | Maximum pivot/socket error if snapped |
| --- | --- | --- | --- |
| 2 mm | 60,152 | 240,608 B | 0.995 mm |
| 4 mm | 7,238 | 28,952 B | 2.302 mm |

That baseline source was 2,211 B. XYZI measurements exclude scene chunks, palette, material
and sidecar costs; they are a format comparison, not total file benchmarks.
The baseline bake preserved all authored coordinates exactly as JSON numbers:
168 vertices, 84 triangles, 7 part/material draw submissions, 5,040 B of
position/normal/u32-index data. Final LOD fixture costs are below. Mesh memory excludes JSON text, engine allocation
overhead, GPU alignment and materials. Compare complete in-game costs at the
M4 slice gate before accepting production content budgets.

## Authoring contract

Version 1 requires `units: "metres"` and
`axes: "right_handed_y_up_negative_z_forward"`, matching
[weapon references](WEAPON_REFERENCES.md). Bounds are explicit `min`/`max`,
not dimensions silently multiplied by an import scale.

- Asset, part, material and socket IDs are stable lowercase names.
- A part's `pivot` maps its local vertices and sockets into the parent frame.
  Exactly one root is required; parents must exist and cannot form a cycle.
  Quaternions are normalized xyzw; all components are finite.
- Boxes are axis-aligned in their own part frame. Rotated rigid pieces use their
  parent transform. Continuous partial boundaries preserve dimensions such as
  a 368.30 mm barrel without stretching to a voxel-grid multiple.
- Boxes of the same material may overlap as a union. Overlapping different
  materials are an authoring error, rather than order-dependent paint priority.
  Adjacent different materials are valid and retain separate surfaces.
- Materials explicitly specify opaque sRGB color, perceptual roughness and
  metallic, plus `metal`, `polymer`, `wood` or `rubber` surface family.
- `movable` identifies independent mechanical pieces. Meshing never crosses a
  part boundary, even where two pieces touch; removing a magazine must reveal
  its previously hidden mating faces.
- Sockets reference a part and retain full translation/quaternion precision.
  They are metadata, not vertices, and are shared by both LODs.

Hard limits guard malformed or unexpectedly expensive input: 1 MiB/source,
64 parts, 256 boxes/part, 32 materials, 128 sockets, depth 16, 262,144 compressed
cells/part and 100,000 total vertices. Coordinates are bounded to ±8 m; each box
extent is at least 0.01 mm. These are loader limits, not artistic/performance
acceptance budgets. Unknown fields/references, duplicate JSON keys/IDs,
nonfinite numbers, bad scale/axes or pivot rotations fail before publication.

## Bake and evidence

The baker partitions each part along its authored box boundaries. It evaluates
solid occupancy on that compressed grid, removes all internal faces (including
material interfaces) and greedily merges coplanar rectangles sharing material
and normal. It never fills concave steps. Output has outward counter-clockwise
triangles, hard normals and one mesh per visible part/material pair.

Parts/materials/sockets and traversal are canonically ordered. Reordering source
arrays leaves geometry unchanged; SHA-256 still records the exact source bytes.
The entire asset set is validated before any output is written. `--check` also
requires byte-identical generated artifacts. Manifests include local bounds,
per-surface and per-part bounds, per-part and total vertices/triangles/submissions/
buffer bytes, sockets and hash.

Acceptance fixtures independently calculate triangle area and signed volume for
boxes, adjacent solids, overlaps, concave steps and material boundaries; they
also check winding, continuous pivots/sockets, moving-part separation, limits,
stale artifacts and diagnostic failures. A new piece/material uses data only.

## Bevy consumer and shared cache

`client::render::voxel::Plugin` is wired into the renderer. Insert `VoxelModel`
on a root with `Transform` and `Visibility`, providing an asset ID and
`view_model` flag. The build generates an embedded registry from all checked
bakes; adding an asset or part needs no renderer branch. Orphaned baked files
without sources fail the build, so the registry cannot silently include an
obsolete asset.

Loading is lazy and occurs once per requested asset. One `VoxelCache` owns strong
handles for its lifetime. Both perspectives and every actor clone these handles,
not mesh buffers. Each part/material pair becomes one indexed Bevy mesh and
uses its authored PBR material. Render layers and view-model shadow exclusion
are instance properties; they do not create another copy of the asset.

Each instance has one owned container, named part pivots, named socket children,
and material surfaces beneath their pivot. `VoxelPart`, `MovableVoxelPart` and
`VoxelSocket` mark the integration points for mechanics. Animate a pivot and its
surfaces/sockets follow through Bevy's normal hierarchy propagation. The fixture
has 13 child entities, regardless of its voxel occupancy.

Only a changed `VoxelModel` requests assembly. An identical assignment is a
no-op. An invalid replacement records `VoxelLoadError` and preserves the last
valid hierarchy; no partial replacement or handle allocation is published.
Changing a valid model/perspective replaces its owned hierarchy. Removing the
component or despawning the root removes the owned entities. Cached buffers
remain available for future instances.

Minimal Bevy app tests verify identical handles across two world actors and a
view-model, stable allocations over 60 updates, PBR/buffer fidelity, precise
socket propagation during magazine movement and rotated roots, atomic invalid
replacement, perspective replacement and recursive cleanup. They use real
`Assets`, `Mesh3d`, material handles and transform systems without a window.

## LOD contract and baseline calibration

Source version 1 accepts optional `far_boxes` on static parts. Baker version 2
emits both meshes; without an override the same geometry is reused. The override
must preserve visible material groups and cannot increase vertices or triangles.
Movable mechanisms always reuse near geometry. One pivot/socket hierarchy serves
both levels, so switching handles cannot move contacts or rebuild mechanisms.

The baker bounds bidirectional distance between the complete near/far solids to
**2 mm**, using cells partitioned at both sets of box boundaries. Every differing
cell must lie within tolerance of a target cuboid: the maximum corner distance
to that convex cuboid bounds all points of the cell. This conservative test
may reject a valid simplification, but cannot approve one merely because sampled
vertices happen to match. The bound limits silhouette displacement, not artistic
equivalence or topology; authored openings/details still need slice review.

`voxel::lod` runs after global transforms propagate and before Bevy recalculates
mesh bounds. Each active world perspective camera evaluates transformed part
corners, viewport resolution, FOV, distance and projected error. The error bound
includes off-axis depth changes and conservative transform scale under shear.
All cameras must accept far detail. Missing/invalid projection keeps near detail.
View-models always use near geometry.

| Transition | Distance | Projected extent | Projected deviation |
| --- | --- | --- | --- |
| Near → far (all conditions) | ≥5 m | ≤80 px | ≤0.5 px |
| Far → near (any condition) | ≤4 m | ≥100 px | ≥0.75 px |

The band holds the previous level under camera jitter. Changes replace only
`Mesh3d` handles; no entities, buffers or materials are created. Bevy updates
surface bounds from the selected mesh. Identical near/far buffers share a handle.

The final fixture adds one 2 mm receiver ridge which the far variant omits:

| Level/storage | Vertices | Triangles | Surface submissions | Buffer bytes |
| --- | --- | --- | --- | --- |
| Near active | 200 | 100 | 7 | 6,000 |
| Far active | 168 | 84 | 7 | 5,040 |
| Unique cached buffers for both | 228 | 114 | 8 stored meshes | 6,840 |

Far detail reduces active geometry/buffer bytes by **16%**. Six of seven
surfaces share handles across levels, including all moving parts. Both actors
and view-models reuse the eight cached meshes and four materials. These measured
fixture budgets calibrate the initial thresholds; full M4 in-game rendering,
draw batching, frame-time and artistic acceptance remain the slice's gate.

Bevy fixtures drive 3→5.2→4.5→3.9→4.5→6 m transitions, 60 jitter frames, zoom
60°→15°, 1080p→4320p, multiple cameras and missing/zero-sized projection.
They assert active triangles 100↔84, unchanged entities/anchors, detailed
view-models and stable cache allocations. Bake tests reject ±3 mm changes,
accept ±1 mm, and reject simplified mechanisms or lost material groups.

The current legacy weapon presentation is not yet migrated to this data-driven
assembler; configured weapon/hand integration belongs to TAR-23.
