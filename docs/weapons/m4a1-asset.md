# M4A1 asset authoring

## Dimensioned blockout

`assets/voxel/m4a1_refined.json` and `m4a1_magazine_30.json` are original,
dimensioned source assets under refinement. They are not yet the active game
bindings. Native appearance, final detail, contact integration and artistic
acceptance remain required by TAR-24.

Provenance follows F1/F2 in [the reference contract](../WEAPON_REFERENCES.md):
published longitudinal dimensions are **F**, other external dimensions and
recesses below are **V** interpretations. PBR values and simplification are
**G** choices. Reference photographs are not distributed as game assets.

| Property | Authored value |
| --- | --- |
| Origin / axes | Breech on bore; +X right, +Y up, -Z forward |
| Physical barrel | 0 to -0.36830 m (**F**, 14.5 inches) |
| Device outlet | -0.39565 m (**V**, selected device extension) |
| Extended stock rear | +0.44255 m; assembled length 0.83820 m (**F**, 33 inches) |
| Stock travel | -0.08255 m; retracted length 0.75565 m (**F**, 29.75 inches) |
| Sight line | X=0, Y=0.066 m (**V**), continuous front/rear/eye anchors |
| Magazine seat | (0, -0.065, +0.029) m; magazine has its own seat-local root |
| Ejection opening | Right upper-receiver wall; open between Z=0.012–0.084 m |

The shell, carrier, charging handle, bolt catch, dust cover, trigger and selector
have separate pivots. The charging handle is a receiver child, never a carrier
child. Receiver and magwell are hollow assemblies of surfaces; the source does
not paint an opening onto a solid receiver. The near rear sight contains an
actual aperture. Rubber buttpad, polymer furniture, anodized receiver, darker
steel and carrier have distinct material responses.

The first far override removes only the rail covers' 2 mm raised ribs. It
retains the bore cavity and shares every pivot/socket with near. This bounded
simplification is not yet the required complete 2/4 mm in-game comparison.

## Current bake cost

| Asset | Near vertices / triangles | Far vertices / triangles | Draw submissions | Cached mesh bytes |
| --- | --- | --- | --- | --- |
| M4A1 | 3272 / 1636 | 2824 / 1412 | 19 | 101040 |
| 30-round magazine | 272 / 136 | same | 2 | 8160 |

Counts come from the committed manifests, excluding engine/GPU allocation
overhead. Independent source tests check published longitudinal dimensions,
sight axes, actual aperture clearance and the entire downward magazine-removal
sweep against the lower receiver. The baker separately checks all surfaces,
materials, hierarchy and the maximum 2 mm LOD solid deviation.

## Integration boundary

The old baseline uses a presentation-space origin and an oversized envelope.
The new breech-root asset requires an explicit transform into the existing
presentation frame, composed once for geometry and logical anchors together.
Muzzle clearance, sight framing and hands must be reverified at that boundary.
Do not merely replace a visual binding while leaving the old logical contacts
or an old offset on the authoritative barrel.
