# Pistol framing: independent static checkpoint

**Verdict: no confirmed implementation defect in this unit; not full-feature PASS.**

Independent reviewer session `ses_e23107326ffezntACCXc46L5fl` inspected `c0c6af3..58c1f3a` read-only. It performed no builds, tests, native playback, mutations or file changes and did not inspect the then-running candidate17. This is a transcription of its returned scoped review, not a final `validation.md`.

## Located outcome checks

| Outcome | Independent evidence |
| --- | --- |
| Receiver-centred cant | `client/src/render/feedback.rs:350–360` cancels rotation of the eye offset; composing the child leaves receiver placement independent of reload cant. |
| Final-camera viewport fit | `feedback.rs:589–616,649–683` transforms conservative cached mesh bounds into the final eye frame, solves viewport/near-plane inequalities and converts correction back into parent space. Actual mesh extrema are cached in `client/src/render/weapons.rs:289–310,337–367`. |
| Centred ADS | `feedback.rs:671–681` removes lateral correction at full ADS; retreat along the sight line preserves an already centred sight. Actual-vertex/ADS/lens assertions are at `:810–824,843–857`. |
| Deliberate lowering | `feedback.rs:602–614` decays the previous correction during swap/fall/recovery/death rather than pulling the weapon back into frame. |
| Connected sleeves | `feedback.rs:617–644` uses the corrected assembly inverse and final camera-relative torso anchors. Existing cuff/length/elbow assertions remain at `:1105–1124`. |
| Schedule/lifecycle/visibility | Framing follows local animation/camera feedback at `feedback.rs:13–16`; hidden geometry is excluded at `:593–595`; missing support-hand visibility remains at `:418–424`; raid changes clear the response at `:121–124`. The support child is unconditionally created by the assembly. |

The reviewer checked the retained exact-tree manifest and completed logs:112tests passed (27client/42common/43server), with source-tree identity and documentation-only differences before commit. No prior assertion was deleted or weakened.

## Remaining coverage limits

1. Composed pistol swap/fall/recovery trajectories, threshold re-entry and visibility are not fully asserted. The new fall check is one stored-offset decay step (`feedback.rs:860–877`); the older composed lowering test uses a rifle without this new framing system.
2. The disabled-framing negative control correctly detects actual portrait vertical clipping at reload fraction0, not specifically the former right-edge reload-cant defect. A separate receiver-position invariant or pivot-only mutation would discriminate that cause more precisely.
3. Containment is not contact/readability acceptance. The new test uses one fixed pitch/roll at120Hz; it does not establish every fire/turn/interruption/frame-rate case or finger-to-mechanism contact. The sleeve-occlusion guard checks two of three rail rays at one pose, not every release frame.

Normal-speed artistic UAT, matched native/performance evidence and the final independent behavior-level sensor remain required. Source line references identify the reviewed `58c1f3a` runtime, which is unchanged by remote documentation/history synchronization at `1023673`.
