# The society and the realm

Agents run on the server. The window receives the Hand, the profiles, the
hamlets with their agents and channels, the library and the waits through the
client protocol; it reads no records from the realm's disk. The glossary is
[`CONTEXT.md`](../CONTEXT.md), and what the society must do is in
[`spec/hamlet.md`](spec/hamlet.md).

## The Hand and the city

A realm is made with the Hand, an agent of its own that is always there
and sits above every lead. It has a worktree, `hand/`, and a profile that no
chief adds agents of; its character starts from `crates/hamlet_server/assets/hand.md`. It is the agent
`hamlet chat` talks to unless told otherwise, and a new realm has no hamlet,
so a chief asks the Hand, or types `/create-hamlet`, to make the first. It has
a tool for each of the chief's acts on the realm (H35) but sign-in,
credentials, endpoints, the network, answering waits, grants and its own
model. Its reserved acts wait for a chief (`Reserve::Hand` in
`tools/catalogue.rs`).

The city holds the profiles. A profile is a git repository
(`profiles/<id>.git`) with a character, memory and skills, and a house in the
city. A chief makes one with `/profile new <name>[: <character>]`, a parent
with `create_profile`, or with `add_agent` given a new name, which makes a blank
profile from the realm's default character (Settings); one a parent or the Hand
makes belongs to the chief whose task the turn works on. A profile is its id:
its address is `profile#<id>`. Its name is a label of one line, 1 to 40
characters with no colon, which may repeat and which `/name-profile
<profile#id> <name>` changes, a decision of the chief's like `/name-hamlet`.
`/profiles` lists them, and `/remove-profile`, `/restore-profile` and
`/erase-profile` archive one with its agents, bring it back, or erase it for
good.

The server sets each new house down itself, at the first free spot on a
spiral out from the city's middle, its door toward the middle
(`society/city.rs`); `/move-house <profile#id> <x> <z> [degrees]` sets it down
elsewhere, on a spot clear of the middle and of every house. One a chief or
the Hand makes moves in at once. One a parent makes is a newcomer, which the
`newcomers` setting lets in (H45): at once (`open`); after a chief welcomes
it from a wait (`chief`, the default); or as the Hand judges it in an
errand (`hand`, `calls/society/welcome.rs`). A newcomer waits at the
harbor, its agents working all the while. `/welcome` lets a profile at the
harbor in and `/turn-away` sends one back, undoing the Hand's verdicts.

`/make-profile <address> [name]`, Make Profile in an agent's menu, or
`make_profile` makes a profile of the city of what an agent has learned
(`clients/commands/promotion.rs`, `city::make_of_agent`). A blank agent's
own profile joins the city with the agent's branch merged into its `main`. An
agent of a city profile starts a new profile forked from its branch,
keeping its old profile. Either arrives as a new profile does: housed when
a chief or the Hand makes it, a newcomer when a parent does.

A chief reads and edits a profile's character and memory through the
Hub or `hamlet edit character <profile#id>` and `hamlet edit memory`, and
each save is a commit on `main`. `/tag` names a version, `/versions` lists
them, `/pin` holds an agent at one, `/revert` undoes a commit, `/fork` starts a
new profile from a version and `/purge` takes a path out of the whole history.
The Hand's `read_profile`, `edit_profile`, `tag`, `versions`, `pin`,
`revert`, `fork` and `purge` do the same on a chief's behalf; `edit_profile`,
`tag`, `revert` and `purge` wait for the chief. Its `welcome` and `turn_away`
answer for newcomers, and its prompt lists those at the harbor.

## Hamlets, agents and the tree

`/create-hamlet [profile#id|blank] <name>[: purpose]` creates a hamlet named
`<name>`. Its lead is an agent of an existing profile, or a blank agent named
Lead when none is named; with no purpose it is there to run the hamlet. A
hamlet's id is a UUID made when it is created, never reused, even after
erasure; commands name a hamlet by it, and `/hamlets` lists each with its id.
Its name is a label that may repeat and may hold spaces but no colon, and is
what the window shows; `/name-hamlet <hamlet> <name>` changes it.
`/add-agent <profile#id>[@<version>]|blank [in <hamlet>] [under <address>] [purpose]`
adds an agent of a profile to the hamlet you are in, up to the hamlet's tier
of 50, 100, 200 or 500 (`/tier`); the same profile can have more than one
agent. `blank` adds a blank agent instead, as an agent's `add_agent` with no
profile does: a profile of its own, made for that agent alone, which is not
of the city, so it has no house, never waits at the harbor and is not listed
with the city's profiles (`Profile.arrival` is `apart`). Reasons for a
decision may always be left out. An agent a chief adds without a purpose
takes the first line of the first message it is given, from the chief or by
another agent, as its purpose. An agent always says what it adds an agent
for.

An address names an agent by ids and never by a name: `hand`, `lead@<hamlet>`
(the lead of the hamlet with that id), `agent#<id>` and `worker#<id>`. A
profile is `profile#<id>`. Wherever an agent is shown to an agent or a chief,
a prompt, a notice, a message's heading or a tool's result, its address comes
first and its name beside it, as `agent#3 (Mira)`: its own name, else its
profile's name.
The Hand and parents see the profiles they may add agents of in their
prompts, as `profile#<id> (<name>)`, since a name finds nothing.

An agent's branch and worktree are its own. Its tools write the worktree, a turn
that changed it is committed to the branch, and the diff against `main` is
what it learned, which the Hub shows under an agent's memory. The agents of a
hamlet form a tree under the lead, as deep as its parents make it. An agent
with agents under it is a parent, with the lead's powers over its subtree: it
adds agents, creates profiles, makes profiles of the agents under it
(`make_profile`), ends and merges agents, chooses their models, turns tools off and sets
allowances (`set_model`, `set_tools`, `set_allowance`), gives them names
the world shows them by (`rename_agent`, or a chief's `/name-agent`), and opens and
closes channels among them. The lead's office can be given to another agent with `/lead`; the
old lead and its agents go under the new one.

A worker is started for one task by any agent but a worker (`start_worker`)
or by a chief (`/worker`). It is a child of its starter, has the built-in
tools its starter gave it and no society tools, works in the hamlet's folder
and a scratch folder, and ends with its answer, which goes back to its starter
as the tool's answer or as a response in its starter's conversation. A chief
watches its own worker's conversation.

## Channels and messages

Tree channels join an agent to its parent and the agents under it, and the Hand
to each lead; leads have channels to each other; a parent or a chief opens a channel
between two agents of one hamlet, which costs the hamlet and the task that
opened it. An agent proposes one to its parent with `propose_channel`. Agents of
different hamlets
have a channel once both leads call `agree_channel`, while the realm's limits allow
it.

`notify` sends a notification and `request` a request, which expects a
response. Neither waits: each is a turn of the receiver's one conversation,
and the receiver's final answer to a request goes back as a response, a
turn of the sender's (H30, H31). A request that fails comes back as an
error response, which starts no turn (`conversations/messages.rs`). An
agent at rest with a request unanswered shows "Waiting on" its receiver.
Either can name `paths` the sender may read, which land in the receiver's
`inbox/from-<sender>/`; a send over 32 MiB or 1000 files waits for a chief.
The task follows their work and charges, and a response is delivered even
past the message cap. Messages between hamlets are paid by the sender's
hamlet. See [`chat.md`](chat.md) for the queue and a stop.

An address is read with or without its name beside it, as `agent#2 (Lead)`,
so a copied ID works as a tool's argument or in a command.

## Ending, merging and restoring

`/end` ends an agent: its turns stop, its waits lapse, its grants are taken
back, its worktree is committed one last time and archived, and its agents go to
its parent. What it learned stays on its branch. `/end <address> dream`,
End with Dream, or `end_agent` with `dream` also has it dream once more to
bring that home (`hamlet_agent.ending_dream`), after a last reflection when
turns are unreflected (`reflections::before_last_dream`). The agent that
ended it is told how the dream went (`Core::dreamt`). The lead cannot be
ended; give the office away first. A parent,
the Hand included, may not end or merge an agent on a chief's task (H7): while
an input of that task is still to end in its conversations, or until the
hamlet's `protectedMinutes` have passed since the chief's instruction was
answered. A chief may.
`/merge <a> into <b>` makes one agent of two of a hamlet: the kept agent
takes the absorbed one's opened channels, agents and inbox, and takes in what it
learned. Agents of one profile merge their branches; agents of different
profiles bring what the absorbed agent learned into the kept branch, a
clashing file kept as `<name>.from-<absorbed>` and a clashing skill as
`<skill>-from-<absorbed>`, with the absorbed agent as `agent-<id>`. The kept
agent's conversation is given the absorbed one's, from its latest summary
and at most 24000 characters, without a turn.
`/restore` brings an ended agent back under the same id, or splits a
merged one out again by undoing the merge's commit. `/erase` deletes an ended agent's worktree and branch for good. A
message to an agent that has left is rejected with how it left.

`/remove-hamlet` ends every agent of a hamlet and keeps its tree and worktrees;
`/restore-hamlet` brings them back while their profiles live, and
`/erase-hamlet` deletes the worktrees and branches. Removed hamlets appear
apart from the living ones in the overview.

Once something the chief erased in the window is gone, a toast says so and
the Hub turns to its parent's thread, else its hamlet's page, else the
realm's (`screens/hub/erasure.rs`).

## Memory, reflection and dreams

An agent reflects every few answered turns of a chief's lines in its
conversation, or when a chief asks (`/reflect`), and keeps what it learned in
its worktree's `memory/` and `skills/`. That stays on its branch. When dreams are
turned on in the settings, an agent that is idle and has learned something its
profile lacks dreams, within the chief's hours: it is given its worktree and a
checkout of `main`, and decides what goes to the profile and what stays. A
profile with living agents dreams once a day to tidy `main`. One dream runs at
a time. A dream asked for (`/dream`, the Hand's `dream`, the window's menus)
runs whatever the settings say; of a profile, its agents that learned
something dream first. Asked dreams wait in `asked_dream`, ahead of an
agent's last dream, ahead of scheduled ones (`conversations/dreams.rs`,
`next`). A dream is a task with a cost cap, and its commits are named after
it.

## The shared library and chief oversight

The library is a repository, checked out read-only for every agent, which
starts with the shipped skills. An agent offers a skill using `submit_skill`;
the librarian judges it in one model call with no tools, and `ACCEPT` as the
first word shelves it. Other answers refuse it. `request_librarian` sends the
librarian a request about the shelf, and `/unshelve` takes a skill out. The shelf and each decision are kept
by the server.

A chief's instruction is recorded before it starts. In its own conversation
an agent may object once and wait for go or withdraw. When a chief instructs
an agent under a parent directly, its parent's review, as the limits set,
runs beside the work and may raise its own objection. An agent can put
questions to a chief (`request_chief`) from any turn of its conversation,
and a worker from the turn of a chief's own line, and wait for reply or
dismiss. These waits appear in the Hub and survive a
server restart. Tool calls wait too (H11): the network, manual-mode edits and
commands, the acts a chief's limits reserve, and the Hand's acts on profiles.
Answering allows once, for the agent's conversation, or always; `/grants`
lists and `/revoke` takes back what was allowed for good.

## Several hamlets

Another hamlet's agent is reached by its `agent#<id>` like any other,
`lead@<hamlet>` addresses the lead of the hamlet with that id, and
`/hamlet <hamlet>` switches the terminal to it. `/agent <address>` finds any agent of the realm.
A lead sees the other hamlets in its prompt.

Removed hamlets retain their worktrees and records and appear under CLOSED DOWN
in the overview. RESTORE brings one back. DELETE FOR GOOD asks first and
permanently erases its worktrees; that question is discarded on restore or
leaving the page.

The server sends whole replacement states when the library shelf or the
removed-hamlet list changes, or a hamlet leaves the live list. The client
reconciles surviving entities and removes those no longer sent. The terminal
also replaces its addresses from those states.
