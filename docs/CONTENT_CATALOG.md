# Shared content catalog

`common/data/catalog.ron` is the logical content source. `Catalog::parse` loads
RON using the version in the document; `Catalog::validate` rejects unsupported
versions before producing an immutable registry. The bundled registry is loaded
once with `catalog()`. The server does not load a mesh, texture or audio file.

## Identity

- Definition IDs are explicit nonzero `u32` values, unique across weapons,
  components, ammunition and profiles. Never derive them from array positions,
  display names or hashes. Keep existing IDs when renaming display text.
- Lowercase ASCII keys are also unique across those categories. `m4a1` is the
  single M4A1 identity; `Rifle` is a presentation/mechanism class.
- Instance IDs are nonzero `u64` values assigned by the owning simulation. Two
  instances of the same definition have independent ammunition and configuration.
- Equipment slots select an instance; they are not weapon identities.
- Instance IDs are allocated monotonically for the running simulation. Persisting
  a world must also persist its allocator high-water mark before restoring instances.
- General items extend this document and namespace. Inventory references the
  existing weapon instance instead of duplicating its ammunition state.

Protocol 11 inputs select `Primary`/`Sidearm` slots and sequence reload, inspect
and selector changes. Snapshots include the compact mechanical action/phase,
chamber, bolt, inserted-magazine, selector and equip state. Loadouts carry a stable
weapon definition ID; equipped snapshots carry definition/instance IDs and
confirmed shots carry only the definition ID. Repeating instance IDs and class
metadata in every recent shot exceeds the existing worst-case snapshot budget.
The legacy player class field remains a procedural presentation profile, checked
against the definition at the receive boundary. It never selects ammunition.
The lobby cycles registry primaries; new definitions need no selector match.
Versioned configuration travels reliably once per change; snapshots refer to an
epoch/handle. See [replication](WEAPON_REPLICATION.md) for cache and command bounds.
Local view templates are created on first equip rather than all at startup.

## Resolution

Schema 2 weapons reference a profile, ammunition and a bounded attachment tree.
Each attachment has a stable nonzero node ID, component definition, parent node
(or root), socket key and indexed rail position. Socket interfaces match kind,
standard **and** size code exactly. No compatibility is inferred from display
names. See [assembly contracts](WEAPON_ASSEMBLY.md) for transforms, constraints,
property reductions and revision transactions.

Resolution produces capacity, damage, penetration, cadence, reload duration,
velocity, mass, balance, modifiers, anchors and conservative obstruction bounds.
`EquippedWeapon` caches this result when equipped or successfully reconfigured.
The physical magazine pool, separate chamber, actions and configuration revision
belong to its `WeaponInstance`. Loaded/reserve counters are derived views; see
[mechanical state](WEAPON_MECHANICS.md).

The initial values preserve TK's existing combat rules, including the temporary
17-round pistol. These are migration baselines, not claims of real-world
fidelity. See [weapon references](WEAPON_REFERENCES.md) for the intended targets.
The bundled root masses and magazine interfaces are **G migration placeholders**,
not manufacturer-certified kit masses. Baseline roots still contain the entire
legacy assembly; replacing a baseline with a decomposed kit must subtract the
removed parts from root mass.

Visual mesh construction, palettes, animation tracks and sound resources belong
to the client. The shared bore offset remains logical because authoritative
projectile spawning and cover clearance require it. The former unused
`WeaponSpec` model scale/colour/offset fields are removed.

## Validation and diagnostics

Invalid IDs/keys, references, versions, capacities, non-finite values and
non-positive physical properties fail with a source filename and field path.
Signed spatial coordinates are allowed; they must be finite. Typed decode
errors also retain their field path, including negative unsigned values.
Unknown definition lookup returns `None`/`Err`, never a substituted base weapon.
Network boundaries must validate IDs before using the convenience `spec()`
lookup for known content.

RON and Serde were already transitive dependencies. They are direct workspace
dependencies for this document; `serde_path_to_error` preserves diagnostic
paths. Runtime network serialization still uses `common::data::{encode,decode}`
and rkyv, not RON.
