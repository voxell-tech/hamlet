# The society and the realm

Agents run on the server. The window receives the Hand, the scholars, the
hamlets with their hires and roads, the library and the waits through the
client protocol; it reads no records from the realm's disk. The glossary is
[`CONTEXT.md`](../CONTEXT.md), and what the society must do is in
[`spec/hamlet.md`](spec/hamlet.md).

## The Hand and the city

A realm is made with the Hand, an agent of its own that is always there
and sits above every mayor. It has a lodge, `hand/`, and a scholar that no
chief hires out; its character starts from `assets/hand.md`. It is the agent
`hamlet chat` talks to unless told otherwise, and a new realm has no hamlet,
so a chief asks the Hand, or types `/found`, to make the first. Its changes to
a scholar's `main` and its removal of a hamlet wait for a chief, as every
reserved act does.

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

A chief reads and edits a scholar's character and memory through the
Hub or `hamlet edit character <scholar#id>` and `hamlet edit memory`, and
each save is a commit on `main`. `/tag` names a version, `/versions` lists
them, `/pin` holds a hire at one, `/revert` undoes a commit, `/fork` starts a
new scholar from a version and `/purge` takes a path out of the whole history.
The Hand's `read_scholar`, `edit_scholar`, `tag` and `revert` do the same on a
chief's behalf, each waiting for the chief.

## Hamlets, hires and the tree

`/found <hamlet> [scholar#id] [purpose]` founds a hamlet. Its mayor is a hire
of an existing scholar, and of a new scholar named Mayor when none is named;
with no purpose it is there to run the hamlet. The
hamlet's id is lowercase letters, digits and hyphens, never reused, even
after erasure; `hand`, `mayor`, `scholar` and `worker` are not ids.
`/hire <scholar#id>[@<version>] [under <address>] [purpose]`
hires a scholar into the hamlet you are in, up to the hamlet's tier of 50,
100, 200 or 500 (`/tier`); the same scholar can be hired more than once. A
hire a chief makes without a purpose takes the first line of the first message
it is given, from the chief or by letter, as its purpose. An agent always says
what it hires for.

An address names an agent by ids and never by a name: `hand`, `mayor@<hamlet>`
(the hamlet's mayor), `<hamlet>#<number>` (the hire with that number in the
hamlet, the mayor being 0) and `worker#<id>`. A scholar is `scholar#<id>`.
Wherever an agent is shown to an agent or a chief, a prompt, a notice, a
letter's header or a tool's result, its address comes first and its name beside
it, as `orchard#3 (Mira)`: a hire's nickname, else its scholar's name.
Hands and leads see the scholars they may hire in their prompts, as
`scholar#<id> (<name>)`, since a name finds nothing.

A hire's branch and lodge are its own. Its tools write the lodge, a turn that
changed it is committed to the branch, and the diff against `main` is what it
learned, which the Hub shows under a hire's memory. The hires of a hamlet form a tree under
the mayor, no deeper than the depth limit. A hire with hires under it is a
lead, with the mayor's powers over its subtree: it hires, creates scholars,
ends and merges hires, chooses their models, turns tools off and sets
allowances (`set_model`, `set_tools`, `set_allowance`), gives them nicknames
the world shows them by (`nickname`, or a chief's `/nickname`), and lays and
takes up roads among them. The mayor can be given to another hire with `/mayor`; the
old mayor and its hires go under the new one.

A worker is started for one task by any agent but a worker (`start_worker`)
or by a chief (`/worker`). It is a child of its starter, has the built-in
tools its starter gave it and no society tools, works in the hamlet's folder
and a scratch folder, and ends with its answer, which goes back to its starter
as the tool's answer or as a letter. A chief watches its own worker's
conversation.

## Roads and letters

Tree roads join a hire to its parent and its hires, and the Hand to each
mayor; mayors have roads to each other; a lead or a chief lays a road between
two hires of one hamlet, which costs the hamlet and the task that laid it. A
hire asks its lead for one with `request_road`. Hires of different hamlets
have a road once both mayors call `agree_road`, while the realm's limits allow
it.

`tell` sends a letter without waiting; `ask` waits for the answer. Each
receiver has separate conversations for the sender's letters and questions,
both kept across restarts. The task follows their work and charges, and a
reply is delivered even when the message cap has been reached. Messages
between hamlets are paid by the sender's hamlet.

An agent's session menu in the Hub opens those conversations read-only,
one for each sender's letters and questions. Their authors are shown as agents, never as the
chief. See [`chat.md`](chat.md) for paging and watches.

## Ending, merging and restoring

`/end` ends a hire: its turns stop, its waits lapse, its grants are taken
back, its lodge is committed one last time and archived, and its hires go to
its parent. The mayor cannot be ended; give the office away first. A lead,
the Hand included, may not end or merge a hire on a chief's task (H7): while
an input of that task is still to end in its conversations, or until the
hamlet's `protectedMinutes` have passed since the chief's instruction was
answered. A chief may.
`/merge <a> into <b>` makes one hire of two of a hamlet: the kept hire takes
the absorbed one's laid roads, hires and inbox, and takes in what it learned.
Hires of one scholar merge their branches; hires of different scholars bring
what the absorbed hire learned into the kept branch, a clashing file kept as
`<name>.from-<absorbed>` and a clashing skill as `<skill>-from-<absorbed>`,
with the absorbed hire as `<hamlet>-<number>`.
`/restore` brings an ended hire back under the same id and number, or splits a
merged one out again by undoing the merge's commit. `/erase` deletes an ended hire's lodge and branch for good. A
letter to a hire that has left is refused with how it left.

`/remove-hamlet` ends every hire of a hamlet and keeps its tree and lodges;
`/restore-hamlet` brings them back while their scholars live, and
`/erase-hamlet` deletes the lodges and branches. Removed hamlets appear
apart from the living ones in the overview.

## Memory, reflection and dreams

An agent reflects every few answered turns of a chief chat, when a chief asks
(`/reflect`) or when a conversation is put away, and keeps what it learned in
its lodge's `memory/` and `skills/`. That stays on its branch. When dreams are
turned on in the settings, a hire that is idle and has learned something its
scholar lacks dreams, within the chief's hours: it is given its lodge and a
checkout of `main`, and decides what goes to the scholar and what stays. A
scholar with living hires dreams once a day to tidy `main`. `/dream` runs one
now. A dream is a task with a cost cap, and its commits are named after it.

## The shared library and chief oversight

The library is a repository, checked out read-only for every agent, which
starts with the shipped skills. An agent offers a skill using `submit_skill`;
the librarian judges it in one model call with no tools, and `ACCEPT` as the
first word shelves it. Other replies refuse it. `ask_librarian` asks about the
shelf, and `/unshelve` takes a skill out. The shelf and each decision are kept
by the server.

A chief's instruction is recorded before it starts. In a chief conversation an
agent may object once and wait for go or withdraw. When a chief instructs a
hire directly, its lead's review, as the limits set, runs beside the work and
may raise its own objection. An agent overseen by a chief can ask a question
and wait for reply or dismiss. These waits appear in the Hub and survive a
server restart. Tool calls wait too (H11): the network, manual-mode edits and
commands, the acts a chief's limits reserve, and the Hand's acts on scholars.
Answering allows once, for the session, or always; `/grants` lists and
`/revoke` takes back what was allowed for good.

## Several hamlets

Another hamlet's hire is reached by `mill#<number>`, `mayor@mill` addresses
mill's mayor, and `/hamlet mill`
switches the terminal to it. `/agent <address>` finds any agent of the realm.
A mayor sees the other hamlets in its prompt.

Removed hamlets retain their lodges and records and appear under CLOSED DOWN
in the overview. RESTORE brings one back. DELETE FOR GOOD asks first and
permanently erases its lodges; that question is discarded on restore or
leaving the page.

The server sends whole replacement states when the library shelf or the
removed-hamlet list changes, or a hamlet leaves the live list. The client
reconciles surviving entities and removes those no longer sent. The terminal
also replaces its addresses from those states.
