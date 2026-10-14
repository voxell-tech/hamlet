---
status: accepted
---

# The window is a client of the server's state

The window and the server are separate processes, even on one machine, and
the window never starts the server: `hamlet dev` starts both for local work.
The server sends the window ready-made state (a snapshot of hamlets, agents,
roads, budgets and what waits, then its changes, plus the live events of
turns and transcripts on request) over one WebSocket that needs a token even
on localhost. The window fills its existing components from it and holds no
rules of its own.

The messages are defined once, in TypeBox, and exported as JSON Schema, from
which the Rust types are generated.

## Considered options

- Sending the records line by line, for the window to rebuild state with the
  read side of `hamlet_rules`, as it does from the files today. Rejected:
  the rules would live in two languages, TypeScript writing the records and
  Rust reading them, and would have to agree forever.
- The window starting the server itself. Left for later as a convenience:
  a window that only connects behaves the same against a local server and a
  deployed one.

## Consequences

- `hamlet_rules`, `hamlet_model`, `hamlet_home` and `hamlet_sandbox` leave
  the window. Its world and panels keep reading the same components.
