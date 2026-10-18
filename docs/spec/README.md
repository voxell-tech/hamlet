# The spec

What Hamlet does, as requirements, independent of how it is built. It is the
target of the rebuild in `docs/proposals/agent-server.md` and of the Rust
server in `docs/proposals/rust-server.md`: the concepts and rules stay; the
server, the storage and the connection to the window are new. The words are those of [`CONTEXT.md`](../../CONTEXT.md).

| File | Holds | IDs |
|---|---|---|
| [`core.md`](core.md) | One agent: its house, what it is told, its tools, what they reach, its conversations | C |
| [`hamlet.md`](hamlet.md) | The society: chiefs, mayors, roads, letters, tasks, budgets, votes, the realm, the library, what waits for a chief | H |
| [`server.md`](server.md) | Where agents run and how the window reaches them | S |

The drawn world is not specified here; the pages in `docs/` describe it.

## IDs

Code refers to requirements by ID. This spec was written again from the code,
after the earlier one was lost. The IDs the code cites (C5, C7, H3, H5 to H8,
H10 to H14, H16 to H20, H22) keep the meaning they have there. The earlier
spec's other IDs are unknown, so they are never reused, and new requirements
are numbered after the highest known one: C8 on, H23 on. An ID never changes
meaning; a requirement that is dropped keeps its ID, marked as dropped.

A requirement says what is true, not how. Values in brackets are the
defaults, which the chief's limits or the settings change.
