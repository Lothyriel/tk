# Animation and ragdoll technical reference

## Ownership and decisions

- `common/src/weapon_animation.rs` owns original procedural weapon rigs, grip/muzzle anchors and empty/tactical action curves. Presentation never grants ammunition or changes authoritative timing.
- `client/src/render/{weapons,feedback}.rs` assembles models and applies recoil, reload/swap, ADS and framing. Confirmed shot IDs prevent duplicate feedback. Interruption retains filtered action state rather than snapping mechanisms to idle.
- Held RMB requests server-authoritative ADS. Reload, sprint, fall, death and inactive input suppress it. Default world/weapon FOV is 70°/65°; ADS zoom is 52° for pistol and 44° for primaries. Sights must remain ahead of the 0.1 m near plane, not an arbitrary eye distance.
- Pistol reload cant pivots around the receiver rather than the eye offset. Framing uses actual displayed mesh bounds with a 6% edge margin, keeping the weapon lens fixed and ADS sights centred. Sleeves join glove cuffs to body-side anchors; they may leave the viewport at the body.
- `server/src/tick/{locomotion,ragdoll,live_body}.rs` owns planted contacts, directional momentum/local impulse transfer, surviving dynamic falls and same-ID recovery. Support, rest and headroom gate recovery; death reuses existing bodies. Debris is excluded from character controller support/autostep queries.
- Stationary large turns release and replant feet to prevent crossed-shin occlusion. Anatomical knee frames constrain the actual bend rather than adding the captured bend to a new joint range.
- Server-only eye-to-muzzle clearance resolves posed anatomy and map surfaces before continuing the muzzle ray. This avoids close-range eye-ray parallax misses without bypassing intervening cover or charging wood attenuation twice. `TK_SHOT_DIAGNOSTICS=1` enables diagnostic contacts; compare performance with equivalent flags.

## Verification anchors

Use `python scripts/verify.py` and the focused package/filter options described in [DEVELOPMENT.md](DEVELOPMENT.md).

- Shared animation tests cover anchor ordering/contact, empty versus tactical actions, continuity and interruption.
- Composed client tests exercise real weapon/glove/magazine geometry, actual projection, sleeve/slide visibility and framing across aspect ratios. Helper-only assertions do not establish assembled visual contact.
- Real Rapier tests cover rest/run floor/wall/corner/step contacts, momentum, local impulse conservation, stable IDs, separation, support/headroom, recovery and death during transitions.
- Stress serialization has a 60,000-byte bound. A historical protocol-5 stress sample used 59,784 bytes; remeasure before adding replicated fields.
- Headless assertions do not establish natural motion, all intermediate trajectories, displayed interpolation under UDP loss or artistic acceptance.

## Reproduction tools

`scripts/playtest.py` runs a rendered normal raid; `scripts/playtest_physical.py` runs the focused healthy death/rest network matrix. Both use real transport. Use them only for relevant acceptance, not inner-loop physics tuning.

`scripts/record_animation_baseline.py --prepare-only --match-current-camera` exports the historical presentation baseline with source fingerprints and a disclosed projection overlay. `scripts/playtest_animation_baseline.py` verifies baseline provenance before running its historical route. Consult `--help` and use fresh output paths.

Compare performance without capture overhead, using matched hardware, route, resolution, projection and diagnostic flags. Do not run competing clients/benchmarks on the same GPU/port. Video and frame-review helpers require explicit capture/review scope.

## Acceptance and provenance

Operational follow-up is in [TAR-10: visual/network acceptance](https://linear.app/tarkov-killer/issue/TAR-10) and [TAR-9: matched performance](https://linear.app/tarkov-killer/issue/TAR-9). Neither is closed by this reference.

Historical candidate17 (`58c1f3a`) passed its diagnostic dual UDP/loss raid and sampled pistol framing review; that is not normal-speed artistic acceptance. Candidate15's old 16.8 s recovery checkpoint failed and stays failed. The later accepted observation bounds actual Standing completion at 18.2 s while preserving the 1 s get-up, physical gates, IDs and fatal-shot deadline.

The complete historical evidence ledger and failed attempts remain in [Git at 6a4a99b](https://github.com/Lothyriel/tk/blob/6a4a99b/docs/ANIMATION_RAGDOLL.md). Its local `target/playtest/` paths may not exist on another machine; no old process is presumed active. Public reference clips informed contact/weight analysis only; their art, code and curves were not imported.
