---
status: accepted
---

# pi owns the conversation

An agent's conversation is a pi-durable conversation, not Hamlet's own
session log (`crates/hamlet_model/src/log.rs`). pi-durable already keeps the
transcript, compacts it, forks it and counts each call's usage and cost, so
keeping Hamlet's log beside it would mean holding every conversation twice
and translating between the two. Each of today's sessions is one
conversation, and rewind becomes a fork from the entry before the message,
with the session pointed at the fork.

## Consequences

- The window's transcript, usage and rewind (`crates/hamlet_core/src/session/`)
  are rebuilt on what the server sends from pi-durable's conversations.
- Hamlet's tool records carry `policy` and `degraded`, which pi-durable's
  tool results do not. They go in the result's `details`.
