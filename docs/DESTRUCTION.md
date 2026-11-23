# Material destruction

Interior wood panels and concrete cover share authoritative remaining-solid
geometry between server collision and client meshes. The floor, arena perimeter
and loot containers retain their existing behavior.

## Local damage and fracture

`AmmoDefinition::bore_diameter_mm` separates physical openings from impact decals.
Current rifle/SMG/pistol defaults are 6/9/11 mm. A bounded sequence of box cutters
follows the impact ray, including oblique incidence; there is no dense
millimetre-resolution grid. Concrete chips retain a backstop and stop their
impacting round. Wood uses the existing attenuated penetration budget, and the
round creating a hole still pays for the material it struck.

Spatial clusters accumulate actual incoming damage. Material-specific thresholds,
cluster radius, thickness, incidence and prior geometry control cavity growth.
Wood has elongated torn rims; concrete has more compact chips. These are tunable
game coefficients, not measured engineering properties. Existing internal box
seams are compacted without intentionally enlarging or healing the damage.
Growing cavities open their local rough mouth back to the cluster's first entry
plane, even when a later round strikes an interior cavity side. This prevents
larger released chips from being sealed behind an older narrow opening.

Larger newly removed material becomes scale-1 physical spall. Smaller losses are
accounted as fines, without claiming a dust simulation. Fixed material thresholds
start at 1 g/0.5 mm for wood and 2 g/1 mm for concrete; sheets at four times the
minimum mass remain physical even below the thickness threshold. These thresholds
do not depend on available debris capacity. Static material, fragment
volume and fines form the conservation oracle. An old cell-wide HP threshold no
longer causes failure; its health value reflects the remaining volume.
Entry-face chips eject toward the exposed side, with a bounded tangential
response, so shallow concrete spall can leave its preserved backstop.

Support is evaluated on surviving solid islands, including connections across
the old half-metre cell boundaries. Contact area and finite material compression
or shear capacity route the islands' gravity loads to the floor. Unsupported
material is partitioned into connected, nonuniform fragments. Bounded hit
impulses, contact lever arms, mass and density determine initial movement; Rapier
handles subsequent map/rubble contact. Debris neither damages players nor becomes
character-controller support. Cover persists across Arena respawns and resets
on lobby return or a fresh deployment, matching the existing map lifecycle.

## Geometry and transport limits

| Resource | Limit |
| --- | ---: |
| Accepted cuts | 128 |
| Static boxes per original cell | 384 |
| Static boxes in the scene | 1,024 |
| Temporary boxes during one subtraction | 6 × 384 |
| Active physical fragments | 64 |
| Frozen boxes per fragment | 16 |
| Frozen boxes across all fragments | 256 |
| Fragment lifetime | 20 s |
| Cover message envelope | 16,000 bytes |

Cut updates are transactional. If the accepted representation would exceed its
limits, retain the previous holes and collision. Pending fragments reserve body
and shape capacity; support release waits for capacity rather than deleting
healthy cover. Retry deferred release on a new geometry change or released
capacity, without repeatedly solving an unchanged saturated scene every tick.

Protocol 29 gives each fragment an independent ID and frozen local box geometry.
Later damage to its original cell does not reshape released debris. Full cover
updates use reliable unordered channel 5, a 512 KiB queue and 100 ms retry.
Sequence checks reject superseded states. Configuration, controls and shot
commits retain higher transport priority. Moving cover updates are capped at
30 Hz; unchanged state refreshes periodically. Validate shape/identity/geometry
limits before client publication. Both binaries must be rebuilt together.

## Verification

Run focused geometry, authoritative physics and mesh/sync tests through the
central runner, then the full workspace gate:

```sh
python scripts/verify.py --package common --filter destruction
python scripts/verify.py --package server --filter destruction
python scripts/verify.py --package client --filter destruction
python scripts/verify.py --mode full
```

The explicit real-UDP saturation gate uses actual loopback sockets and periodic
datagram loss, including solo, dual and late-join reconstruction alongside
ordinary controls/configuration. Its seeded full-shape fixture tests transport,
not the production impact path:

```sh
cargo test --locked -p server destruction_udp_full_fragment_state -- --ignored --nocapture
```

After headless checks, these opt-in native routes drive ordinary movement,
semi-automatic fire and reload input against each material. They launch a game
and take five named screenshots, without video:

```sh
python scripts/playtest.py --destruction wood --output target/playtest-destruction-wood
python scripts/playtest.py --destruction concrete --output target/playtest-destruction-concrete
```

Evidence waits for consumed ammunition, changed authoritative geometry, a real
fragment ID and observed movement/settling. These corner scenarios inspect local
bores/chips and irregular breakoff. They do not establish the appearance of a
whole collapsing wall or final artistic quality. Operational results and pending
acceptance belong in TAR-98.
