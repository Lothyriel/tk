---
name: game-state
description: Keeps docs/GAME_STATE.md and its Mermaid diagram in sync with the current game. Run after every project implementation or change unless the user opts out. Also use for direct requests to document the game or prepare a playtest.
---

# Game state

Maintain [the game state guide](../../../docs/GAME_STATE.md) in English. It should tell a playtester how to launch the current build, what they can do, and what limits the session.

1. Review the changes from this task and read the current guide. On the first run, trace the normal player path from the client and server entry points. Verify commands, controls, and rules in the current code and configuration. For networked mechanics, follow the input from the client to the server and check what the client shows.
2. Create or update `docs/GAME_STATE.md` in the repository's `docs/` directory. Keep launch steps, controls, available mechanics, and playtest limitations current. Describe player actions and visible results. Mark partial features where they affect playtesting and keep the guide focused on the current build.
3. Review the embedded Mermaid flowchart on every run and update it alongside the prose. Show the paths a player can actually take. Use short English labels and group nodes by gameplay area when that makes the flow easier to scan. If the chart gets crowded, split it into small diagrams under named headings. Check that every transition agrees with the guide and the implementation.
4. Replace the final **Latest review** note with one sentence about the scope reviewed and how it was verified. If the gameplay flow still matches the chart, record that in the note and keep the diagram stable.

Honor an explicit opt-out for the current request. If the user pauses game-state updates for the session, retain that preference for the session until they ask to resume.
