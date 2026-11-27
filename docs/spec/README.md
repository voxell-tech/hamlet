# The spec

What Hamlet does, as requirements, independent of how it is built. The
words are those of [`CONTEXT.md`](../../CONTEXT.md), and the decisions
behind the design are in [`docs/adr/`](../adr/). What the design names and
is not built is in [`docs/proposals/`](../proposals/).

| File | Holds | IDs |
|---|---|---|
| [`core.md`](core.md) | One agent: its filesystem, what it is told, its tools, what they reach, its conversations | C |
| [`hamlet.md`](hamlet.md) | The society: chiefs, the Hand, the city of profiles and their versions, hamlets, hires and their tree, workers, roads, letters, tasks, budgets, dreams, the library, what waits for a chief | H |
| [`server.md`](server.md) | Where agents run, how the window reaches them, credentials, and the `hamlet` command | S |
| [`actor-runtime.md`](actor-runtime.md) | The actors that keep the realm: their kinds, their files on disk, who may call whom, and what survives a stop | A |

The drawn world is not specified here; the pages in `docs/` describe it.

## IDs

Code refers to requirements by ID, and an ID never changes meaning. The
numbers are not in order and have gaps. A new requirement takes the next
number after the highest in its file.

A requirement says what is true, not how. Values in brackets are the
defaults, which the chief's limits or the settings change.

### Retired

**H13** (votes) is retired with votes, ballots and the pass fraction, and
**H31** (no letters while another agent waits on a reply) with the question
that held its asker. **S17** (a conversation stays with its harness), **S24**
(what a harness must let the server decide) and **S25** (a missing part of a
realm is an error) are retired with the harness and the one database. Their
IDs are not used again. The rebuilt society changed what some requirements
govern, not what they are about: C8 and C9 name the agent's filesystem and
the agent's id where they named a house and a hamlet-and-number; C10 names
profiles; H18,
H19, H14 and H6 lost votes, requests to remove another hamlet's and moving an
agent between hamlets (`agree_move`); H5 and H20 govern hires.

### Added

In `server.md`: **S21** (the Node host runs only while its server does),
**S26** to **S28** (records in one transaction, effects as steps, versions
in semver), **S29** (images and files kept once by hash) and **S30**
(a model's image capability and limits). In `core.md`: **C33** (images
and files attached to a line) and **C34** (the read tool shows an image). In
`hamlet.md`: **H37** (the Hand), **H38** (the city),
**H39** (versions), **H40** (hires), **H41** (the tree and leads), **H42**
(workers), **H43** (dreams), **H44** (allowances) and **H45**
(newcomers at the harbor).

In `actor-runtime.md`: **A1** to **A17**, the actor runtime.
