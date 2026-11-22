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

## Armed stride reference (TAR-102)

The carry layer samples the confirmed gait phase, planar speed, acceleration
and yaw rate; it does not start a sprint oscillator. POV samples the same
snapshot history as the local root/camera; the observer samples the presented
body state. The camera keeps its existing bob, not this carry. The upper body
leans into acceleration/turns; the weapon moves about the trigger-grip pivot
and both palms remain on the configured sockets. The carry is presentation
only: foot targets, authoritative hit volumes, and the physics handoff are unchanged.
Carry blends out toward walk, ADS and the presented manipulation pose.

| Existing game kit | Lowering | Base pitch | Base cant |
| --- | ---: | ---: | ---: |
| `compact` (handguard hold) | 40 mm | 0.16 rad | 0.09 rad |
| `suppressed` (lower RVG hold) | 27 mm | 0.11 rad | 0.055 rad |

The lower socket reduces these three base values; the phase, stride harmonics,
turn and acceleration gains are the same. The configured socket also defines
the different wrist position and orientation; it is not moved by the curve.

### Acceptance procedure

After coordinating an exclusive rendered/UDP slot, build the same revision of
both binaries (`cargo build --locked --workspace`). Run this bench once per kit
and cadence (`compact`/`suppressed`, `30`/`165`), with fresh output directories:

```sh
python scripts/playtest.py --no-build --server-path "$CARGO_TARGET_DIR/debug/server" \
  --client-path "$CARGO_TARGET_DIR/debug/client" --pov stop-lean --clients 2 \
  --viewport 900x700 --weapon-kit "$kit" --pov-hz "$hz" --pov-drop-every 3 \
  --output "../tar102-tmp/udp-$kit-$hz"
```

The existing bench repeats rest → walk → sprint → brake before the low wall
→ crouch/lean/return. It drops every third snapshot on both clients as
configured above. Check `run-settings.json`, client logs, `pov-trace.csv` and
`peer/gait-trace.csv` for completion, logged loss, actual cadence and the
states sampled. Compare leader walk/run/stop captures and the peer's running
silhouette; check both grips, full-body weight transfer and camera bob.
Screenshots/traces do not establish normal-speed continuity.

For live temporal acceptance, use a two-client private test session with the
same kit and camera settings. Launch both ordinary clients with
`TK_DROP_SNAPSHOT_EVERY=3` for the loss pass, unset it for the control, and
confirm `SNAPSHOT LOSS` in both client logs. Record achieved FPS (the automated
bench above gives explicit 30/165 Hz update pacing). On a clear lane toward a fixed marker,
walk for one second, hold Run for one second, turn about 25° while still moving,
then release Run and movement during the next step with ADS held. Resume a walk
while still aiming, sprint again while holding ADS, then release Run and confirm
ADS returns. Repeat on both kits, placing the observer clear of the path with a
three-quarter view of the torso and hands. Review live at normal speed.
Record separately whether each transition
is continuous in POV and on the peer, whether either grip breaks contact, and
whether ADS resumes. The straight bench does not script the turn or ADS during
braking, so it is not sufficient on its own. No video is required.
