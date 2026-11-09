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
An AI worker the server runs: the Hand, a hire or a worker.
_Avoid_: citizen, resident, bot

**Hand**:
The chiefs' own agent. It is always there, sits above every mayor, and
founds hamlets and looks after scholars when a chief asks. Its reserved
acts wait for a chief.
_Avoid_: assistant, root agent, admin agent

**Scholar**:
Who an agent is when it is not at work: a git repository holding a
character, memory and skills. Its `main` is what it knows. Many hires can
be of one scholar. A scholar is its id, `scholar#<id>`; its name is a label
that a chief changes and that may repeat.
_Avoid_: persona, profile, template, house

**Hire**:
A scholar working in a hamlet, on a branch of its own and in a lodge, for a
purpose a chief may leave out. Hires form a tree under the mayor.
_Avoid_: member, employee, citizen

**Blank agent**:
A hire, the mayor included, that is not of a scholar of the city: its
scholar is made for it alone, has no house, never waits at the harbor and
is not listed with the city's scholars.
_Avoid_: blank scholar, temp hire

**Address**:
How a chief or an agent names an agent or a scholar, by ids and never by a
name: `hand`, `mayor@<hamlet>` (by the hamlet's id), `hire#<id>`,
`worker#<id>`, `scholar#<id>`. Wherever an agent is shown, its address comes
first and its name beside it.
_Avoid_: handle, username

**Name**:
The words a scholar, a hamlet, a hire or a worker is shown by: a scholar's
name, a hamlet's name, a hire's or a worker's nickname. Metadata a chief
changes and that may repeat; it never finds anything. Everyone but the Hand
has one a chief can change. A scholar and a hamlet always have one; a
hamlet's id is a UUID no one reads.

**Nickname**:
What a hire or a worker is called in the world, given by a chief, or for a
hire by the Hand or a lead above it. Its address, `hire#<id>` or
`worker#<id>`, stays the same.
_Avoid_: alias

**Mayor**:
The hire at the top of a hamlet's tree: the one the chief talks to first.
Given to another hire with `/mayor`.
_Avoid_: orchestrator, main agent

**Lead**:
A hire with hires under it, or the Hand. It has the mayor's powers over its
subtree: hiring, ending, merging, choosing models and tools, setting
allowances and laying roads.
_Avoid_: manager, supervisor

**Worker**:
An agent started for one task by another agent or a chief. A child of its
starter, with a scratch folder and no society tools, which ends with its
answer.
_Avoid_: subagent, helper, runtime

**Errand**:
A one-off conversation that is not an agent's: the librarian's answer or a
lead's review.
_Avoid_: job (a job is the server's outbox)

### Scholars and their memory

**Character**:
`CHARACTER.md` in a scholar's repository: who it is. Edited by a chief or
given when a lead makes the scholar.

**Memory**:
The files under a scholar's `memory/`. Files at its root are in every
prompt; each folder is described by its `MEMORY.md`.
_Avoid_: notes, `NOTES.md`

**Version**:
A tag on a scholar's `main`. A hire can be hired at one or pinned to one.
_Avoid_: release (for a scholar)

**Pin**:
Holding a hire at one version of its scholar, so it does not take in `main`.

**Lodge**:
The checkout of a hire's branch where its tools work, without
`CHARACTER.md`, plus an `inbox/` that is never committed. The Hand's is
`hand/`.
_Avoid_: house, workspace, home

**Dream**:
A task in which a hire brings what it learned home to its scholar's `main`,
or a scholar tidies `main`. Off by default; the settings give its hours, cost
cap and rests.
_Avoid_: sleep, sync

**Merge**:
Making one hire of two, in one hamlet: the kept hire takes the absorbed
one's roads, hires, inbox and what it learned.

**Split**:
Bringing a merged hire back out again, by undoing the merge's commit
(`/restore`).

### Places

**Realm**:
Everything one chief keeps: the Hand, the city, the hamlets, the library,
the database and the credentials, in one folder (`HAMLET_HOME`).
_Avoid_: home, world, server

**City**:
Where the scholars live, apart from any hamlet, each in a house the server
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
A scholar a lead made, waiting at the harbor until a chief or the Hand lets
it into the city, as the realm's `newcomers` setting says.
_Avoid_: applicant, guest

**Hamlet**:
A group of hires under one mayor, with its own record, limits and the
folder it works on. A new realm has none. A chief creates one with
`/create-hamlet`; the Hand founds one with its tool.
_Avoid_: team, project, town

**Workspace**:
Where agents' tools run: this machine, or a remote one the chief chooses in
Settings.
_Avoid_: sandbox, lodge

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
(a hire and its lead, or the Hand and a mayor), mayors', laid (between two
hires of a hamlet) or agreed (between hires of two hamlets).

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
A hamlet's account of its hires, departures, roads and mayor's office, and
the decisions that changed them. `/history` and the window's History
show it.

**Decision**:
A choice by a mayor or a chief, kept in the record with its reason.

**Library**:
The skills shared by every agent of the realm.

**Librarian**:
The judge of skills offered to the library, and the adviser on its shelves.

**Archive**:
What is kept of hires and hamlets that left, so a chief can restore them.
_Avoid_: trash, deleted agents

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
and beside it a thread, a scholar, a hamlet, the library or the page of
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

**Web**:
A panel that shows a web page: an agent's page, a site an agent serves
while it builds, or any site. Every Web panel shares the chief's one
browsing session.
_Avoid_: browser, webview
