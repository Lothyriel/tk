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

A centre-sampled occupancy probe over each exemplar part measured:

| Cell | Occupied cells | XYZI records only (4 B/cell) | Maximum pivot/socket error if snapped |
| --- | --- | --- | --- |
| 2 mm | 60,152 | 240,608 B | 0.995 mm |
| 4 mm | 7,238 | 28,952 B | 2.302 mm |

The source is 2,211 B. XYZI measurements exclude scene chunks, palette, material
and sidecar costs; they are a format comparison, not total file benchmarks.
The selected bake preserves all authored coordinates exactly as JSON numbers:
168 vertices, 84 triangles, 7 part/material draw submissions, 5,040 B of
position/normal/u32-index data. Mesh memory excludes JSON text, engine allocation
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
  They are metadata, not vertices, and must survive future LOD changes.

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
per-part and total vertices/triangles/submissions/buffer bytes, sockets and hash.

Acceptance fixtures independently calculate triangle area and signed volume for
boxes, adjacent solids, overlaps, concave steps and material boundaries; they
also check winding, continuous pivots/sockets, moving-part separation, limits,
stale artifacts and diagnostic failures. A new piece/material uses data only.

The first increment provides the offline source/bake/build contract. Shared
Bevy handle loading and world/view LOD selection are the next TAR-21 increment;
the current in-game legacy weapons are not yet migrated to these buffers.
