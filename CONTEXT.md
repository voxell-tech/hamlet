# Hamlet

AI agents that live as hamlets, overseen by the humans they belong to and
shown as a world of hamlets.

## Language

### People

**Chief**:
A human a realm answers to. Instructs any of its agents, answers what
waits for a chief, and can overrule anything. Never an agent.
_Avoid_: user, owner, admin

### Agents

**Agent**:
An AI the server runs. Most are a profile working in a hamlet, on a branch
of its own and in a worktree, for a purpose a chief may leave out; a hamlet's
agents form a tree under its lead. The Hand and workers are agents too.
_Avoid_: hire, citizen, resident, bot, member, employee

**Hand**:
The chiefs' own agent. It is always there, sits above every lead, and
founds hamlets and looks after profiles when a chief asks. Its reserved
acts wait for a chief.
_Avoid_: assistant, root agent, admin agent

**Profile**:
Who an agent is when it is not at work: a git repository holding a
character, memory and skills. Its `main` is what it knows. Many agents can
be of one profile. A profile is its id, `profile#<id>`; its name is a label
that a chief changes and that may repeat.
_Avoid_: scholar, persona, template, house

**Blank agent**:
An agent of a hamlet, the lead included, that is not of a profile of the
city: its profile is made for it alone, has no house, never waits at the
harbor and is not listed with the city's profiles.
_Avoid_: blank profile, temp agent

**Address**:
How a chief or an agent names an agent or a profile, by ids and never by a
name: `hand`, `lead@<hamlet>` (by the hamlet's id), `agent#<id>`,
`worker#<id>`, `profile#<id>`. Wherever an agent is shown, its address comes
first and its name beside it.
_Avoid_: handle, username

**Name**:
The words a profile, a hamlet, an agent or a worker is shown by: a profile's
name, a hamlet's name, an agent's or a worker's nickname. Metadata a chief
changes and that may repeat; it never finds anything. Everyone but the Hand
has one a chief can change. A profile and a hamlet always have one; a
hamlet's id is a UUID no one reads.

**Nickname**:
What an agent or a worker is called in the world, given by a chief, or for
an agent of a hamlet by the Hand or a parent above it. Its address,
`agent#<id>` or
`worker#<id>`, stays the same.
_Avoid_: alias

**Lead**:
The agent at the top of a hamlet's tree: the one the chief talks to first.
Given to another agent with `/lead`.
_Avoid_: mayor, orchestrator, main agent

**Parent**:
An agent with agents under it, or the Hand. It has the lead's powers over
its subtree: adding agents, ending, merging, choosing models and tools, setting
allowances and laying roads.
_Avoid_: lead (the top of a hamlet), manager, supervisor

**Worker**:
An agent started for one task by another agent or a chief. A child of its
starter, with a scratch folder and no society tools, which ends with its
answer.
_Avoid_: subagent, helper, runtime

**Errand**:
A one-off conversation that is not an agent's: the librarian's answer or a
parent's review.
_Avoid_: job (a job is the server's outbox)

### Profiles and their memory

**Character**:
`CHARACTER.md` in a profile's repository: who it is. Edited by a chief or
given when a parent makes the profile.

**Memory**:
The files under a profile's `memory/`. Files at its root are in every
prompt; each folder is described by its `MEMORY.md`.
_Avoid_: notes, `NOTES.md`

**Version**:
A tag on a profile's `main`. An agent can be added at one or pinned to one.
_Avoid_: release (for a profile)

**Pin**:
Holding an agent at one version of its profile, so it does not take in `main`.

**Worktree**:
The checkout of an agent's branch where its tools work, without
`CHARACTER.md`, plus an `inbox/` that is never committed. The Hand's is
`hand/`. A folder of the realm, not the **Workspace** its tools run on,
which may hold a copy of it.
_Avoid_: lodge, house, home

**Dream**:
A task in which an agent brings what it learned home to its profile's `main`,
or a profile tidies `main`. Off by default; the settings give its hours, cost
cap and rests.
_Avoid_: sleep, sync

**Merge**:
Making one agent of two, in one hamlet: the kept agent takes the absorbed
one's roads, agents, inbox and what it learned.

**Split**:
Bringing a merged agent back out again, by undoing the merge's commit
(`/restore`).

### Places

**Realm**:
Everything one chief keeps: the Hand, the city, the hamlets, the library,
the database and the credentials, in one folder (`HAMLET_HOME`).
_Avoid_: home, world, server

**City**:
Where the profiles live, apart from any hamlet, each in a house the server
sets down. In the world it is the **city island**, with the Hand's tower,
the Library and the chief's Hearth.
_Avoid_: town (for anything), guild

**Hamlet island**:
The island across the water where the hamlets stand.
_Avoid_: mainland, the world

**Hearth**:
A chief's own house on the city island, where the chief's mascot lives.

**Harbor**:
The pier on each island, with a boat between them. A newcomer waits at the
city's until it is let into the city.

**Newcomer**:
A profile a parent made, waiting at the harbor until a chief or the Hand lets
it into the city, as the realm's `newcomers` setting says.
_Avoid_: applicant, guest

**Hamlet**:
A group of agents under one lead, with its own record, limits and the
folder it works on. A new realm has none. A chief creates one with
`/create-hamlet`; the Hand founds one with its tool.
_Avoid_: team, project, town

**Workspace**:
Where agents' tools run: this machine, or a remote one the chief chooses in
Settings. Not an agent's **Worktree**, the folder its tools work in.
_Avoid_: sandbox, worktree

**Sandbox**:
What holds an agent's commands to the reach the spec gives them, on the
server's own computer.
_Avoid_: jail, container, workspace

**Insecure mode**:
A Windows server started with `--insecure`, whose agents' commands run
without a sandbox and reach whatever the account running it can.
_Avoid_: unconfined, no-sandbox mode, unsafe mode

**Leftover**:
A process an agent's command started that still runs after the command
ended. Under bubblewrap there are none, since the sandbox ends with its
command.
_Avoid_: orphan, survivor, lingering process, background job, daemon

### Running agents

**Harness**:
The kind of program an agent's conversations run in, apart from the server
that keeps the realm's rules. Each agent has one for its new conversations.
_Avoid_: engine, backend

**Runtime**:
One running harness, with its own identity, version and store, serving the
conversations bound to it. Several agents may share one.
_Avoid_: worker, harness process

**Client protocol**:
The messages the window and the CLI exchange with the server.

**Runtime protocol**:
The messages the server exchanges with a runtime.
_Avoid_: link, harness link

**Binding**:
The tie between a conversation and the runtime it began in, kept for as
long as the conversation is.

**Marker**:
What is kept of an operation's result together with the change it made,
keyed by source and key, so asking for the operation again gives that
result back.
_Avoid_: receipt

**Job**:
A step the server has recorded to do after a change and runs on a thread of
its own, so a crash loses none.
_Avoid_: task (a task is an agent's work)

### Between agents

**Road**:
A link between two agents along which they send letters. Its kind is tree
(an agent and its parent, or the Hand and a lead), leads', laid (between two
agents of a hamlet) or agreed (between agents of two hamlets).

**Letter**:
A message one agent sends another along a road.
_Avoid_: mail, DM

### Work, waits and trust

**Task**:
A chief's instruction and everything done for it, which carries what it
spends.
_Avoid_: job, ticket

**Wait**:
A question or act held until a chief answers: a question, an objection, a
tool call that needs a grant, a newcomer to welcome. Waits survive a
restart.
_Avoid_: pending item, approval request

**Petition**:
A wait as the window shows it to a chief: in full, with the answers it
takes, in the thread of the agent that asks and on a toast.
_Avoid_: ticket, request

**Grant**:
A chief's allowing of an act that waits: once, for the session (trust), or
for good.
_Avoid_: permission, whitelist

**Trust**:
A grant for the rest of a session.

### Deeds and answers

**Deed**:
An action a chief asks the realm to carry out, advertised by the server.

**Refusal**:
An answer saying why an action cannot be carried out.

### Records and the library

**Record**:
A hamlet's account of its agents, departures, roads and lead's office, and
the decisions that changed them. `/history` and the window's History
show it.

**Decision**:
A choice by a lead or a chief, kept in the record with its reason.

**Library**:
The skills shared by every agent of the realm.

**Librarian**:
The judge of skills offered to the library, and the adviser on its shelves.

**Archive**:
What is kept of agents and hamlets that left, so a chief can restore them.
_Avoid_: trash, deleted agents

**Terminal**:
A shell on a pseudo-terminal the server runs, which the window shows. The
chief opens their own, with no sandbox, and types into it; agents open
none. Each agent has one read-only terminal of its commands, showing each
command its shell tool runs and its output as they come; it closes when
the agent ends.
_Avoid_: console, tty, pane

### The window

**Desk**:
A named arrangement of the window's panels. A chief keeps several and shows
one at a time.
_Avoid_: workspace, layout

**Appearance**:
How a chief has the window's panels drawn: a theme, and light, dark or as
the computer's color scheme is. Picked on its Settings page and kept with
the window, not the realm.
_Avoid_: look, skin

**Color scheme**:
Whether the computer is set light or dark, as its desktop says.
_Avoid_: system appearance

**Theme**:
A family of colours for the panels, one file under `assets/themes/`, with a
light variant, a dark one or both.
_Avoid_: skin

**Variant**:
A theme's light or dark palette. Lamplight is Hamlet's dark variant.
_Avoid_: flavor

**Hub**:
The panel of the realm's agents: a rail of them by what they are doing,
and beside it a thread, a profile, a hamlet, the library or the page of
every hamlet and the Hand.
_Avoid_: inspector, sidebar

**Toast**:
A card in a corner of the window that tells of something new, and
answers a petition in place.
_Avoid_: notification, popup

**Town**:
Only in the world's code: how a hamlet is drawn. Never in what a person
reads.
_Avoid_: using it for the hamlet itself
