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
founds hamlets and looks after profiles when a chief asks. Its reserved
acts wait for a chief.
_Avoid_: assistant, root agent, admin agent

**Profile**:
Who an agent is when it is not at work: a git repository holding a
character, memory and skills. Its `main` is what it knows. Many hires can
be of one profile. A profile is its id, `profile#<id>`; its name is a label
that a chief changes and that may repeat.
_Avoid_: scholar, persona, template, house

**Hire**:
A profile working in a hamlet, on a branch of its own and in its own
filesystem, for a purpose a chief may leave out. Hires form a tree under
the mayor.
_Avoid_: member, employee, citizen

**Blank agent**:
A hire, the mayor included, that is not of a profile of the city: its
profile is made for it alone, has no house, never waits at the harbor and
is not listed with the city's profiles.
_Avoid_: blank profile, temp hire

**Address**:
How a chief or an agent names an agent or a profile, by ids and never by a
name: `hand`, `mayor@<hamlet>` (by the hamlet's id), `hire#<id>`,
`worker#<id>`, `profile#<id>`. Wherever an agent is shown, its address comes
first and its name beside it. An actor's address is another thing: its
kind and its key.
_Avoid_: handle, username

**Name**:
The words a profile, a hamlet, a hire or a worker is shown by: a profile's
name, a hamlet's name, a hire's or a worker's nickname. Metadata a chief
changes and that may repeat; it never finds anything. Everyone but the Hand
has one a chief can change. A profile and a hamlet always have one; a
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
A short-lived agent outside the tree that runs one turn for the realm and
then ends: a lead's review of a chief's line, the librarian's answer, a
dream.
_Avoid_: job

### Profiles and their memory

**Character**:
`CHARACTER.md` in a profile's repository: who it is. Edited by a chief or
given when a lead makes the profile.

**Memory**:
The files under a profile's `memory/`. Files at its root are in every
prompt; each folder is described by its `MEMORY.md`.
_Avoid_: notes, `NOTES.md`

**Version**:
A tag on a profile's `main`. A hire can be hired at one or pinned to one.
_Avoid_: release (for a profile)

**Pin**:
Holding a hire at one version of its profile, so it does not take in `main`.

**Agent's filesystem**:
An agent's own folder, where its file tools work: `memory/`, `skills/` and
`inbox/`, beside the agent's database at `actors/agent/<id>.fs/` in the
realm folder, `<id>` being the agent's number. The Hand's is
`actors/agent/1.fs/`. Only the agent writes it, through its file tools and
its own commands, and no path or link leads a file tool out of it.
_Avoid_: lodge, house, workspace, home

**Dream**:
An errand in which a hire brings what it learned home to its profile's
`main`, or a profile tidies `main`. Off by default; the settings give its
hours, cost cap and rests.
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
A profile a lead made, waiting at the harbor until a chief or the Hand lets
it into the city, as the realm's `newcomers` setting says.
_Avoid_: applicant, guest

**Hamlet**:
A group of hires under one mayor, with its own record, limits and the
folder it works on. A new realm has none. A chief creates one with
`/create-hamlet`; the Hand founds one with its tool.
_Avoid_: team, project, town

**Workspace**:
Where agents' tools run: this machine, or a remote one the chief chooses in
Settings. On the actor host it is one actor per machine.
_Avoid_: sandbox, lodge

**Reach**:
What one agent's tool calls may touch on a machine, and which of them
wait for a chief: the folder its hamlet works on and its mode, the
network, and trust. Only the realm sets it, each time as a newer version.
_Avoid_: permissions

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
What an agent's conversations run in, inside its actor on the Node host
(`hosts/node`), apart from the realm that keeps the rules. The agent opens
it when it wakes.
_Avoid_: engine, backend, runtime

**Client protocol**:
The messages the window and the CLI exchange with the server.

**Marker**:
What is kept of a call's answer with the change it made, under its caller
and its id, so the same call again gives that answer back.
_Avoid_: receipt

### Actors

**Actor runtime**:
What runs actors: the `hamlet_actor` and `hamlet_actor_*` crates,
`hamlet_host`, the host that runs them in one process, and
`hamlet_address`, an actor's address.
_Avoid_: runtime

**Actor**:
One instance of an actor kind, found by its kind and its key, with its
state in a file of its own. A kind and a key take only lower-case
letters, digits, `-`, `_` and `.`, and do not start with `.`.

**Call**:
An actor's action run for its caller, which the host stamps on it:
another actor, a client, or none for the program that opened the host.
A call made for a task carries that task in its own message. A call
never makes its actor.

**Subscription**:
A client's following of one event of an actor: each broadcast of the
event, in order, until the actor closes it or stops. Events a slow
reader or a lost link misses are not kept; a gap in their place tells
the reader to read the actor's state again.

**Set-aside timer**:
An actor's one-shot timer that used up its tries without its handler's
reply. The actor keeps it, and nothing runs it again until a re-arm puts
it back. The actor's destroy removes it.
_Avoid_: dead letter (a letter is what one agent sends another)

**Copy**:
What an agent keeps of a document the realm owns: its charter, its
character, its model or its settings, each with the realm's version. The
realm makes an agent with its copies, sends a newer one, and the agent
reads them again at each wake.
_Avoid_: cache, mirror

### Between agents

**Road**:
A link between two agents along which they send letters. Its kind is tree
(a hire and its lead, or the Hand and a mayor), mayors', laid (between two
hires of a hamlet) or agreed (between hires of two hamlets).

**Letter**:
A message one agent sends another along a road.
_Avoid_: mail, DM

**Parcel**:
The files a letter sends with it. The realm copies them into its own
store unchanged, and the receiver fetches them in parts into its inbox.
_Avoid_: gift, attachment

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

**Offer**:
A skill an agent gives the library to be judged, its files sent in parts.
The librarian's verdict puts it on the shelves or refuses it, and the parts
of an offer still under way are dropped when its hire ends.
_Avoid_: gift, submission

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
