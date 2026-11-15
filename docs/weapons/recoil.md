# Configured authoritative recoil

The server owns angular recoil at 128 Hz. `ClientInput.camera` is voluntary aim.
Before each fixed tick's pose/movement/fire work, the recoil spring advances in
seconds. A shot uses that effective bore and configured muzzle, then injects
exactly one angular impulse. Safe, dry, stale, duplicate semi edges and canceled
actions inject none. The usual eye-to-muzzle anatomy/world clearance still runs.

## Parameters and units

`AmmoDefinition.recoil_impulse_ns` is an effective bullet/gas momentum estimate.
The current 3.6/4.8/3.0 N·s rifle/pistol/SMG values are **gameplay approximations**,
not measured force curves. AP currently shares its platform's cartridge momentum;
its damage/penetration multipliers do not affect recoil.

For assembly mass `m` (kg), first-moment magnitude `b` (kg·m), cartridge momentum
`J`, platform angular transfer `g`, velocity multiplier `v` and explicit component
recoil multiplier `r`:

```text
lever = clamp(b / m, 0, 0.6) metres
pitch impulse = clamp(J * v * g * r / (m * (1 + 2 * lever)), 0.15, 2.4) rad/s
yaw impulse scale = 0.28 * pitch impulse
return rate = clamp(14 / (1 + lever), 9, 14) rad/s
```

The transfer coefficient represents the platform's bracing/ergonomics. Component
stock/grip/device tuning uses explicit validated modifiers rather than bonuses
inferred from names. More mass reduces angular kick; a forward balance reduces
kick and slows recovery. Barrel velocity changes momentum through `v`. Adding a
heavy suppressor does not automatically grant a compensator's gas-redirection
benefit. These are bounded controls, not a complete firearm/body dynamics model.

A seeded eight-shot lateral pattern and ±8% vertical variation depend on the
actor/raid seed and per-actor authorized-shot ordinal. Other actors' shots cannot
change that pattern. A critically damped spring uses the closed-form solution,
with yaw bounded to ±0.035 rad, pitch to 0–0.12 rad and angular velocities bounded
at impulse injection. At a positional bound, outward motion is stopped. The
baseline trajectory stays below those bounds; the 128-shots/s stress is bounded
too. The server applies no invisible random spread.

## Replication and presentation

Protocol 12 transmits eight recoil bytes per actor: signed 16-bit angles at
1/32768 rad and velocities at 1/4096 rad/s. Maximum rounding errors are
1/65536 rad and 1/8192 rad/s. The impulse ordinal stays on authority. The duplicate
reload timer was removed from the snapshot; the HUD derives it from the existing
mechanical phase/remaining time. Field packing preserves the physical stress
snapshot's original 60 KB budget.

Clients extrapolate the received spring and exponentially remove reconciliation
error (40/s), preserving the displayed angle at snapshot arrival. Duplicate/old
snapshot sequences never reapply an impulse. Camera/view-rig composition uses
this presented angle, leaving the serialized voluntary input untouched. Shot
events retain every fresh ID for cosmetic kick, bolt/flash and casings; they do
not drive angular recoil. Lost event copies cannot erase authoritative recoil.

The same seeded server fixture produces identical shot times/directions with
30/60/165 Hz input production and every third input dropped. Independent spring
oracles tolerate 1e-6 rad across those render rates. Presentation tests cover
duplicate/out-of-order state, continuous correction and convergence after delay.
The native raid/action review is the separate rendered/network acceptance.
