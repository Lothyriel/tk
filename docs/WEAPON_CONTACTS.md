# Configured weapon contact solver

`common::humanoid::handling` provides a shared, renderer-independent contract for
weapon-root contacts and two-bone arms. Distances are metres; poses are rigid,
right-handed, +Y up / −Z forward, expressed in a caller-selected frame.

## Contacts and ownership

- Build `WeaponContacts::from_assembly` once per resolved configuration revision.
  `SupportGrip` and `TriggerGrip` are required. `StockContact` is optional until
  a braced pose requests it. Missing anchors return a named error.
- Contacts retain both position and quaternion. A foregrip's orientation rotates
  the support palm; it is not reduced to a positional offset.
- During manipulation, `with_support` takes the magazine/control socket after
  its mechanical transform, in weapon-root coordinates. The trigger grip and
  stock remain unchanged. The caller owns action blending and socket selection.
- Transforms with nonfinite values, nonunit quaternions or scale are rejected.
  Asset scale must not silently change anatomy.

## Reach and root adjustment

`solve_arm` preserves a 0.30 m upper arm and 0.28 m forearm, using the existing
humanoid rig's 0.10–0.575 m shoulder-to-wrist interval. This avoids folded and
straight singularities (elbow interior angles approximately 19–165°).
The pole determines the bend plane. A collinear pole uses a deterministic
perpendicular direction, rather than collapsing bone lengths. Zero/invalid poles
and unreachable wrists return errors; there is no clamped success.

`fit_weapon` keeps the requested root rotation and moves the whole assembly,
not individual contacts. Without a stock target it performs bounded alternating
projection into both arm-reach intervals. It then checks the exact resulting
contacts and a caller-supplied root-translation budget. Nonconvergence or an
excessive adjustment is a rejection, even if another pose/orientation could work.
This is a deterministic conservative solver, not a search for every possible pose.

For braced poses, the caller supplies a shoulder/clothing surface point.
The authored stock contact pins the root translation to that point. Both arms
must still reach; otherwise the caller must choose another pose/configuration.
The solver does not infer clothing thickness, search collision geometry or
allow stock drift to conceal a bad grip.

Successful solutions contain root, elbows and full wrist poses for both hands.
The same solver can operate in actor-local, camera-local or world coordinates.
Only the caller's frame/shoulders/poles change; the contact tree remains rigid.

## Integration and evidence

The existing humanoid's aiming and fallen/bracing paths reuse the analytic elbow
calculation after their existing reach clamp. Configured callers use the checked
APIs and must handle `HandlingError`.

`client::render::voxel::configured` consumes `ClientSession.configurations.cache`
by epoch/handle. Client-owned `VisualBindings` map weapon/component definitions
to baked asset IDs; catalogue/network data carries no asset paths. Node zero is
the weapon definition, and attachment nodes retain their resolved IDs, definitions
and root-local poses. Flattening those already-composed poses under one weapon
root preserves the resolved rigid tree.

Both perspectives use this assembler, the shared voxel buffers/materials and
three shared upper-arm/forearm/palm meshes. Render layers and view-model detail
are explicit instance settings. `ConfiguredHandling` supplies a preferred root,
shoulders/poles, optional stock target, bounded root adjustment, and mechanical
deltas addressed by node/part. A node/socket address can select the support hand's
current contact. The trigger grip stays authored.

Revision replacement preflights all bindings, assets and handling before spawning
the next owned hierarchy. Old revisions of the same instance are rejected.
An unavailable configuration, missing binding or infeasible pose retains the
previous hierarchy and transforms with a named `ConfigurationVisualError`.
Removing the request cleans its owned hierarchy and error, including an initial
request which failed before an instance existed.

Per-frame preparation composes authored pivots and mechanical deltas, resolves
the moving socket, solves both arms and validates every ECS target before writing
any transforms. Identity mismatches, invalid motion or missing targets reject the
whole update. This shares meshes, not entity hierarchies, across instances.

The default visual bindings now map the three baseline weapon definitions and
their magazines to six baked assets. Their original cuboids were migrated at
micrometre precision, with separate receiver/furniture/bolt/control pieces and
attachment-local magazines. Root assets expose muzzle/control sockets; magazines
expose their moving grasp socket. Catalogue anchors preserve existing support/
trigger contacts, place the magazine attachment, and locate the authored stock
surface where present. They do not force every pose to be stock-braced.

Production actor/view-model producers still need to migrate to these components.
The modular revision fixtures use the pipeline exemplar, while a separate native
fixture loads all six baseline assets in both perspectives and checks actual
contacts, muzzle position, render layers and shared buffers. This establishes the
asset/assembly path before switching production presentation.

Fixtures independently measure both bone lengths, exact contact position and
orientation, elbow limits and pole direction. They cover collinear poles,
crouch/lean/pitch, moving magazine sockets, clothing thickness, infeasible stock/
grip combinations, root-adjustment limits, missing/nonrigid anchors and equivalent
solutions across rigid reference frames. These are geometric headless checks;
the card's final local/remote in-game acceptance is still required.

Minimal Bevy apps additionally verify a real resolved rail/grip revision changes
support position/orientation without changing the trigger contact, local/world
instances share buffers, moving magazine sockets match palm position and all
three oriented axes, 60 frames keep entities and allocation counts stable, and
rejected updates preserve the entire previous pose. Affine decomposition produces
nearly-unit quaternions; angular comparisons normalize these first, alongside
direct basis-vector comparisons, without loosening the contact tolerances.
