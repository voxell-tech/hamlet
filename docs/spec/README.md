# The spec

What Hamlet does, as requirements, independent of how it is built. The
words are those of [`CONTEXT.md`](../../CONTEXT.md), and the decisions
behind the design are in [`docs/adr/`](../adr/).

| File | Holds | IDs |
|---|---|---|
| [`core.md`](core.md) | One agent: its house, what it is told, its tools, what they reach, its conversations | C |
| [`hamlet.md`](hamlet.md) | The society: chiefs, mayors, roads, letters, tasks, budgets, votes, the realm, the library, what waits for a chief | H |
| [`server.md`](server.md) | Where agents run, the harnesses their conversations run in, and how the window reaches them | S |

The drawn world is not specified here; the pages in `docs/` describe it.

## IDs

Code refers to requirements by ID, and an ID never changes meaning. The
numbers are not in order and have gaps. A new requirement takes the next
number after the highest in its file.

A requirement says what is true, not how. Values in brackets are the
defaults, which the chief's limits or the settings change.
