---
status: accepted
---

# pi owns the conversation

In the pi harness, an agent's conversation is a pi-durable conversation.
pi-durable already keeps the transcript, compacts it, forks it and counts
each call's usage and cost, so Hamlet keeps no conversation log of its own
beside it: that would hold every conversation twice and translate between
the two. A rewind is a fork from the entry before the message, with the
chat pointed at the fork.

## Consequences

- The window's transcript, usage and rewinds are built from what the
  server sends from the harness's conversations.
- Hamlet's tool records carry `policy` and `degraded`, which pi-durable's
  tool results do not. They go in the result's `details`.
- The server may cache a transcript to serve clients, but never keeps a
  second one of its own.
