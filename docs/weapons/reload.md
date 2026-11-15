# Reload identity and clocks

The server owns chamber, magazine locations, rounds and manipulation commits.
Rendering never changes ammunition. Action time advances by the same handling
rate that injuries apply to the authoritative simulation.

## Physical identity on the wire

Protocol 14 publishes an ordered roster of up to four magazine IDs/definitions
with each reliable weapon configuration. The order is part of the versioned
contract: it is the physical pool order, not a sort by capacity or definition.
Reloading changes the inserted definition/revision but preserves that roster.
Selecting a new free lobby kit allocates a new pool and publishes a new revision.

Two 16-bit snapshot references carry the current/outgoing magazine and reload
target. Low three bits encode pool index plus one; zero means absent. The upper
bits encode rounds, bounded by the catalog's maximum capacity of 1000. Identity
and geometry are recovered from the matching configuration, never from a guessed
class or capacity. The current/outgoing reference retains the source throughout
the action so a late observer can distinguish it from an already inserted target.

During Remove, the source is installed. During Insert, neither is installed.
During Feed/Finish, the target is installed. Outside reload, the first reference
is simply the installed magazine and the target is absent. Cancelled removal
therefore leaves no installed magazine; cancelled insertion preserves the
committed target. A feed-only resume has a target but no outgoing magazine.

The reliable roster contains no changing ammunition counts or second inventory.
The two compact references preserve the existing 60,000-byte worst-case snapshot
budget; copying full physical IDs/definitions per frame would exceed it.

## AR presentation tracks

`ReloadTiming` owns the normalized removal (0.25), insertion (0.70) and
feed (0.90) commits. The M4A1 track clears the feed lips axially, moves toward
retention, and swaps the outgoing/incoming visual identity inside the hidden
0.38–0.44 retention interval. Retrieval aligns at 0.60 and seats at 0.66,
leaving a dwell before insertion. Tactical reload never presses the bolt catch;
empty reload holds the carrier open until the feed commit and returns from the
catch to the installed support grip.

The client resolves each distinct owned magazine geometry once when accepting
the reliable configuration. Local and remote assemblies choose that variant
before insertion, with the same physical ID and authored grasp socket. There is
one displayed magazine, never separate installed/carried copies. Authoritative
anatomy uses the corresponding pre-resolved presentation variant; gameplay
properties continue to use the installed configuration.

The displayed root/hand response remains filtered through interruption. Once
the server commits insertion, the magazine pose is seated without filter lag;
bolt state likewise follows the authoritative commit. Cancellation clears the
incoming binding and displays only the committed installed location.
During slot switch, the response retains the outgoing weapon's last known
mechanical state and collapses its reload to the committed installed magazine.
That state follows the lowering weapon until the displayed slot changes; the
new pistol/rifle snapshot cannot make its predecessor's magazine reappear.

Composed engine fixtures cover mixed 20/30-round pools, local/remote contacts,
stock/grip extremes, 30/60/165 Hz updates, slowed mechanical time, lost updates,
and cancellation in each phase. Projection of the actual magazine mesh checks
that retention leaves the local viewport before hiding. These measurements do
not certify normal-speed weight/naturalness or final finger clearance.
