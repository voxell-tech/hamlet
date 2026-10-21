---
status: accepted
---

# The window is a client of the server's state

The window and the server are separate processes, even on one machine, and
the window never starts the server: `hamlet dev` starts both for local work.
The server sends the window ready-made state (a snapshot of hamlets, agents,
roads, budgets and what waits, then its changes, plus the live events of
turns and transcripts on request) over one WebSocket that needs a token even
on localhost. The window fills its components from it and holds no rules of
its own.

The messages are defined once, as the Rust types of `hamlet_protocol`;
the server derives from them the JSON Schema it checks each client
message against.

## Considered options

- Sending the records line by line, for the window to rebuild state with
  rules of its own. Rejected: the rules would live twice, one copy writing
  the records and one reading them, and the two would have to agree
  forever.
- The window starting the server itself. Left for later as a convenience:
  a window that only connects behaves the same against a local server and a
  deployed one.

## Consequences

- No window crate depends on a crate that holds the rules;
  `tests/window_is_a_client.rs` guards that.
