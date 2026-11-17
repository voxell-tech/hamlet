# The society and the realm

Agents run on the server. The window receives the Hand, the scholars, the
hamlets with their hires and roads, the library and the waits through the
client protocol; it reads no records from the realm's disk. The glossary is
[`CONTEXT.md`](../CONTEXT.md), and what the society must do is in
[`spec/hamlet.md`](spec/hamlet.md).

## The Hand and the city

A realm is made with the Hand, an agent of its own that is always there
and sits above every mayor. It has a lodge, `hand/`, and a scholar that no
chief hires out; its character starts from `crates/hamlet_server/assets/hand.md`. It is the agent
`hamlet chat` talks to unless told otherwise, and a new realm has no hamlet,
so a chief asks the Hand, or types `/create-hamlet`, to make the first. It has
a tool for each of the chief's acts on the realm (H35) except signing in,
credentials, endpoints and the network, answering what waits, grants, and its
own model, effort and fast tier. Its changes to a scholar's `main`, a purge,
removing or erasing anything, a hamlet's tier, folder, mode, limits and
budget, and the realm's settings and charter wait for a chief
(`HAND_RESERVED` in `oversight/gate.rs`), as every reserved act does.

The city holds the scholars. A scholar is a git repository
(`scholars/<id>.git`) with a character, memory and skills, and a house in the
city. A chief makes one with `/scholar new <name>[: <character>]`, a lead
with `create_scholar`, or with `hire` given a new name, which makes a blank
scholar from the realm's default character (Settings); one a lead or the Hand
makes belongs to the chief whose task the turn works on. A scholar is its id:
its address is `scholar#<id>`. Its name is a label of one line, 1 to 40
characters with no colon, which may repeat and which `/name-scholar
<scholar#id> <name>` changes, a decision of the chief's like `/name-hamlet`.
`/scholars` lists them, and `/remove-scholar`, `/restore-scholar` and
`/erase-scholar` archive one with its hires, bring it back, or erase it for
good.

The server sets each new house down itself, at the first free spot on a
spiral out from the city's middle, its door toward the middle
(`society/city.rs`); `/move-house <scholar#id> <x> <z> [degrees]` sets it down
elsewhere, on a spot clear of the middle and of every house. One a chief or
the Hand makes moves in at once. One a lead makes is a newcomer, which the
`newcomers` setting lets in (H45): at once (`open`); after a chief welcomes
it from a petition (`chief`, the default); or as the Hand judges it in an
errand (`hand`, `calls/society/welcome.rs`). A newcomer waits at the
harbor, its hires working all the while. `/welcome` lets a scholar at the
harbor in and `/turn-away` sends one back, undoing the Hand's verdicts.

`/make-scholar <address> [name]`, Make Scholar in a hire's menu, or a lead's
or the Hand's `make_scholar` makes a scholar of the city of what a hire, living
or ended, has learned (`commands/promotion.rs`, `city::make_of_hire`). A blank
hire's own scholar joins the city, renamed when a name is given, and a job
merges the hire's branch into its `main`; the hire stays its hire. A city
scholar's hire starts a new scholar instead, forked from its branch's tip
after its lodge is committed: the hire's memory and skills with the
character of the scholar it came from, named as the hire is called unless a
name is given. The hire stays a hire of its old scholar. Either arrives as a
scholar newly made does: housed at once when a chief or the Hand makes it, a
newcomer when a lead does. The record keeps it as `made-scholar`, and the new
scholar is removed or turned away as any other is.

A chief reads and edits a scholar's character and memory through the
Hub or `hamlet edit character <scholar#id>` and `hamlet edit memory`, and
each save is a commit on `main`. `/tag` names a version, `/versions` lists
them, `/pin` holds a hire at one, `/revert` undoes a commit, `/fork` starts a
new scholar from a version and `/purge` takes a path out of the whole history.
The Hand's `read_scholar`, `edit_scholar`, `tag`, `versions`, `pin`,
`revert`, `fork` and `purge` do the same on a chief's behalf; `edit_scholar`,
`tag`, `revert` and `purge` wait for the chief. Its `welcome` and `turn_away`
answer for newcomers, and its prompt lists those at the harbor.

## Hamlets, hires and the tree

`/create-hamlet [scholar#id|blank] <name>[: purpose]` creates a hamlet named
`<name>`. Its mayor is a hire of an existing scholar, or a blank agent named
Mayor when none is named; with no purpose it is there to run the hamlet. A
hamlet's id is a UUID made when it is created, never reused, even after
erasure; commands name a hamlet by it, and `/hamlets` lists each with its id.
Its name is a label that may repeat and may hold spaces but no colon, and is
what the window shows; `/name-hamlet <hamlet> <name>` changes it.
`/hire <scholar#id>[@<version>]|blank [in <hamlet>] [under <address>] [purpose]`
hires a scholar into the hamlet you are in, up to the hamlet's tier of 50,
100, 200 or 500 (`/tier`); the same scholar can be hired more than once.
`blank` hires a blank agent instead, as an agent's `hire` with no scholar
does: a scholar of its own, made for that hire alone, which is not of the
city, so it has no house, never waits at the harbor and is not listed with
the city's scholars (`Scholar.arrival` is `apart`). Reasons for a decision
may always be left out. A
hire a chief makes without a purpose takes the first line of the first message
it is given, from the chief or by another agent, as its purpose. An agent always says
what it hires for.

An address names an agent by ids and never by a name: `hand`, `mayor@<hamlet>`
(the mayor of the hamlet with that id), `hire#<id>` and `worker#<id>`. A
scholar is `scholar#<id>`. Wherever an agent is shown to an agent or a chief,
a prompt, a notice, a message's heading or a tool's result, its address comes
first and its name beside it, as `hire#3 (Mira)`: a hire's nickname, else its
scholar's name.
Hands and leads see the scholars they may hire in their prompts, as
`scholar#<id> (<name>)`, since a name finds nothing.

A hire's branch and lodge are its own. Its tools write the lodge, a turn that
changed it is committed to the branch, and the diff against `main` is what it
learned, which the Hub shows under a hire's memory. The hires of a hamlet form a tree under
the mayor, as deep as its leads make it. A hire with hires under it is a
lead, with the mayor's powers over its subtree: it hires, creates scholars,
makes scholars of its hires (`make_scholar`), ends and merges hires, chooses their models, turns tools off and sets
allowances (`set_model`, `set_tools`, `set_allowance`), gives them nicknames
the world shows them by (`nickname`, or a chief's `/nickname`), and lays and
takes up roads among them. The mayor can be given to another hire with `/mayor`; the
old mayor and its hires go under the new one.

A worker is started for one task by any agent but a worker (`start_worker`)
or by a chief (`/worker`). It is a child of its starter, has the built-in
tools its starter gave it and no society tools, works in the hamlet's folder
and a scratch folder, and ends with its answer, which goes back to its starter
as the tool's answer or as a response in its starter's conversation. A chief
watches its own worker's conversation.

## Roads and messages

Tree roads join a hire to its parent and its hires, and the Hand to each
mayor; mayors have roads to each other; a lead or a chief lays a road between
two hires of one hamlet, which costs the hamlet and the task that laid it. A
hire proposes one to its lead with `propose_road`. Hires of different hamlets
have a road once both mayors call `agree_road`, while the realm's limits allow
it.

`notify` sends a notification and `request` a request, which expects a
response. Neither waits: each goes into the receiver's one conversation as a
turn of its own, under a heading that names its sender, as
`[A notification from hire#3 (Mira)]`. The receiver's final answer to a
request's turn goes back to the sender as the response, which starts a turn
of the sender's (H30, H31). A request that fails, because the receiver had
no model it could use, failed, ended or was refused, comes back as an error
response: it is shown in the sender's chat and is in its context, but starts
no turn (`conversations/messages.rs`). An agent at rest with a request not yet
answered shows "Waiting on" the agent it sent it to. Either can name `paths`,
files or folders the sender may read, which land in the receiver's
`inbox/from-<sender>/`; a send over 32 MiB or 1000 files waits for a chief.
The task follows their work and charges, and a response is delivered even
when the message cap has been reached. Messages between hamlets are paid by
the sender's hamlet.

A conversation takes one turn at a time. The chief's lines and retries go
ahead of the notifications, requests and responses that wait; those take
their turns as they came. In the chat their authors are shown as agents,
never as the chief. See [`chat.md`](chat.md) for the queue, a stop and
watches.

An address is read with or without the name shown beside it, as
`hire#2 (Mayor)` or `scholar#4 (Mira)`, the form "Copy Agent ID" copies, so
a copied ID works as a tool's argument or in a command.

## Ending, merging and restoring

`/end` ends a hire: its turns stop, its waits lapse, its grants are taken
back, its lodge is committed one last time and archived, and its hires go to
its parent. What it learned stays on its branch. `/end <address> dream`, End
with Dream in the window's question, or `end_hire` with `dream` also has it
dream once more right after, whatever the settings say about dreams, to bring
that home to its scholar's `main`; the choice is kept with the hire
(`hire.ending_dream`). The mayor cannot be ended; give the office away first. A lead,
the Hand included, may not end or merge a hire on a chief's task (H7): while
an input of that task is still to end in its conversations, or until the
hamlet's `protectedMinutes` have passed since the chief's instruction was
answered. A chief may.
`/merge <a> into <b>` makes one hire of two of a hamlet: the kept hire takes
the absorbed one's laid roads, hires and inbox, and takes in what it learned.
Hires of one scholar merge their branches; hires of different scholars bring
what the absorbed hire learned into the kept branch, a clashing file kept as
`<name>.from-<absorbed>` and a clashing skill as `<skill>-from-<absorbed>`,
with the absorbed hire as `hire-<id>`. The kept hire's conversation is
given the absorbed one's without a turn: who said what in it, from its
latest summary on and at most 24000 characters, with a folded block in
the chat that opens it. The absorbed hire's conversation stays as it was.
`/restore` brings an ended hire back under the same id, or splits a
merged one out again by undoing the merge's commit. `/erase` deletes an ended hire's lodge and branch for good. A
message to a hire that has left is refused with how it left.

`/remove-hamlet` ends every hire of a hamlet and keeps its tree and lodges;
`/restore-hamlet` brings them back while their scholars live, and
`/erase-hamlet` deletes the lodges and branches. Removed hamlets appear
apart from the living ones in the overview.

Once the server has erased a hire, a scholar or a hamlet the chief asked
to erase in the window, a toast says "<name> was erased", the selection
lets go of it, and the Hub leaves its page for its lead's thread, else
its hamlet's page, else the realm's (`screens/hub/erasure.rs`). A page
whose subject is gone for any other reason turns to the realm's.

## Memory, reflection and dreams

An agent reflects every few answered turns of a chief's lines in its
conversation, or when a chief asks (`/reflect`), and keeps what it learned in
its lodge's `memory/` and `skills/`. That stays on its branch. When dreams are
turned on in the settings, a hire that is idle and has learned something its
scholar lacks dreams, within the chief's hours: it is given its lodge and a
checkout of `main`, and decides what goes to the scholar and what stays. A
scholar with living hires dreams once a day to tidy `main`. One dream runs at
a time. A dream asked for runs whatever the settings say: `/dream`, the
Hand's `dream`, Dream in a hire's menu, or Note what its hires learned on a
scholar's page. Asked of a scholar, each living hire of it that learned what
it lacks dreams first, then the scholar tidies. One asked while another runs
waits its turn in `asked_dream`, ahead of the last dream of a hire ended with
one, which comes ahead of the scheduled ones (`conversations/dreams.rs`, `next`). A dream is a
task with a cost cap, and its commits are named after it.

## The shared library and chief oversight

The library is a repository, checked out read-only for every agent, which
starts with the shipped skills. An agent offers a skill using `submit_skill`;
the librarian judges it in one model call with no tools, and `ACCEPT` as the
first word shelves it. Other answers refuse it. `request_librarian` sends the
librarian a request about the shelf, and `/unshelve` takes a skill out. The shelf and each decision are kept
by the server.

A chief's instruction is recorded before it starts. In its own conversation
an agent may object once and wait for go or withdraw. When a chief instructs a
hire directly, its lead's review, as the limits set, runs beside the work and
may raise its own objection. An agent can put questions to a chief
(`request_chief`) from any turn of its conversation, and a worker from the
turn of a chief's own line, and wait for reply or dismiss. These waits appear in the Hub and survive a
server restart. Tool calls wait too (H11): the network, manual-mode edits and
commands, the acts a chief's limits reserve, and the Hand's acts on scholars.
Answering allows once, for the agent's conversation, or always; a rewind
forks a conversation that trusts nothing yet. `/grants` lists and `/revoke`
takes back what was allowed for good.

## Several hamlets

Another hamlet's hire is reached by its `hire#<id>` like any other,
`mayor@<hamlet>` addresses the mayor of the hamlet with that id, and
`/hamlet <hamlet>` switches the terminal to it. `/agent <address>` finds any agent of the realm.
A mayor sees the other hamlets in its prompt.

Removed hamlets retain their lodges and records and appear under CLOSED DOWN
in the overview. RESTORE brings one back. DELETE FOR GOOD asks first and
permanently erases its lodges; that question is discarded on restore or
leaving the page.

The server sends whole replacement states when the library shelf or the
removed-hamlet list changes, or a hamlet leaves the live list. The client
reconciles surviving entities and removes those no longer sent. The terminal
also replaces its addresses from those states.
