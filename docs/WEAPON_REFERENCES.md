# Weapon reference and authoring contract

Revision 1. Reference review: 2026-11-15. Units below are metres unless marked.
This is the source contract for the M4A1 and M1911 slices, not artistic acceptance
of the current cuboid models. Integration evidence belongs in Linear TAR-22;
asset and in-game acceptance belong in TAR-24/35 and TAR-32/36.

## Identity and evidence classes

| Base identity | Display / presentation class | Selected real reference |
| --- | --- | --- |
| `m4a1` | M4A1 / Rifle | FN military M4A1, 5.56×45 NATO, 14.5-inch barrel, collapsible stock |
| `m1911` | M1911 target / Pistol | Colt 1911 Classic Government O1911C, Series 70, blued carbon steel, .45 ACP, 5-inch barrel |
| `mp5` | MP5 target / Smg | Existing presentation; platform qualification is separate |

`ResolvedWeapon::catalog_key` identifies the current base model; `WeaponId` is
its stable network identity and `WeaponKind` its procedural presentation class.
Display text is never a network key. The rifle has one identity, with no alternate
public alias. The [shared registry](CONTENT_CATALOG.md) preserves current gameplay
before the fidelity changes.

Every authored property must identify its evidence class:

- **F — published fact:** primary source, variant, unit and page/section.
- **V — visual interpretation:** exterior shape/material inferred from a cited
  image; not an engineering measurement.
- **G — game choice:** authored curve, tolerance, balance value or presentation
  setting. Never describe a G value as manufacturer data.

## Primary references

| ID | Source and reviewed content | What it supports |
| --- | --- | --- |
| F1 | [FN M4A1 product sheet](https://fnamerica.com/products/rifles/fn-m4a1/), Specs and description | Calibre, barrel, overall lengths, magazine, operation, cyclic rate, listed weight |
| F2 | FN gallery [right](https://fnamerica.wpenginepowered.com/wp-content/uploads/2016/09/FN_M4A1_Rotators_1-1800x750.jpg) / [left](https://fnamerica.wpenginepowered.com/wp-content/uploads/2016/09/FN_M4A1_Rotators_2-1800x750.jpg) | Exterior silhouette, asymmetric controls, railed handguard with covers, stock and magazine |
| C1 | [Colt Classic product page](https://www.colt.com/detail-page/classic/) and [2026 commercial catalog](https://www.colt.com/wp-content/uploads/2026/02/26-COLT-0200-COLLATERAL-Commercial_Catalog-RND2-FINAL-Flipbook.pdf), “1911 Classic” | O1911C, blued carbon steel, Series 70, 5-inch National Match barrel |
| C2 | [Colt Government Model MKIV/Series 70 Model O manual](https://www.colt.com/wp-content/uploads/2023/02/governmentmodelmkivseries70modelo.pdf), printed pp. 4, 19–20, 47–48 | .45 ACP, 5-inch barrel, 7-round magazine, 6.5-inch sight radius; linked short-recoil action and named moving parts |
| C3 | [Colt Classic exterior image](https://www.colt.com/wp-content/uploads/2022/12/1911-classic-.45.jpg) | Blued slide/frame, wood double-diamond panels, spur hammer, fixed sights, short trigger |
| C4 | [Colt 7-round magazine 53355B](https://shop.colt.com/45-acp-government-commander-blue-carbon-7-round-magazine) | .45 ACP Government/Commander fit and blued carbon-steel finish |
| C5 | [Colt 8-round magazine 54926B](https://shop.colt.com/45-acp-government-commander-blue-carbon-8-round-magazine) | Explicit fit for Government 5-inch and Commander 4.25-inch .45 ACP models |
| C6 | [Wilson Combat 47D](https://wilsoncombat.com/magazine-1911-45-acp-full-size-8-round-standard-base-pad) | Optional full-size single-stack .45 ACP, 8 rounds, stainless tube and 0.350-inch polymer base pad |

The Colt product page combines multiple variants. Its generic image alt text
mentions a firing-pin safety; C2 explicitly says the Series 70 Model O does
**not** have that mechanism. Use the variant-specific manual, not the shared
alt text or a retailer's mixed specification. A Government magazine's verified
fit does not imply compact Officer/Defender or different-calibre compatibility.

Reference photos/manuals remain linked to their owners, not redistributed as
game assets. Original meshes, palettes, curves and schematics are TK-authored.
No extracted game assets, textures, motion curves or audio are a source.

## Variant sheets

### M4A1

The [modular kit sheet](weapons/m4a1-kit.md) selects the TAR-26 components,
primary sources and compatibility matrix, with production acceptance tracked
separately from reference selection.

| Property | Target / provenance |
| --- | --- |
| Barrel length | **F1:** 14.5 inches = 0.36830; this is barrel length, not total visible forward assembly |
| Overall length | **F1:** 29.75–33 inches = 0.75565–0.83820, stock retracted/extended |
| Listed weight | **F1:** 6.36 lb ≈ 2.885 kg; page does not specify every attached accessory, so do not treat it as an assembled kit mass |
| Magazine / chamber | **F1:** 30-round magazine; **G:** independent single chamber state, capacity never includes that chamber |
| Operating family | **F1:** gas/direct impingement, safe/semi/automatic; no three-shot burst substituted for auto |
| Cyclic rate | **F1:** 700–950 RPM; **G proposal:** 800 RPM for the faithful slice, reviewed in the recoil/mechanical cards |
| Finish | **V, F2:** dark receiver/rail, matte black stock/grip/covers, darker steel barrel and muzzle device; avoid one uniform glossy black material |
| Sight configuration | **V, F2:** fixed front sight tower and rear aperture on the top rail; use the pictured rear assembly, not a mixture of incompatible upper-receiver references |

The current rifle is 600 RPM, 30 rounds, 2.4-second reload, with class-tuned
damage/velocity. Naming it M4A1 does not claim those numbers or the existing
oversized geometry are faithful. Changes to them need the mechanical/firing
cards and acceptance, not a silent rename-side effect.

### M1911

| Property | Target / provenance |
| --- | --- |
| Variant | **C1:** O1911C Government, .45 ACP, Series 70, blued finish |
| Barrel / sight radius | **C2:** 5 inches = 0.12700 / 6.5 inches = 0.16510 |
| Overall envelope | **V:** approximately 0.216 long × 0.14 high × 0.035 wide, exterior authoring estimate; C1/C2 do not publish a complete orthographic envelope |
| Base magazine | **C2/C4:** 7 rounds; **G:** separate chamber permits 7+1, never 17 in a single-stack magazine |
| Alternate magazines | **C5:** 54926B, 8 rounds; **C6:** 47D, 8 rounds with visibly distinct base pad |
| Mechanism | **C2:** single-action, linked short recoil; slide and barrel initially move together before unlocking, slide continues, hammer is cocked and slide returns |
| Finish / shape | **V, C3:** blue-black steel with different flats/edges, brown wood panels, short trigger, spur hammer, fixed front blade/rear notch |
| Fire cadence | Semi-automatic; **G:** current 400 RPM is an input/game cap, not a manufacturer cyclic-rate claim |

Do not merge Commander length, a rail-bearing frame, Series 80 internals or
double-stack capacity into this reference. Existing 17-round gameplay is
explicit migration debt owned by the mechanical M1911 delivery.

## Views and geometric uncertainty

The [M4A1](weapons/m4a1-reference.svg) and [M1911](weapons/m1911-reference.svg)
sheets provide original side/front/top authoring schematics. Side shapes are
interpreted from F2/C3. Front/top are **V reconstructions**, not manufacturer
orthographic scans. Grey masses distinguish rigid groups; the dashed line is
the bore. They are not meshes, manufacturing drawings or final asset approval.

| View | M4A1 checks | M1911 checks |
| --- | --- | --- |
| Both sides | Overall length/stock endpoints; curved magazine; separate upper/lower; ejection right, bolt catch left; handguard/rail proportions | Slide over frame; angled wood grip; short trigger/guard; hammer behind slide; ejection right, controls left |
| Front | Bore below sight line; open front-sight guard; handguard surrounds barrel; no filled-in aperture | Muzzle inside bushing/slide face; front blade above bore; recoil-spring housing below; no solid block filling every recess |
| Top | Barrel, receiver and stock axes aligned; separated rail teeth; right ejection opening; rear charging-handle silhouette | Slide side planes/serrations; centred front/rear sights; right ejection opening; barrel visible when slide retracts |

Published longitudinal dimensions are hard targets. Widths, receiver heights,
fine controls and hidden recesses remain labelled V until the corresponding
asset author qualifies them with additional views. Do not scale a perspective
photo as if it were a calibrated orthographic drawing. Record the chosen stock
position and included muzzle device before comparing silhouettes.

## Coordinate and assembly contract

- Right-handed local coordinates: +X right, +Y up, **-Z forward**. Metres,
  radians, normalized quaternion `[x, y, z, w]`; transforms must be finite.
- New asset root: breech centre on the bore axis at rest. Existing WeaponRig
  offsets use a presentation origin; migrate via an explicit root transform,
  not per-part rounding or changes to the authoritative shot ray.
- Piece vertices are local to their authored pivot; child transform maps into
  the parent's coordinates. Bind transforms and anchor transforms retain full
  floating-point precision even when surface occupancy is voxelized.
- `receiver/frame` is root. M4 barrel, stock interface, handguard, grip,
  magazine and sights attach through typed sockets; bolt/carrier, catch and
  charging handle have independent tracks. On M1911 the slide and barrel
  have distinct tracks; hammer, magazine and slide stop are distinct pieces.
- Discrete content/configuration drives assembly. Simulation owns ammo/actions;
  a presentation curve does not move ammunition or authorise a shot.

| Anchor | Parent and convention |
| --- | --- |
| `bore.breech`, `bore.muzzle` | Barrel; define physical axis and exit, forward -Z |
| `muzzle_device.mount`, `muzzle_device.exit` | Barrel/device; preserve both physical muzzle and final device outlet |
| `ejection` | Receiver (M4), moving slide (M1911); orientation follows the actual opening, never a camera-relative offset |
| `magazine.seat`, `magazine.grasp` | Receiver/frame socket and magazine-local hand contact; insertion track is separate from rest pose |
| `grip.trigger`, `grip.support` | Trigger grip and configured handguard/foregrip; pose contact includes palm orientation |
| `sight.front`, `sight.rear`, `sight.eye` | Installed sight parts and configuration; irons define a line, optics additionally define eye-relief interval |
| `stock.mount`, `stock.shoulder` | Receiver interface and stock; shoulder point follows extension |
| `control.bolt_catch`, `control.charge` | M4 controls; separate pivot/contact from the hand trajectory |
| `control.slide_stop`, `control.slide_grasp`, `control.hammer` | M1911 frame/slide; hammer rotation and slide translation use their own pivots |

Changing a part recomputes anchors/properties once. Both perspectives use the
same assembly; only documented camera placement and LOD differ. No entity per
voxel, no per-frame mesh rebuild, no full attachment tree in each snapshot.

## Mechanical and timing contract

F1 describes operation and a cyclic-rate range; C2 establishes the order of
moving parts and feeding. Neither gives a universal human reload duration.
Current `reload_action` phase landmarks are **G**, not motion-captured evidence.
Keep these categories separate when authoring:

| Event | Required relationship | Initial G review envelope |
| --- | --- | --- |
| M4 shot | Confirmed shot, carrier cycle, ejection and recoil share event identity; charging handle does not reciprocate on ordinary fire | Cycle fits selected RPM interval; animation cannot emit extra shots |
| M1911 shot | Slide/barrel coupled initially, barrel unlocks, slide continues and cocks hammer, return feeds; no empty-case ejection on a dry trigger | No invented automatic cadence; frame-dependent visual phase never grants ammo |
| Tactical magazine change | Loaded chamber retained; remove/seat magazines; finish contact, no unnecessary charging action | M4 2.4 s / pistol 1.5 s are current G baseline, not validated final timing |
| Empty reload | Action locked open, magazine seats, visible control/slide manipulation closes action and chambers once | Distinct final phase; do not reuse tactical duration as a factual empty-reload measurement |
| Interrupt / swap / fall | Displayed pose blends from the actual last pose; committed server phases remain committed | Test interruption before/after every ammo-changing phase |
| Inspect / equip | Hands maintain reach and mass cues; cannot reveal another weapon's magazine or action | Original curves; review at normal speed in both perspectives |

For later timing evidence, record source, subject/variant, action type,
timestamps and uncertainty; compare relative phase order before total duration.
Manufacturer cyclic rate does not establish reload speed. Authored Foley must
track the same event phases and have recorded provenance/licence.

## Resolution comparison and tolerances

Choose **2 mm local cells** for near receiver/furniture surfaces, with **1 mm**
detail islands for narrow sights/controls as needed. **4 mm** is a candidate
coarser LOD, not a blanket grid for the whole weapon. Pivot coordinates, overall
dimensions, contact anchors and sight axes are never snapped.

Two-grid comparison: an 8 mm feature (the current rifle front-blade width in
`client/src/render/weapons.rs`) spans 4 cells at 2 mm, only 2 at 4 mm. A
368.30 mm barrel spans 184.15 versus 92.075 cells; a 127 mm barrel spans 63.5
versus 31.75. Preserve those lengths with local bounds/partial boundary cells,
not an integer-cell rescale. Nearest-grid centre error is at most
`sqrt(3) * cell / 2`: 1.732 versus 3.464 mm.

Using the client's real vertical projection settings (`render/mod.rs`),
`pixels = height * size / (2 * depth * tan(vertical_fov / 2))`:

| Projection / sample depth | 2 mm step (px) | 4 mm step (px) |
| --- | ---: | ---: |
| 720p, view-model 65°, 0.5 m | 2.260 | 4.521 |
| 1080p, view-model 65°, 0.5 m | 3.391 | 6.781 |
| 1440p, view-model 65°, 0.5 m | 4.521 | 9.041 |
| 1080p, world 70°, 5 m | 0.308 | 0.617 |
| 1080p, aimed primary world 44°, 5 m | 0.535 | 1.069 |

These are perspective projections at explicit sample depths, not GPU captures
or a claim of perceived quality. The 4 mm grid is visibly coarse near the eye
and can erase narrow controls; distant surfaces can use it while retaining
silhouette and continuous socket positions. Test actual projected vertices at
all poses before accepting the LOD.

```python
from math import radians, tan
for height, fov, depth in [(720, 65, .5), (1080, 65, .5),
                           (1440, 65, .5), (1080, 70, 5), (1080, 44, 5)]:
    print(height, fov, depth,
          [round(height * cell / (2 * depth * tan(radians(fov / 2))), 3)
           for cell in (.002, .004)])
```

Authored acceptance tolerances (**G**, not manufacturing tolerances):

- Published overall/barrel dimensions: ±1 mm metadata, unchanged between LODs;
  visible boundary displacement ≤ one local cell against the authored surface.
- Static socket seams: ≤1 mm / 0.5°; no visible open seam in close inspection.
- Hand contacts: ≤3 mm to authored contact surface, with no rigid-part clipping
  across sampled action phases. This allows surface quantization, not palm drift.
- Sight axes: ≤0.25 mm lateral anchor error and ≤1 px projected alignment at
  1080p ADS; use continuous anchors if a cell grid cannot meet the pixel limit.
- Compare centre/near-plane/frustum at 720p, 800p, 1080p, 1440p, ultrawide and
  HiDPI, world FOV 20/70/160°, weapon FOV 65°. Art must fit the actual final
  camera, not a substitute wide lens used only to pass an image check.

## Rubric for the asset and integration gates

Each category requires evidence and a reviewer result; “build passed” is not
a score. Record revision, configured parts, resolution, camera and scene.

| Category | Scenes and failure conditions |
| --- | --- |
| Silhouette / proportion | Both sides/front/top, stock endpoints, bare and equipped; compare F dimensions and V outlines separately |
| Materials | Neutral daylight and dim scene; wood, polymer and metal readable without painted-on highlights or copied textures |
| Mechanical readability | Tactical/empty/dry shot, mid-action interrupt, empty magazine lock-open; each moving part follows its own mechanism |
| Contact / weight / timing | Slow sampling for contacts, normal-speed observation for weight; no floating glove, stretching arm, teleport or generic rifle reload on pistol |
| ADS | Irons/red dot, FOV extremes, close cover; sight line and authoritative bore remain coherent, device exit does not bypass cover |
| Perspective parity | Local, remote and inspection show identical installed parts, chamber/magazine state and event sequence |
| Extreme configurations | Short/long magazine, stock endpoints, foregrip, muzzle devices, tall/low sight combination; explicit incompatibility instead of clipping |
| Audio / effects | Original/licensed sources, once per authorised event, aligned controls/impact; silent or rejected action produces no invented firing event |
| Runtime quality | Shared caches/LODs, no internal faces where removable, bounded effects; measure matched load without capture overhead |

Asset gates must render and compare the 2/4 mm candidates on the actual game
camera and qualify the inferred front/top geometry. Integration gates add real
UDP and human review of motion/audio. Screenshots suffice for static review;
recording remains opt-in. This reference delivery does not certify those gates.
