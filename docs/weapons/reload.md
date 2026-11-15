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
