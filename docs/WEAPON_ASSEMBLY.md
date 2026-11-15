# Rigid weapon assembly

The pure resolver in `common/src/catalog/assembly/` extends the shared RON
registry (schema 2). The server and future customizer use the same rules.
Bundled content retains the three baseline magazines. The acceptance matrix
uses explicitly synthetic connectors; real model selection and integrated
geometry belong to the modular kit.

## Topology and compatibility

An attachment names a nonzero `u16` node, `DefinitionId`, optional parent node,
socket key and `u16` position. Parent `None` means the weapon root. Serialization
order is irrelevant. A root-connected tree allows at most **32 components** and
**8 levels**, excluding the weapon root. Cycles, duplicate nodes, orphans and
missing definitions fail before reduction.

- Interfaces match `InterfaceKind`, standard string and size code exactly.
  Size is a standard-specific nonzero integer code, not a dimension conversion.
  Distinct thread standards and sizes cannot substitute for each other.
- Socket `accepts` lists allowed component kinds. Contracts include magazine,
  stock, handguard, barrel, muzzle device, compensator, suppressor, scope, red
  dot, iron sight, grip and adapter.
- A component can restrict platform and caliber. `None` means universal for
  that dimension, not universal connector compatibility. Magazine ammunition
  must also match the weapon's current ammo definition.
- Rigid sockets have one position and zero pitch. Rails have 1–256 positions,
  pitch 0.001–0.1 m and occupied half-open intervals `[position, position+width)`.
  Adjacent intervals are legal; overlap and endpoints beyond the rail fail.
- An adapter is an ordinary component exposing child sockets. Mount/barrel
  chains follow real parent links; having an adapter elsewhere is insufficient.
- Required sockets must be occupied. `requires`/`excludes` refer to installed
  component definition IDs globally; exclusion is enforced from either part.
  Self/unknown/duplicate constraints, contradictions and required-dependency
  cycles or chains exceeding 32 parts are invalid content.
- Exactly one magazine definition is required in the resolved configuration.
  Mechanical state may temporarily have no inserted magazine after a removal
  commit; the configuration retains its last definition during that interval.

## Coordinates, anchors and conservative volumes

Use the [reference conventions](WEAPON_REFERENCES.md): +X right, +Y up, -Z
forward, metres, unit xyzw quaternion, rigid unit scale. New root is the breech
centre. A component's mount origin is its local origin; geometry and contacts
are relative to it. Rest placement is:

`parent * socket.pose * translation(0, 0, position * pitch_m)`

Rail travel is local +Z; rotate the socket to author another rail direction.
Transforms compose with Bevy `Transform`; local translations/centres/bounds
must be finite and within ±10 m. Quaternion squared length tolerance is 0.0001;
accepted rotations are normalized. Each geometry allows 16 sockets, 16 solids,
16 clearance boxes and one anchor per kind.

Root anchors are fallbacks, overridden by an installed component. Two installed
parts providing the same anchor kind are ambiguous and rejected. Keep these
semantics distinct:

- `Bore`: physical barrel muzzle used for authoritative projectile origin.
- `DeviceExit`: final suppressor/device outlet used by subsequent flash FX.
- `Sight`, `FrontIron`, `RearIron`: sight references; no automatic ADS calibration.
- `Ejection`, `TriggerGrip`, `SupportGrip`, `StockContact`, `MagazineGrip`:
  rigid rest contacts, including orientation.

Resolved anchors stay in breech-root space. Explicit `hip_from_root` and
`ads_from_root` bridges map a configured bore into the current eye-relative rig.
The shared muzzle helper interpolates those mapped points. With no authored
bore, the three baseline profile/WeaponRig offsets remain unchanged. Camera,
sight calibration and moving-part animation consume these contracts in their
respective integrations.

Solids are conservative non-contact boxes. Author deliberate mating volumes
outside those boxes; the resolver does not blindly exempt parent/child pairs.
Clearance boxes describe swept charging/reload/iron-line corridors. Transform
all eight corners into conservative root-space AABBs. Solids from different
parts must not overlap each other or another part's clearance. Boundary contact
is legal. A part does not conflict with its own boxes.

The obstruction envelope retains transformed solid boxes separately from bore
and outlet. These checks are **pre-validation**: rotated AABBs may overreject,
and omitted/mistaken volumes cannot certify geometry. Real-kit inspection must
still examine meshes and complete action trajectories with both perspectives.
Empty baseline volumes make no clipping or trajectory guarantee.

## Deterministic properties

Resolve topology in `(depth, node ID)` order, then reduce in ascending node ID.
Accumulate mass, moments and factors as `f64`, casting the accepted result once.
Permuting entries does not change the result.

| Property | Composition and units | Accepted limits |
| --- | --- | --- |
| Mass | root + installed parts, kg | root 0.01–30; each part >0 and ≤30; total ≤50 |
| Centre of mass | sum(mass × root-space centre) / total mass, m | each final axis ±10 |
| Balance | length of first moment about root, kg·m | ≤500 |
| Magazine | installed magazine's capacity, rounds | 1–1000 |
| Recoil / velocity / signature | product of explicit dimensionless factors; identity 1 | each 0.1–4; final 0.05–8 |
| Muzzle speed | ammo speed × velocity factor, m/s | 1–5000 |

Invalid compositions are rejected, never silently clamped. No percentage is
added to another percentage, and a “grip” label has no implicit recoil bonus.
Balance is a measured handling input, not an invented ergonomic score. Recoil
and signature factors are cached inputs for subsequent firing/FX integration.
Damage, penetration, cadence and reload time remain their existing profile/ammo
values; this resolver does not invent physical laws from part names.

## Atomic revisions and authoritative use

`EquippedWeapon::reconfigure(catalog, expected_revision, configuration)` accepts
only IDs/placements. It checks the revision and definition identity, canonicalizes
node order, resolves all constraints and then replaces configuration/cache
together. A failed change leaves identity, ammunition, revision and cached
geometry untouched. Identical configurations (including entry permutations)
return the current revision and reuse allocations. Each actual accepted change
increments the `u64` revision; overflow fails.

Removing a mount alone fails while its children remain. Explicit dependencies
also prevent orphaned logical configurations. A caller can submit a complete
valid removal of optional parts in one transaction. A magazine definition change
selects an already-owned compatible physical magazine; an unowned definition
fails. The previous magazine keeps its rounds in reserve and the chamber is
untouched. No physical magazine changes capacity or gains rounds.

`Arsenal::reconfigure` adds the manipulation lock and slot lookup. The server owns its
catalog and rederives properties; no client stats enter the transaction.
Ownership/lobby-ready checks and configuration requests are integrated with the
customizer; configuration replication uses the dedicated protocol work. The
protocol 11 snapshot carries dynamic mechanical state and a compact configuration
reference. [Reliable configuration replication](WEAPON_REPLICATION.md) resolves and
caches the complete tree at the receiver. `WeaponInstance` serialization includes
its revision, attachment nodes and physical magazine pool.

`spec()` returns the already-resolved cache: no catalog scan, tree traversal or
pairwise volume check runs each frame. Resolution may test bounded pairs once
per actual change, at most 32 parts × 16 boxes (plus root geometry).
Owned magazine variants are resolved on restore/reconfiguration. A reload that
seats a different definition selects that cache and increments the revision.
Revision exhaustion cancels such a manipulation before its configuration commit.
