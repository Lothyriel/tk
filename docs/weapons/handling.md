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
