# M4A1 modular kit

Reference selection for TAR-26, reviewed 2026-11-15. Production and acceptance
are pending. This sheet extends the [F/V/G contract](../WEAPON_REFERENCES.md):
**F** is a published manufacturer fact, **V** is interpreted exterior geometry,
and **G** is an authored game value. Images remain linked to their owners.

## Selected models and sources

| Component | Reference and published constraints | Authoring boundary |
| --- | --- | --- |
| Base magazine | FN M4A1 [F1/F2](../WEAPON_REFERENCES.md): 30-round military magazine | Reuse definition 101 and `m4a1_magazine_30`; aluminum body/ribs/follower are V. Do not invent an OEM or NSN from the photograph. |
| Short magazine | [Magpul PMAG 20 AR/M4 GEN M3, MAG560](https://magpul.com/pmag-20-ar-m4-gen-m3.html): 20 rounds, 5.56×45/.223, STANAG-compatible, 4 oz, maximum length 5.6 inches | F: 0.113398 kg, 0.14224 m. V: polymer ribs, flared floorplate and over-insertion stop. Share the seat interface; author a distinct grasp on its shorter body. |
| Base stock | FN F1/F2 M4 collapsible stock | Reuse accepted shape and 0.08255 m travel. Separate the stock from the fixed receiver extension. Isolated stock mass is G; FN lists a whole-weapon mass. |
| Alternate stock | [Magpul CTR Mil-Spec, MAG310](https://magpul.com/ctr-carbine-stock-mil-spec.html): mil-spec extension only; 8.9 oz without extension; 7.0 × 1.7 × 5.2 inches maximum; approximately 3.3 inches LOP adjustment | F: 0.252311 kg, 0.1778 × 0.04318 × 0.13208 m. V: A-frame opening, latch, friction lock, QD recess and rubber pad. Keep extension mass on the receiver. |
| Base handguard | [KAC M4 RAS 98064](https://www.knightarmco.com/13129/shop/hand-guards/rasris/m4-ras-forend-assembly): carbine gas, delta ring, fixed front sight/gas block and round cap; M1913 rails; M4/Government contour preferred | Current page: 6.25 inches, 8.9 oz, aluminum. Qualify the TAR-24 interpreted handguard against this named reference before calling it a dimensional reproduction. |
| Alternate handguard | [Magpul MOE SL carbine, MAG538](https://magpul.com/moe-sl-hand-guard-carbine-length-ar15-m4.html): carbine gas, A2 front sight, round cap and delta ring; M-LOK at 2/6/10 o'clock; original front sling swivel incompatible | F: maximum length 8.9 inches, mounting length 6.6 inches, width 1.8 inches, height 2.4 inches, 6.7 oz. V: narrow polymer shell, front extension and vent/slot openings. |
| Base barrel group | FN F1/F2 M4A1, 14.5 inches | Preserve the 0.36830 m bore and fixed front sight. Separate device geometry. Isolated group mass and hidden geometry are G/V, not inferred by dividing FN's total weight. |
| Short barrel group | [Daniel Defense 10.3-inch Government, with FSB](https://danieldefense.com/10-3-5-56mm-carbine-1-7-government-profile.html): 5.56 NATO, carbine gas, A4 extension, 1/2×28 muzzle, phosphate exterior/chrome bore | F: 0.26162 m, 1.46 lb = 0.662245 kg **with FSB**. Do not use the 1.15 lb stripped or 1.23 lb low-profile-gas-block mass for this asset. Exterior/profile remains V. |
| Base device | FN F2 A2-style flash hider | Reuse the accepted cavity and outlet. Exact modeled extension and isolated mass remain V/G; it is not a SOCOM adapter. |
| Brake / suppressor mount | [SureFire SFMB-556-1/2-28](https://www.surefire.com/socom-muzzle-brake/): 5.56, 1/2×28, SOCOM Fast-Attach; 2.6 inches, 110.5 g | F: 0.06604 m, 0.1105 kg. V: external ports and mounting collar. This single component is both brake/compensator and the RC2 mount. |
| Suppressor | [SureFire SOCOM556-RC2](https://www.surefire.com/socom556-rc2/): SOCOM adapters; 5.56/.223; minimum 10-inch barrel; 6.4 inches long, 1.5-inch diameter, 482 g | F: 0.16256 m, 0.0381 m diameter, 0.482 kg. Published added length is approximately 3.8 inches and explicitly adapter-dependent. Author mating depth as V and check the final outlet; do not add the full can length to the adapter outlet. |
| Irons | FN F2 fixed front tower and rail-mounted rear aperture | Reuse the open TAR-24 sight line. The front tower belongs to the barrel group; the rear aperture is a removable rail component. Its exact commercial identity and isolated mass are not published by F2. |
| Red dot | [Aimpoint PRO 12841](https://www.aimpoint.com/products/pro-patrol-rifle-optic-2-moa-red-dot-reflex-sight-with-standard-spacer-and-qrp2-mount): 2 MOA, 1×, unlimited eye relief, QRP2 + ring + standard spacer | F: axis 39 mm above mounting interface; published kit envelope 132 × 72 × 81 mm. V: open tubular housing, adjustment/battery housings and folded lens caps. Functional reticle/zero acceptance belongs to TAR-31. |
| Optic mount | Same Aimpoint 12841 kit: QRP2, base/top ring and standard AR spacer | One separate mount component carries the real ring and spacer. The PRO requires this mount in the kit; direct rail installation fails. Do not count kit mass again on the optic. |
| Vertical grip | [Magpul RVG, MAG412](https://magpul.com/rvg-rail-vertical-grip.html): M1913 Picatinny, 3.5 inches tall, 2.6 oz with hardware | F: 0.0889 m, 0.073709 kg. V: textured polymer, narrow mount and distinct vertical palm contact. |
| Angled grip | [Magpul AFG-2, MAG414](https://magpul.com/afg-2-angled-fore-grip.html): Picatinny/STANAG4694, recommended minimum 3-inch rail; 4.70 inches long, 1.94 inches below rail, 1.37 inches wide, 2.53 oz | F: 0.11938 × 0.034798 × 0.049276 m, 0.071724 kg. V: open triangular profile, finger shelf and angled palm contact. |

### Source disagreements and choices

- The KAC current page and 2016/2017 catalogs give 6.25 inches / 8.9 oz;
  its 2014 catalog gives 6.625 inches / 11.6 oz for the same part number.
  Use the current page's handguard body reference. Rail panels and retaining
  hardware need explicit accounting. TAR-24's 174 mm interpreted handguard shell
  was replaced with a 158.75 mm body, open upper/lower rails, side vents and
  shortened side panels. The receiver now carries the delta ring; the handguard
  mounts at Z=-22 mm. Its support contact remains at the accepted Z=-110 mm.
  Width, vent stations, panel contours and the mounting station remain V, not
  manufacturer dimensions. The named RAS still needs native seam/silhouette review.
- Aimpoint's international page lists 192 g sight-only and 360 g with mount in
  the highlights, but 357 g in its specification table. The US page lists
  220 g sight-only / 366 g kit. Select the international 12841 geometry and
  **G mass split of 0.192 kg optic + 0.168 kg mount**. The selected 0.360 kg total
  follows that page's highlights; the split is not a separately weighed mount.
- SureFire's centimetre values are rounded. Use the published inch dimensions
  converted to metres, with source precision documented; do not treat 16.2 cm
  and 6.4 inches as contradictory engineering measurements.
- The preliminary Colt 11.5-inch candidate is superseded by DD's 10.3-inch
  **FSB** option, whose primary page specifies the selected group and mass.
  This is a configured M4-family kit, not a claim to reproduce a factory FN
  short-barrel model or a complete Mk18.
- MOE SL is selected over a second quad rail for its distinct polymer silhouette
  and M-LOK interface. RVG/AFG-2 attach directly only to the RAS in this kit.
  The supported SL configuration uses its handguard contact. Neither a hidden
  adapter nor a universal Picatinny/M-LOK compatibility exception is allowed.

## Assembly and property contract

The receiver retains receiver controls, pistol grip and fixed extension. Reuse
its accepted geometry when extracting stock, handguard, barrel/device and rear
sight into components. Preserve continuous socket transforms and moving control
names; a new component must not leave a second copy inside the receiver.

The source assembly declares seven component exports: `m4a1_receiver`,
`m4a1_barrel_145`, `m4a1_a2`, `m4a1_ras`, `m4a1_stock` and
`m4a1_rear_iron`, plus `m4a1_front_iron`. The shared front tower is a required
barrel child for both carbine-gas groups; the DD group mass includes it.
Its V retaining cap joins the front of the RAS and fits inside the SL shell.
The baker derives them from the existing parts, resets each
export root to identity, and retains its children, materials and socket frames.
No source geometry is copied into independently editable component files.
The complete `m4a1_refined` bake remains an authoring/reference fixture; the
modular runtime will bind the component exports. Tests require a disjoint,
complete partition and reconstruct every original surface at its source pivot.
The generated exports carry the master source hash, so source edits invalidate
the related bakes together. The RAS length oracle follows the current KAC page.

The shared magazine source remains `m4a1_magazine_30`, definition 101. TAR-59 and
TAR-87 consume this same origin; no alternate inventory mesh or magazine-ammo
state is introduced here. Capacity is 30 or 20 according to the installed
definition, excluding the independently modeled chamber.

`m4a1_pmag_20.json` is the authored empty PMAG source, with a recessed follower,
open feed lips, left catch opening, stepped lower body, ribs and flared base.
Its V interpretation uses the manufacturer's [side view][pmag-side] and
[oblique view][pmag-oblique]; loaded cartridges in the reference photograph are
not baked into the empty magazine. The continuous length is 0.14224 m.
Near/far share 816 vertices / 408 triangles, 3 submissions and 24480 cached
mesh bytes. Source tests cover its full magwell removal sweep and palm contact.
Catalog integration and native visual acceptance are pending.

[pmag-side]: https://magpul.com/media/catalog/product/cache/f8158826193ba5faa8b862a9bd1eb9e9/m/a/mag560-blk_magpul_pmag_20_ar_m4_gen_m3_02.1.jpg
[pmag-oblique]: https://magpul.com/media/catalog/product/cache/f8158826193ba5faa8b862a9bd1eb9e9/m/a/mag560-blk_magpul_pmag_20_ar_m4_gen_m3_01.1.jpg

The alternate furniture sources use the manufacturer galleries linked above.
CTR preserves the open extension sleeve and A-frame, with a separate rubber
pad and hardware. MOE SL preserves the barrel channel, side vents and lower
front extension. RVG and AFG-2 have separate rail jaws, hardware and authored
surface contacts; AFG-2's triangle is open geometry. All four use the same
near/far surfaces at this size. Their published envelopes and open spaces are
checked independently of the mesher; native silhouette review is still pending.

| Source | Vertices / triangles | Submissions | Cached mesh bytes |
| --- | --- | --- | --- |
| `m4a1_ctr` | 580 / 290 | 3 | 17400 |
| `m4a1_moe_sl` | 852 / 426 | 1 | 25560 |
| `m4a1_rvg` | 376 / 188 | 2 | 11280 |
| `m4a1_afg2` | 512 / 256 | 2 | 15360 |

The muzzle sources use SureFire's product photographs (SFMB-556-1-2-28-BK-isoL
and SOCOM556-RC2-BK-isoL). Both retain open outlets; the brake has two open lateral
ports. The RC2 surrounds its separate SFMB with no intersecting source surfaces.
The selected V mating depth gives 96.52 mm added length relative to the SFMB,
rather than adding the can's entire length beyond the brake. This exterior
model contains no internal suppressor structure.

The DD source uses the manufacturer's published variant specifications. Its
external stepped profile is interpreted from [the product photo hosted by
Bauer Precision][dd-photo]; the stripped photograph qualifies the profile only,
not the selected FSB variant's mass. The front tower reuses the F2-interpreted
source and carbine station. No hidden manufacturer's geometry is asserted.

The PRO and QRP2 sources use [Aimpoint's oblique product image][pro-photo].
They have separate meshes, a hollow 30 mm ring interface, spacer openings and
a 39 mm optical axis above the rail. Source checks require an unobstructed
20 mm square sight corridor and nonintersecting mount/optic surfaces. The
132 mm length and 72 mm combined width follow the published kit; folded cap
poses are V and do not claim the closed-cap 81 mm height. Glass/reticle rendering
and sight zero remain TAR-31 work.

| Source | Vertices / triangles | Submissions | Cached mesh bytes |
| --- | --- | --- | --- |
| `m4a1_dd_103` (without shared tower) | 584 / 292 | 1 | 17520 |
| `m4a1_sfmb` | 752 / 376 | 1 | 22560 |
| `m4a1_rc2` | 588 / 294 | 2 | 17640 |
| `m4a1_pro` | 992 / 496 | 2 | 29760 |
| `m4a1_qrp2` | 608 / 304 | 2 | 18240 |
| `m4a1_ras` (with side panels) | 1740 / 870 | 2 | 53640 |
| `m4a1_front_iron` (shared tower/cap) | 372 / 186 | 1 | 11160 |

[dd-photo]: https://cdn11.bigcommerce.com/s-50c7cl/images/stencil/960w/products/1447/5106/DD07-077-16176-018_2_HR__25050.1794261413.jpg?c=2
[pro-photo]: https://ms.aimpoint.com/products/s--gkExpjWt--/ipbacsoa75yyp5ridbey.png?width=960&height=960

Use typed extension, barrel, handguard, muzzle, optic-mount and magazine
interfaces. Required sockets enforce the barrel/device/furniture chain.
Picatinny placements use bounded intervals; solids and swept corridors qualify
placement. Manufacturer compatibility statements establish eligible interfaces,
not automatic acceptance of every placement or an exemption from geometry.

Semantic anchors inherit down an attachment chain: a foregrip refines its
handguard's support contact and a suppressor refines its mount's device exit.
The deepest provider wins regardless of node ID or packet ordering. Competing
providers on separate branches remain an ambiguity error. Removing a child
restores its ancestor's anchor without leaving a stale contact or outlet.

Mass includes each physical group once. Subtract extracted groups from the
baseline root before adding components. Label estimated base-group masses,
centres of mass and recoil/velocity/signature modifiers G. Keep the existing
base total as an explicit migration target until a separately sourced mass
budget replaces it. No device makes ammunition more damaging by name.

Stock endpoints, magazine seating/grasp, support contact, bore and final outlet
follow installed parts. The receiver's charging-handle sweep is fixed; moving
an optic rearward must not move that corridor out of the way. Intentional mating
regions are excluded from non-contact solids, with the exclusion explained.

## Acceptance matrix

These are required scenarios, not results. All use the same catalog resolver,
logical rigs and client bindings as gameplay.

| Scenario | Observable outcome |
| --- | --- |
| Base | 14.5-inch group, original stock/RAS/A2/irons, magazine30; preserve published length and accepted contacts within the reference tolerances. |
| Compact | DD 10.3-inch FSB, CTR retracted, magazine20; shorter bore/overall envelope and distinct palm target; qualify SL and RAS separately. |
| Long/suppressed | 14.5-inch group, extended stock, SFMB + RC2, magazine30 and PRO/QRP2; device exit and mass/balance include all installed components once. |
| Grips | RVG and AFG-2 at valid RAS positions; support palm follows each surface. Both magazines clear the magwell and the nearby grip through removal/insertion. |
| Stock endpoints | Both stock models at their bounded retracted/extended positions; shoulder contact moves, receiver/charging-handle corridor stays fixed. |
| Rail rejection | RVG and AFG-2 occupying intersecting intervals on the same rail are rejected; moving beyond available rail also fails. |
| Interface rejection | RVG/AFG-2 directly on SL's M-LOK, PRO directly on receiver rail, RC2 on A2 or bare muzzle all fail through interface/socket rules. |
| Clearance rejection | An otherwise rail-compatible optic placement occupying the charging-handle sweep fails geometrically, independently of rail occupancy. |
| Combined extremes | Cross-check both barrels, handguards, magazines, stock endpoints and valid muzzle chains. Record any measured interference as a rejected configuration, never a silent clipping allowance. |

Source geometry already identifies one rejected extreme: DD 10.3 + MOE SL +
SFMB/RC2. The SL's extended lower nose reaches Z=-248.06 mm while the can's
rear begins at -243.97 mm; their outer surfaces intersect over 4.09 mm
longitudinally. RAS clears the same short suppressed chain. Both handguards
clear the shared front tower/cap. The runtime matrix must reject the SL/can
interference through transformed bounds, not through a named combination rule.

Headless evidence must include dimension/shape and magazine-sweep oracles,
valid/invalid catalog configurations, resolved mass/anchors and actual local/
remote binding/contact fixtures. Native review checks silhouettes, material
separation, open sight lines and seams on the actual camera. Existing action
sampling checks static contacts; normal-speed reload/handling and final optical
acceptance remain explicitly tied to TAR-29/31/32. No production or artistic
acceptance is asserted by this reference selection.
