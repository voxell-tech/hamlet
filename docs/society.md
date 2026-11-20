# The society and the realm

Agents run on the server. The window receives the Hand, the profiles, the
hamlets with their agents and channels, the library and the waits through the
client protocol; it reads no records from the realm's disk. The glossary is
[`CONTEXT.md`](../CONTEXT.md), and what the society must do is in
[`spec/hamlet.md`](spec/hamlet.md). Commands are in `clients/commands/`;
`/help` lists them.

## The Hand and the city

The Hand is the realm's own agent, above every lead, with a worktree
`hand/` and a profile no chief adds agents of. It has a tool for each of the
chief's acts on the realm (H35) except sign-in, credentials, endpoints, the
network, answering waits, grants and its own model. Its reserved acts wait
for a chief (`Reserve::Hand` in `tools/catalogue.rs`).

The city holds the profiles. A profile is a git repository
(`profiles/<id>.git`) with a character, memory and skills. One a parent or
the Hand makes belongs to the chief whose task the turn works on.

A profile is its id: its address is `profile#<id>`. Its name is a label of
one line, 1 to 40 characters with no colon, which may repeat; changing it is
a decision of the chief's like `/name-hamlet`. Houses are set down on a
spiral out from the city's middle (`society/city.rs`).

A profile a chief or the Hand makes moves in at once. One a parent makes is a
newcomer, which the `newcomers` setting lets in (H45): `open`, `chief` (the
default) or `hand` (`calls/society/welcome.rs`). A newcomer waits at the
harbor, its agents working all the while.

`/make-profile` makes a profile of the city of what an agent has learned
(`city::make_of_agent`). Each save of a profile's character or memory is a
commit on `main`; the Hand's `edit_profile`, `tag`, `revert` and `purge` wait
for the chief.

## Hamlets, agents and the tree

A hamlet's id is a UUID made when it is created and never reused, even after
erasure; commands name a hamlet by it. Its name is a label that may repeat
and holds no colon.

A hamlet holds up to its tier of agents (`/tier`). `blank` adds an agent
whose profile is not of the city: it has no house and never waits at the
harbor (`Profile.arrival` is `apart`).

An address names an agent by ids, never by a name: `hand`, `lead@<hamlet>`,
`agent#<id>` and `worker#<id>`. Wherever an agent is shown to an agent or a
chief, its address comes first and its name beside it, as `agent#3 (Mira)`,
and an address works with or without the name. Prompts show profiles the
same way.

An agent's branch and worktree are its own: a turn that changed the worktree
is committed to the branch, and the diff against `main` is what it learned.

The agents of a hamlet form a tree under the lead. A parent has the lead's
powers over its subtree (`set_model`, `set_tools`, `set_allowance`, adding,
ending and merging agents, opening channels among them, and moving them
within the subtree with `reparent_agent` or a chief's `/reparent`).

A worker is started for one task by any agent but a worker (`start_worker`)
or by a chief (`/worker`). It has the built-in tools its starter gave it and
no society tools, and ends with its answer, which goes back to its starter.

## Channels and messages

Tree channels join an agent to its parent and the agents under it, and the
Hand to each lead. A parent or a chief opens a channel between two agents of
one hamlet, which costs the hamlet and the task that opened it. Agents of
different hamlets have a channel once both leads call `agree_channel`, while
the realm's limits allow it.

`notify` sends a notification and `request` a request, which expects a
response. Neither waits: each is a turn of the receiver's one conversation,
and the receiver's final answer to a request goes back as a response, a turn
of the sender's (H30, H31). A request that fails comes back as an error
response, which starts no turn (`conversations/messages.rs`).

Either can name `paths` the sender may read, which land in the receiver's
`inbox/from-<sender>/`; a send over 32 MiB or 1000 files waits for a chief. A
response is delivered even past the message cap. Messages between hamlets
are paid by the sender's hamlet. See [`chat.md`](chat.md) for the queue and
a stop.

## Ending, merging and restoring

`/end` stops an agent's turns, lapses its waits, takes back its grants,
commits and archives its worktree, and gives its agents to its parent. What
it learned stays on its branch; `dream` also has it dream once more to bring
that home (`hamlet_agent.ending_dream`). The lead cannot be ended; give the
office away first.

A parent, the Hand included, may not end or merge an agent on a chief's task
(H7): while an input of that task is still to end in its conversations, or
until the hamlet's `protectedMinutes` have passed since the chief's
instruction was answered. A chief may.

`/merge <a> into <b>` makes one agent of two: the kept agent takes the
absorbed one's channels, agents and inbox, and takes in what it learned.
`/restore` undoes the merge's commit; `/erase` deletes an ended agent's
worktree and branch for good. A message to an agent that has left is rejected
with how it left. `/remove-hamlet` ends every agent of a hamlet and keeps
its worktrees until `/erase-hamlet`.

## Memory, reflection and dreams

An agent keeps what it learned in its worktree's `memory/` and `skills/`, on
its branch. Dreams move it to its profile's `main`; one dream runs at a time.
A dream asked for (`/dream`, the Hand's `dream`, the window's menus) runs
whatever the settings say. Asked dreams wait in `asked_dream`, ahead of an
agent's last dream, ahead of scheduled ones (`conversations/dreams.rs`,
`next`).

## The shared library and chief oversight

The library is a repository, checked out read-only for every agent. An agent
offers a skill with `submit_skill`; the librarian judges it in one model call
with no tools, and `ACCEPT` as the first word shelves it.

A chief's instruction is recorded before it starts. An agent may object
once and wait for go or withdraw; a parent's review of a direct instruction
runs beside the work and may raise its own objection.

An agent can put questions to a chief (`request_chief`), and ask for an
MCP server (`request_mcp_server`); these waits survive a server restart.
Tool calls wait too (H11): the network, manual-mode edits and commands,
the acts a chief's limits reserve, the Hand's acts on profiles, and MCP
tools that may change something outside the realm. Answering allows once, for the
agent's conversation, or always; `/grants` lists and `/revoke` takes back
what was allowed for good.

## Several hamlets

The server sends whole replacement states when the library shelf or the
removed-hamlet list changes, or a hamlet leaves the live list. The client
reconciles surviving entities and removes those no longer sent. The terminal
also replaces its addresses from those states.
