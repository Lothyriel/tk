# Handling clocks and priorities

The server owns manipulation state and action IDs. Presentation consumes the
action fraction; it must not infer completion from a local animation timer.
`catalog::HandlingTiming` defines authored mechanical seconds:

| Action | Seconds | Commit |
| --- | ---: | --- |
| Unequip | 0.20 | Mark outgoing weapon unequipped and change active slot |
| Equip | 0.35 | Mark incoming weapon equipped; only then may it fire |
| Inspect | 2.80 | End inspection without changing magazines or chamber |

Injury scales the shared mechanical clock. The slot handoff occurs after
unequip, not halfway through an independent client swap animation. Interrupted
reloads retain their previously committed magazine/chamber locations.

## Inspect priority

Inspect is an external look at the actual equipped assembly. It does not
unload the chamber, remove a magazine, change selector or resolve a new kit.

| Input/state | Effect on inspect |
| --- | --- |
| Fresh inspect command while idle/equipped | Start once |
| ADS or sprint intent | Cancel current inspect; reject simultaneous inspect |
| Fire held or a fresh trigger press | Cancel/reject inspect even on safe, empty or during shot cooldown |
| Fresh reload command | Cancel/reject inspect even if no reload is needed |
| Slot switch | Cancel/reject inspect; perform authoritative unequip/equip |
| Existing reload, equip or unequip | Reject inspect |
| Fall, death, trigger-arm loss, stale input or inactive raid | Existing server gates cancel actions and synchronize command sequences |

Switch remains ahead of reload; a started reload blocks firing until its
mechanical completion. ADS/sprint cancellation here changes inspect only;
it does not silently cancel an authoritative reload.

Every inspect command is consumed when observed, including a rejected one.
Releasing another input does not replay the blocked inspection. Starting again
requires a fresh sequence number. Duplicate/reordered commands likewise cannot
restart the action or grant ammunition.

## Presentation composition

`weapon_animation::manipulation::action` evaluates the mechanical snapshot for
the authoritative anatomical rig and both render perspectives. Inspect presents
one receiver side, dwells, crosses to the other side, then returns. It keeps both
grips and all installed parts; magazine/control contact weights remain zero.
Equip and unequip use distinct fractions of the shared mechanical clock. The
displayed instance changes at the server slot marker, starting the incoming
weapon in its lowered equip pose.

Local composition starts with the configured hip/sight placement, then applies
the action about the receiver rather than the eye, followed by aim, recoil and
the existing locomotion/turn/acceleration response. Action translation is bounded
to 1.2 m/s and rotation to 5 rad/s through interruptions. ADS closes the stock
only as the canted action returns; independent interpolation of both full layers
would jerk the hands across the camera. ADS blend changes at most two units per
second. Acceleration-derived local inertia is filtered before translation so a
new movement snapshot does not instantly displace the weapon.

The whole rigid local rig is framed from cached baked part bounds and the actual
palms in hip/inspect poses. Keeping bounds active at rest avoids a framing jump
when inspect starts. ADS preserves the sight line. Reload retention and deliberate
lowering can leave the viewport; the prior correction decays through these
transitions. Framing never changes FOV, bone lengths or attachment scale.
