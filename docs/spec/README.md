# The spec

What Hamlet does, as requirements, independent of how it is built. The
words are those of [`CONTEXT.md`](../../CONTEXT.md), and the decisions
behind the design are in [`docs/adr/`](../adr/).

| File | Holds | IDs |
|---|---|---|
| [`core.md`](core.md) | One agent: its lodge, what it is told, its tools, what they reach, its conversations | C |
| [`hamlet.md`](hamlet.md) | The society: chiefs, the Hand, the city of scholars and their versions, hamlets, hires and their tree, workers, roads, letters, tasks, budgets, dreams, the library, what waits for a chief | H |
| [`server.md`](server.md) | Where agents run, the harnesses their conversations run in, how the realm's records and effects are kept, and how the window reaches them | S |

The drawn world is not specified here; the pages in `docs/` describe it.

## IDs

Code refers to requirements by ID, and an ID never changes meaning. The
numbers are not in order and have gaps. A new requirement takes the next
number after the highest in its file.

A requirement says what is true, not how. Values in brackets are the
defaults, which the chief's limits or the settings change.

### Retired

**H13** (votes) is retired with votes, ballots and the pass fraction; its ID
is not used again. The rebuilt society changed what some requirements
govern, not what they are about: C8 and C9 name the lodge and the agent's id
where they named a house and a hamlet-and-number; C10 names scholars; H18,
H19, H14 and H6 lost votes, requests to remove another hamlet's and moving an
agent between hamlets (`agree_move`); H5 and H20 govern hires.

### Added

In `server.md`: **S21** (a harness runs only while its server does, which code
already cited), **S26** to **S28** (records in one transaction, effects as
jobs, versions in semver). In `hamlet.md`: **H37** (the Hand), **H38** (the city),
**H39** (versions), **H40** (hires), **H41** (the tree and leads), **H42**
(workers), **H43** (dreams) and **H44** (allowances).
