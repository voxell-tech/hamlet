# Hamlet

AI agents that live as hamlets, overseen by the humans they belong to and
shown as a world of hamlets.

## Language

### People

**Chief**:
A human a realm answers to. Can overrule anything. Never an agent.
_Avoid_: user, owner, admin

### Agents

**Agent**:
An AI the server runs. The Hand and workers are agents too.
_Avoid_: hire, citizen, resident, bot, member, employee

**Hand**:
The chiefs' own agent, above every lead.
_Avoid_: assistant, root agent, admin agent

**Profile**:
Who an agent is: a git repository of a character, memory and skills.
Named by id, `profile#<id>`.
_Avoid_: scholar, persona, template, house

**Blank agent**:
An agent of a hamlet whose profile is made for it alone, outside the
city.
_Avoid_: blank profile, temp agent

**Address**:
How an agent or profile is named, by id and never by name: `hand`,
`lead@<hamlet>`, `agent#<id>`, `worker#<id>`, `profile#<id>`.
_Avoid_: handle, username

**Name**:
The changeable, repeatable label an entity is shown by. It never finds
anything.
_Avoid_: nickname, alias

**Lead**:
The agent at the top of a hamlet's tree.
_Avoid_: mayor, orchestrator, main agent

**Parent**:
An agent with agents under it, or the Hand.
_Avoid_: lead (the top of a hamlet), manager, supervisor

**Worker**:
An agent started for one task, which ends with its answer.
_Avoid_: subagent, helper, runtime

**Conversation**:
What an agent's model reads and answers, kept by its harness.
_Avoid_: session, chat history, thread (the Hub's thread is where one is
shown)

**Queued**:
Said of an input its conversation has not taken up yet.
_Avoid_: pending, backlog

**Stopped**:
Said of an agent a chief stopped: nothing queued starts until resumed.
_Avoid_: paused, frozen

**Compaction**:
A summary standing for the earlier part of a conversation.
_Avoid_: new session, reset, clearing

**Errand**:
A one-off conversation not an agent's, such as a parent's review.
_Avoid_: job (a job is the server's outbox)

### Profiles and their memory

**Character**:
`CHARACTER.md` in a profile's repository.

**Memory**:
The files under a profile's `memory/`.
_Avoid_: notes, `NOTES.md`

**Version**:
A tag on a profile's `main`.
_Avoid_: release (for a profile)

**Pin**:
Holding an agent at one version.

**Worktree**:
The checkout of an agent's branch, a folder of the realm. Not the
**Workspace**.
_Avoid_: lodge, house, home

**Dream**:
A task in which an agent brings what it learned to its profile's `main`.
_Avoid_: sleep, sync

**Merge**:
Making one agent of two. **Split** undoes it (`/restore`).

### Places

**Realm**:
Everything one chief keeps, in one folder (`HAMLET_HOME`).
_Avoid_: home, world, server

**City**:
Where the profiles live, apart from any hamlet. In the world, the **city
island**.
_Avoid_: town (for anything), guild

**Hamlet island**:
The island where the hamlets stand.
_Avoid_: mainland, the world

**Hearth**:
A chief's own house on the city island.

**Harbor**:
The pier on each island.

**Newcomer**:
A profile a parent made, waiting at the harbor until let in.
_Avoid_: applicant, guest

**Hamlet**:
A group of agents under one lead, with its own record, limits and folder.
_Avoid_: team, project, town

**Workspace**:
Where agents' tools run: this machine, or a remote one. Not a
**Worktree**.
_Avoid_: sandbox, worktree

**Sandbox**:
What holds an agent's commands to the reach the spec gives them.
_Avoid_: jail, container, workspace

**Insecure mode**:
A Windows server started with `--insecure`: commands run without a sandbox.
_Avoid_: unconfined, no-sandbox mode, unsafe mode

**Leftover**:
A process an agent's command started that outlives the command.
_Avoid_: orphan, survivor, lingering process, background job, daemon

### Running agents

**Harness**:
The kind of program an agent's conversations run in, apart from the server.
_Avoid_: engine, backend

**Runtime**:
One running harness, serving the conversations bound to it.
_Avoid_: worker, harness process

**Client protocol**:
The messages the window and the CLI exchange with the server.

**Runtime protocol**:
The messages the server exchanges with a runtime.
_Avoid_: link, harness link

**Binding**:
The tie between a conversation and the runtime it began in.

**Marker**:
What is kept of an operation's result, keyed by source and key, so asking
again gives it back.
_Avoid_: receipt

**Job**:
A step the server has recorded to do after a change, so a crash loses none.
_Avoid_: task (a task is an agent's work)

### Between agents

**Channel**:
A link between two agents along which they send notifications and
requests. The world draws one as a road.
_Avoid_: road, link

**Notification**:
A message along a channel (`notify`) that expects no response.
_Avoid_: letter, mail, DM

**Request**:
A message along a channel that expects a response (`request`).
_Avoid_: question (a question is what an agent puts to a chief), ask

**Response**:
The answer to a request.
_Avoid_: reply, answer (for one agent's to another)

### Work, waits and grants

**Task**:
A chief's instruction and everything done for it.
_Avoid_: job, ticket

**Wait**:
A question or act held until a chief answers. Waits survive a restart.
_Avoid_: petition, pending item, approval request

**Grant**:
A chief's allowing of an act that waits: once, for the conversation, or
for good.
_Avoid_: permission, whitelist

**Session grant**:
A grant for the rest of an agent's conversation.
_Avoid_: trust

### Actions and rejections

**Action**:
Something a chief asks the realm to carry out, advertised by the server.
_Avoid_: deed, operation

**Rejection**:
An answer saying why an action cannot be carried out.
_Avoid_: refusal, denial

### Records and the library

**Record**:
A hamlet's account of its agents, channels and lead, with the
**Decision**s (a lead's or chief's choice and its reason) that changed
them.

**Library**:
The skills shared by every agent of the realm. The **Librarian** judges
those offered to it.

**Archive**:
What is kept of agents and hamlets that left, so they can be restored.
_Avoid_: trash, deleted agents

**Terminal**:
A shell on a pseudo-terminal the server runs, which the window shows.
_Avoid_: console, tty, pane

### The window

**Desk**:
A named arrangement of the window's panels.
_Avoid_: workspace, layout

**Appearance**:
A theme, and light, dark or the color scheme. Kept with the window.
_Avoid_: look, skin

**Color scheme**:
Whether the computer is set light or dark.
_Avoid_: system appearance

**Theme**:
A family of colours for the panels, a file under `assets/themes/`.
_Avoid_: skin

**Variant**:
A theme's light or dark palette.
_Avoid_: flavor

**Hub**:
A panel of the realm's agents: a rail, and beside it a thread, profile,
hamlet or the library. One Hub of each desk follows the world's selection
and opens what toasts, menus and links name.
_Avoid_: inspector, sidebar

**Toast**:
A card in a corner of the window that tells of something new.
_Avoid_: notification, popup

**Town**:
Only in the world's code: how a hamlet is drawn. Never in what a person
reads.
_Avoid_: using it for the hamlet itself
