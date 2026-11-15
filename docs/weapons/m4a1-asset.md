# M4A1 asset authoring

## Dimensioned asset

`assets/voxel/m4a1_refined.json` and `m4a1_magazine_30.json` are original,
dimensioned source assets used by the active game bindings. The TAR-24 static
shape/LOD review is recorded below.

TAR-26 adds component exports and qualifies the handguard as KAC RAS 98064,
including exposed upper/lower rails, vents and a receiver-side delta ring.
See [the kit sheet](m4a1-kit.md) for sources and component costs. The historical
TAR-24 review below does not certify these newer surfaces or modular combinations.

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
| Eye position | Z=+0.270 m (**G**); rear aperture 128 mm ahead of the camera, beyond its 100 mm near plane |
| Magazine seat | (0, -0.065, +0.029) m; magazine has its own seat-local root |
| Ejection opening | Right upper-receiver wall; open between Z=0.012–0.084 m |

The shell, carrier, charging handle, bolt catch, dust cover, trigger, selector,
forward assist and serrated magazine release
have separate pivots. The charging handle is a receiver child, never a carrier
child. Receiver and magwell are hollow assemblies of surfaces; the source does
not paint an opening onto a solid receiver. The near rear sight contains an
actual aperture. Rubber buttpad, polymer furniture, anodized receiver, darker
steel and carrier have distinct material responses.
The oxide-coated receiver uses a medium metallic response (0.50) and a slightly
lighter neutral base than steel; the exposed carrier is brighter and smoother.
These **G** values preserve surface separation under the game's existing ambient
light instead of using emissive edges or changing the scene lighting.

The far override removes the rail covers' 2 mm raised ribs, retaining the bore
cavity and every pivot/socket. The 2/4 mm surface comparison deliberately keeps
functional openings and continuous anchors at their authored precision rather
than globally rounding the whole assembly to a 4 mm lattice.

## Current bake cost

| Asset | Near vertices / triangles | Far vertices / triangles | Draw submissions | Cached mesh bytes |
| --- | --- | --- | --- | --- |
| M4A1 reference assembly | 5080 / 2540 | 4604 / 2302 | 23 | 153840 |
| 30-round magazine | 272 / 136 | same | 2 | 8160 |

Counts come from the committed manifests, excluding engine/GPU allocation
overhead. Independent source tests check published longitudinal dimensions,
sight axes, actual aperture clearance and the entire downward magazine-removal
sweep against the lower receiver. The baker separately checks all surfaces,
materials, hierarchy and the maximum 2 mm LOD solid deviation.

## Integration boundary

The old baseline uses a presentation-space origin and an oversized envelope.
The active breech-root asset uses an explicit hip frame from the catalogue and
the inverse `sight.eye` socket for ADS. Geometry, logical contacts and ballistic
bore use that frame. The device outlet remains distinct from the physical bore;
local/remote flashes follow the last accepted outlet, including after a rejected
revision. Hands consume source grip orientation as well as position.

The pipeline now exports `logical_rigs.ron` from these source sockets and pivots.
Catalogue geometry can reference the export by asset/socket name, with no copied
coordinates. Headless checks compare every M4A1/magazine socket frame against the
client's baked hierarchy, including grip orientation; moving the carrier leaves
the charging handle and other receiver-mounted controls fixed. Logical moving
controls conjugate their deltas through the same authored pivot as the renderer.
The rifle's carrier stays open when the authoritative action reports an open bolt.
The hip view's virtual shoulders are placed relative to the authored stock,
behind and below the breech. ADS still blends into the existing stock pocket;
the 30/28 cm arm segments and source hand contacts remain rigid.

Source/render fixtures exercise active local/remote contacts, stock and clothing,
reloads, ADS sight centring and independent physical-bore/device-outlet positions.
The shorter grip spacing and tilted pistol grip also change authoritative anatomy
and the captured ragdoll pose. The running/step regression required stronger
passive ankle damping (4.0 N m s/rad versus 2.5 for other joints); the same eight
floor/wall/corner/step cases retain the original 8–10 second rest limits.

## Static review

Reviewed by the OpenCode agent on 2026-11-15 at revision `7e220b7`, with the
extended stock, base 14.5-inch barrel/device, iron sights and 30-round magazine.
Both native runs used the same debug build, two 936×1007 Vulkan clients, world
FOV 70° / weapon FOV 65°, and the ordinary daylight scene on an RX 6600 XT.
Local artifacts: `target/tar24-materials-{near,far}/`; `peer/asset-*.png` are
unoccluded diagnostic views of the accepted world assembly. Equipped local and
remote action frames retain bodies and hand contacts.

| Category | Result and evidence |
| --- | --- |
| Shape/proportions | Accepted for the base asset. Both sides, front and top preserve barrel/stock endpoints, magazine contour and sight line. Longitudinal dimensions and stock travel also pass the independent source oracles. Front/top widths remain visual interpretations of F1/F2, not measured manufacturing data. |
| Materials | Accepted for the existing scene: lit receiver, steel, furniture and magazine have distinct responses; rubber and shadow-facing controls remain dark. The right-side image is useful for silhouette, but small recesses require the lit/close local views and source clearance checks. |
| Openings/mechanisms | Ejection port, aperture and magazine sweep pass independent geometry checks. Carrier/open-bolt motion is separate from the fixed charging handle; native empty/release frames and shared-pivot fixtures agree. |
| LOD | Accepted. Side/front/top pairs retain component placement, functional openings and overall silhouette. The top pair shows removal of cover ribs; deviation is bounded to 2 mm. Near/far preserve the ADS aperture. Far saves 224 triangles (11.9%) on the rifle; submission count is unchanged. |
| Perspective parity | Accepted for the base configuration: local hip/ADS and remote equipped views use the same parts and anchors. Both full UDP runs passed raid, injury/fall/recovery and anatomy/piece-ID convergence while the peer discarded every third snapshot. |

The final engineering gate passed 30 Python and 233 Rust tests, formatting,
workspace check and Clippy; both binaries built. Capture-time FPS is not a
performance benchmark. Reload choreography, player inspect, modular sight/zero
combinations and the motion/audio/performance slice gate retain their own
acceptance in TAR-29/30/31/32.
