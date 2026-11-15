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

The production remote actor producer now uses these components. It creates one
configured weapon per actor, takes the epoch/handle from the accepted snapshot,
and preserves the previous displayed configuration during the holster phase.
The first-person producer uses the same bindings, cache, action adapter and
solver, retaining one presentation container per equipped class and replacing
its configured contents by revision.
The modular revision fixtures use the pipeline exemplar, while a separate native
fixture loads all six baseline assets in both perspectives and checks actual
contacts, muzzle position, render layers and shared buffers.

`WeaponAction` carries magazine/control contact weights alongside its existing
mechanism poses. Feedback filters those weights with the action; the native
adapter blends the authored support grip toward the **current** moving magazine
socket, then the control socket. The magazine delta is attachment-local and uses
the resolved node ID. Visibility is addressed by node/part, including a removed
magazine after reload interruption.

The assembler publishes `ConfiguredPose` only after a successful atomic update.
It includes the accepted shoulders, so a rejected update cannot recompute an
upper arm from a new shoulder and an old elbow. `ArmSolution::segments` produces
the existing anatomical upper-arm/forearm/hand centres and rotations. Remote
presentation applies them to the existing skin/wound/core entities and records
them in the last-live handoff cache; its additional configured arm meshes are
hidden. Muzzle flash follows the accepted root and is hidden without a visible
weapon. Missing limbs retain their anatomical visibility; falling, physical,
dead or trigger-arm-disabled actors hide their weapon.

The first-person rig explicitly uses presentation-space shoulders/poles, whereas
the remote rig derives them from the anatomical torso. Both keep 30/28 cm bones
and the same socket conventions. Pistol framing previews the cached part bounds
and solved palms, then translates the complete view rig. It retains ADS centring
and the existing lens; it does not rescale forearms or independently move hands.
Magazine removal and a locked-open pistol slide use the shared native adapter.
Production no longer constructs procedural weapon meshes or uses the old
view-arm placement. The previous procedural assembly/placement remains compiled
only in tests as a migration oracle.

This aligns remote visual tissue with the accepted contacts and preserves all 17
segment IDs. Native stock/clothing validation and rendered acceptance remain
before the card can complete.

## Authoritative anatomy and handoff

The logical catalog supplies `MagazineGrip` and `ControlGrip` as well as the
static grips. `handling::mechanisms` shares bolt/control and attachment-local
magazine deltas with the client. `configured_contacts` composes those deltas
with resolved anchors, including rotated attachments, and blends the moving
support contact. Native client fixtures compare this result against baked
sockets; authority uses zero visual recoil.

`humanoid::configured::evaluate` replaces only the six arm/hand poses, using
the same preferred root as remote presentation. Physical poses retain ownership
of all 17 segments. The server captures the configured result from the current
Arsenal into `ragdoll::Motion`; projectile hit testing and injury resolution
consume that accepted pose. An unreachable update records a handling error and
retains the accepted local anatomy while following the actor root. Fall/death
handoff therefore receives the same tissue used for damage. Recovery evaluates
a configured standing target and reports rejection before applying it.

Real server fixtures hit a revised arm with ordinary weapon fire, reject an
unreachable revision while the actor moves, and transfer all 17 segments into
Rapier. The rest/run × floor/wall/corner/step regression also checks every frame
from 8 to 10 seconds for residual velocity and drift. Passive joint motors use
force-based torque damping (2.5 N m s/rad), avoiding inertia-scaled weak damping
of distal segments against loaded angular limits.

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

Native producer fixtures run the actual actor, feedback and voxel systems plus
Bevy transform/visibility propagation. All three baseline weapons traverse
tactical and empty reloads with recoil, crouch, lean and pitch, measuring body-arm
endpoints and contacts against the rendered socket hierarchy. Separate cases
cover magazine-node revision replacement, missing limbs, fall/recovery/death,
entity/cache stability and absence of duplicate arm rendering.

First-person native fixtures instantiate the production view-model container and
baked hierarchy. They measure bone endpoints and moving contacts for all three
weapons, tactical/empty reloads, recoil, ADS and three portrait-to-ultrawide
aspects. The pistol check projects actual visible weapon/palm vertices, enforces
the 94% framing envelope and 35 mm/frame continuity bound, and checks the actual
ADS sight point. Additional cases cover node replacement, missing limbs/fall and
reload interruption with a removed magazine and locked-open slide. These remain
headless geometric checks, not visual acceptance.
