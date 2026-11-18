# The hamlet: the society of agents

Chiefs, the Hand, the city of profiles, hamlets with their agents, the roads
and letters between them, the tasks and budgets that bound their work, the
library, dreams, and what waits for a chief. One agent on its own is in
[`core.md`](core.md).

## Chiefs

**H23. Rule one.** A chief can overrule anything in its hamlets: stop any
or all of their agents, end and restore their agents or the hamlets
themselves, take back any permission, and bring back the agent ended last.
A chief overrules the Hand, parents and leads alike.

**H22.** A chief's limits bound a hamlet, and only a human changes them;
every check reads them as they are now. They can reserve to a chief what a
parent may otherwise do alone: adding agents, laying and taking up roads,
ending agents and merging them. A reserved act waits for a chief (H11). The realm has
limits of its own: whether roads may be agreed between hamlets, and the
budget of the Hand and of the workers started outside a hamlet (H8).

**H24.** Only a line a human types can be a command. A letter between agents
is words, whatever it begins with.

**H26.** A chief has these commands. A command that names an agent takes its
address (H14); one that names no hamlet means the hamlet of the agent the
chief is talking to.

| Command | Does |
|---|---|
| `/hand`, `/hamlet <hamlet>`, `/agent <address>` | Talk to the Hand, to a hamlet's lead, or to an agent |
| `/to <address> <instruction>` | Instruct an agent directly |
| `/new` | Move at once to a new conversation while the old one finishes its queue (C24) |
| `/resume [n]` | List earlier conversations, or go back to one by its stable number (C24) |
| `/retry`, `/reflect` | Carry on a failed or stopped turn as a visible new input, reflect now (C26, C5) |
| `/stop [address\|all]` | Stop an agent's work, or all agents' (H23) |
| `/queue`, `/allow`, `/always`, `/deny`, `/go`, `/withdraw`, `/reply`, `/dismiss` | Answer what waits (H11, H10) |
| `/grants`, `/revoke <address> <network\|unconfined>` | The lasting permissions, and taking one back |
| `/hamlets`, `/create-hamlet [profile#id\|blank] <name>[: purpose]` | List hamlets with their ids, or create one named `<name>` with a profile as its lead, a blank agent named Lead by default (H6, H36) |
| `/name-hamlet <hamlet> <name>`, `/move-hamlet <hamlet> <x> <z>` | Give a hamlet another name to be shown by, or set it down elsewhere on the hamlet island (H32) |
| `/remove-hamlet <hamlet> [reason]`, `/restore-hamlet <hamlet> [reason]`, `/erase-hamlet <hamlet>` | Remove, restore or permanently erase a hamlet (H19, H20) |
| `/tier <50\|100\|200\|500>` | Set how many agents the hamlet holds (H40) |
| `/lead <address> [reason]` | Give the hamlet's office to another agent (H18) |
| `/add-agent <profile#id>[@<version>]\|blank [in <hamlet>] [under <address>] [purpose]` | Add an agent of a profile to the hamlet, or a blank agent (H40) |
| `/end <address> [reason]`, `/merge <address> into <address> [reason]` | End an agent, or make one of two (H5) |
| `/restore <address> [reason]`, `/erase <address>` | Bring an ended agent back, or split a merged one out again, or erase it for good (H20) |
| `/model <address> <model\|none> [reason]`, `/tools <address> -<tool> +<tool>`, `/allowance <address> <dollars\|none>` | An agent's model, tools and allowance (H29, H41, H44) |
| `/effort <address> <off\|minimal\|low\|medium\|high\|xhigh\|max\|none>`, `/fast <address> <on\|off>` | How hard an agent's model thinks, and whether it asks for the provider's fast tier (H29a) |
| `/nickname <address> [nickname]` | What an agent or a worker is called in the world, or its own name again (H46) |
| `/road <address> <address> [reason]`, `/unroad <address> <address> [reason]` | Lay or take up a road between two agents (H3) |
| `/worker <task>` | Start a worker on a task in the hamlet (H42) |
| `/folder <folder\|none>`, `/mode <manual\|auto>` | The hamlet's folder and its mode (C20, C21) |
| `/budget`, `/reset-budget` | The hamlet's spend, and starting its period again (H8) |
| `/history` | The record: who was added, ended, merged and restored, and the other decisions (H17) |
| `/profiles`, `/profile new <name>[: <character>]`, `/name-profile <profile#id> <name>` | List the city's profiles, make one, or give one a new name (H38) |
| `/welcome <profile#id> [reason]`, `/turn-away <profile#id> [reason]` | Let a profile at the harbor into the city, or send one back there (H45) |
| `/remove-profile <profile#id> <reason>`, `/restore-profile <profile#id> [reason]`, `/erase-profile <profile#id>` | Archive a profile and end its agents, bring it back, or erase it for good (H38) |
| `/move-house <profile#id> <x> <z> [degrees]` | Set a profile's house down elsewhere in the city |
| `/tag`, `/versions`, `/pin`, `/revert`, `/fork`, `/purge` | A profile's versions and history (H39) |
| `/dream <profile#id\|address>` | Have an agent bring its learning home, or a profile tidy, now (H43) |
| `/unshelve <skill> [reason]` | Take a skill out of the library (H33) |

The server advertises its supported command names. The window offers only
the deeds it advertises.

## The Hand

**H37.** A realm is made with the Hand, the chiefs' own agent, and the Hand
is always there: it cannot be ended or removed. A new realm has the Hand, an
empty city and no hamlet.

- The Hand has a profile of its own, never added to a hamlet or listed in
  the city,
  whose character starts from the app's default for it, and a lodge. A chief
  edits its character as any profile's.
- Every chief talks to the Hand first. It has a road to each lead and to no
  other agent.
- It can do in any hamlet what a parent can (H41), and found hamlets (H6), give
  a hamlet's office to another agent (H18), remove a hamlet (H19), read a
  hamlet's record, and read and change profiles (H38, H39).
- Its changes to a profile's `main` and its removal of a hamlet are reserved:
  they wait for a chief (H11). It asks for a chief's word before it touches
  what a chief owns.
- Its spend and that of the workers it starts are charged to the realm's
  budget, not a hamlet's (H8).

## The city

**H38.** The city holds the realm's profiles. A profile is a git repository
that the server alone writes, holding what the profile knows with its whole
history:

```text
CHARACTER.md              the character, at most 4000 characters
memory/                   files at the root are in every prompt
memory/<topic>/MEMORY.md  a folder's index; its files are read when needed
skills/<name>/SKILL.md    one folder per skill, with its files
```

- Every memory file and `MEMORY.md` opens with a front matter `description:`
  line, which the prompt shows for folders.
- `main` is what the profile knows. An agent's branch is `agents/<agent id>`.
  A version is a tag. A commit names who made it: a chief, an agent, the
  Hand, a dream or the realm.
- A profile belongs to the chief who made it. One an agent or the Hand makes
  belongs to the chief whose task the turn works on, else the chief who
  founded that hamlet, else the realm's first chief. Only its chief edits it,
  tags, reverts, forks, purges, archives, restores, erases or adds agents of
  it.
- A profile is its id, addressed as `profile#<id>`; its name is a label of
  one line, 1 to 40 characters with no colon, that may repeat and that a
  chief changes with `/name-profile`, which the record keeps.
- A chief makes one (`/profile new <name>[: <character>]`), or the Hand or a
  parent does (`create_profile`). It starts with one commit holding its
  character and an empty `memory/`. One a chief or the Hand makes has a
  house in the city at once; one a parent makes may wait at the harbor
  (H45).
- A blank agent is an agent of a hamlet that is not of a profile of the
  city: `/add-agent blank`, `add_agent` with no profile, or a hamlet founded
  with no profile named. It is a profile of its own all the same, made for
  that agent alone, from the realm's default character for one an agent
  makes, and named Lead, or Agent and its number, unless a name is given. It
  has no house, never waits at the harbor, is not listed with the city's
  profiles and no other agent is added of it.
- The server sets a new house down at the first free spot on a spiral out
  from the city's middle, clear of the middle and of every other house, its
  door toward the middle. A chief can set it down elsewhere
  (`/move-house`), on a spot just as clear.
- `main` changes by a chief's edit of the character or a memory file, a dream
  (H43), or a revert (H39). Never by an agent's tool.
- Archiving a profile ends its living agents first, each with a final commit
  of its lodge; it is refused while one of its agents
  holds a hamlet's office. Restoring brings back the
  agents its archiving ended in hamlets that still live. Erasing deletes its
  repository, its archived agents and their lodges, and cannot be undone.

**H45.** A profile a parent makes is a newcomer, which the realm's
`newcomers` setting lets into the city:

- `open`: it moves in at once.
- `chief` [the default]: it waits at the city's harbor, and a petition asks
  a chief to welcome it, or to leave it there for later.
- `hand`: the Hand judges it in one call with no tools, given the city's
  profiles and the newcomer's character, and lets it in or turns it away
  with a reason the record keeps.

A newcomer's agents work in their hamlet all the same. A chief can let any
profile at the harbor in (`/welcome`), and send one back to it
(`/turn-away`), which undoes either verdict of the Hand.

**H39.** A chief can name, hold to, undo, copy and cut out parts of a
profile's history:

- **Tag.** `/tag <profile#id>[@<commit>] <version> [note]` names `main`, or a
  commit, as a version.
- **Pin.** Adding an agent of `profile#id@version` starts the agent's branch
  at that version and records the pin; a pinned agent takes in no later
  learning. `/pin <address> <version|main>` changes the pin, merging the new
  target into the agent's branch and keeping the agent's side where both
  changed.
- **Revert.** `/revert <profile#id> <commit|version>` makes a commit on `main`
  that undoes that one. A path changed again since keeps its newer lines and
  is named as a clash.
- **History.** A chief can read a profile's commits and the diff between any
  two.
- **Fork.** `/fork <profile#id>[@<version>] <new name>` makes a new profile
  whose first commit is that version's files.
- **Purge.** `/purge <profile#id> <path>` rewrites every commit of every branch
  without that path and replaces the repository, so nothing of the path
  remains.

## Hamlets and their agents

**H6.** A chief, or the Hand, can found a hamlet. A hamlet has a lead, an
agent made when it is founded, who answers to the Hand: the chief names an
existing profile of its own, one at a version, or none for a blank agent
(H38). A hamlet
answers to the chief who founded it, or, when the Hand founds it, to the
chief whose task the Hand works on (C7). A hamlet has a tier, its most agents:
50, 100, 200 or 500, which a chief raises (H40).

**H36.** A hamlet's ID is a UUID made when it is founded, never reused, even
after the hamlet is erased; commands and tools name a hamlet by it. A hamlet
also always has a name, given when it is founded: a label that may repeat and
never finds the hamlet, and the only thing the window shows it by. Its spend
and records belong only to it.

**H27.** The lead is the agent at the root of a hamlet's tree, and the address
`lead` reaches whoever holds the office. The lead is the parent of the whole
hamlet (H41). It cannot be ended, merged away or archived with its profile
until the office is given to another agent.

**H18.** A chief, or the Hand, can give the lead's office to another agent of
the hamlet. The new lead takes the root; the old lead, and the agents under
the old lead, go under it.

**H40.** An agent of a hamlet is a profile working in it, for a purpose, with
a branch of its profile's repository and a lodge of its own (C8).

- **Adding.** A chief adds an agent of its own profile to a hamlet it is
  chief of. The Hand adds one of any chief's profile, and a parent of any
  profile of its hamlet's chiefs. A parent says what the agent is for; a
  chief may leave the purpose out, and the agent takes the first line of the
  first message it is given as its purpose. A hamlet holds at most its tier
  of living agents, the whole tree counted, workers not.
- **Its branch** `agents/<agent id>` starts at `main`, or at its pin.
- **Its lodge** is a checkout of its branch, without `CHARACTER.md`, plus an
  `inbox/` that is never committed. Its tools write there.
- **After each turn** that changed the lodge, the server commits it to the
  branch. What the agent learned is the difference between its branch and
  `main`; a chief can read it.
- **Taking in `main`.** When an unpinned agent is idle and `main` has moved
  since its branch last took it in, the server merges `main` into the branch,
  keeping the agent's side where both changed, and writes the merged files into
  the lodge. A turn never sees `main` change under it.
- **Ending** an agent stops its turns, lets its waits lapse, takes back its
  grants (H28), makes a final commit of its lodge and archives it. Its lodge
  and branch stay until erased. The agents under it go to its parent.
- **Restoring** it brings it back under the same id and number, when its
  profile lives, its hamlet lives and holds fewer than its tier. Its parent is
  its old parent when that lives, else the lead.
- **Erasing** an ended agent deletes its lodge and branch; the record keeps its
  number.

**H41.** The agents of a hamlet form a tree under its lead, as deep as its
parents make it. An agent with agents under it is a parent.

- A parent has the lead's powers over the agents under it, and only those:
  to add agents under itself or under an agent of its tree, make profiles,
  end agents, merge them, choose their models, turn their tools off, give
  them allowances, and lay and take up roads among them. Every agent of a
  hamlet has these tools (H35), so any of them can become a parent.
- The Hand is above every lead, and a chief overrules anything (H23).
- An agent has a road to its parent and to each agent under it while they
  live (H3).
- A chief's instruction to an agent is reviewed by its parent (H10).

**H42.** A worker is an agent started for one task, a child of whoever started
it, which ends when the task is answered.

- Any agent but a worker starts one (`start_worker`), or a chief does
  (`/worker`). The starter says what it is to do and which of its own built-in
  tools it may use.
- It has one conversation, with the task as its first input. Its last answer
  goes back to its starter: as the tool's answer when the starter waits, else
  as a letter. A chief's worker answers in its own conversation, which the
  chief watches.
- It has the built-in tools its starter gave it and no society tools, so it
  starts no workers and sends no letters.
- It works in the hamlet's folder as its starter does, reads its starter's
  lodge and the library, and writes a scratch folder that is removed when it
  ends.
- Its spend is charged to its starter's task, hamlet and allowance.
- It is not counted among a hamlet's agents, and has no branch, lodge,
  memory or roads.

**H17.** Every decision of a chief, the Hand or a parent is kept in the hamlet's
record with its reason, when one was given, and a chief can read the record in
words. Each agent
is told the decisions of the last [7] days that touched its hamlet, at most
twelve, and a parent is told a chief's recent instructions to the agents under
it.

**H5.** A parent, or a chief, can end an agent or merge one into another of the
same hamlet at once, subject to the chief's limits (H22) and tasks (H7). A
reason may be given, and the record keeps it; none is needed, here or for any
other decision.

In a merge the kept agent takes the absorbed agent's laid roads, its agents
and its inbox, and takes in what the absorbed agent learned: when both are of one
profile, the absorbed branch is merged into the kept one, keeping the kept
side where both changed; when they are of different profiles, what the
absorbed agent learned since its branch took in `main` is committed to the kept
branch, a clashing file kept aside as `<name>.from-<absorbed>` and a clashing
skill as `<skill>-from-<absorbed>`. The absorbed agent is archived as merged.
No model rewrites memory at a merge; a dream tidies it later (H43).

**H7.** An agent working on a chief's task cannot be ended or merged by a parent
until it has answered, nor for [0] minutes after.

**H20.** A chief can bring back what left: an ended agent, a merged agent
split out of the agent that absorbed it, a removed hamlet (H19) or an archived
profile (H38). A split brings the absorbed agent back and undoes the merge's
commit on the kept branch; what the kept agent learned since stays. Archived
lodges stay in place until erasure; a chief can delete what is archived of an
agent, a hamlet or a profile for good, after which it cannot come back.

A removed hamlet comes back as it was only while the profiles of its agents
live. Otherwise it is refused, naming them.

**H28.** A message to an agent that has left is refused, with how it left
and why. What it waited for lapses and what it was granted is taken back; if
it comes back, it asks again.

**H29.** An agent runs the model chosen for it, by a chief or, for an agent of a
hamlet, by a parent, else its hamlet's own model, else the realm's default. Every model of
every configured provider may be chosen. A parent may not change a chief's
choice. The choice for an agent of a hamlet is kept in the record with its reason. A worker runs
its starter's model unless it was given one.

**H29a.** How hard an agent's model thinks (off, minimal, low, medium, high,
xhigh or max) is the agent's own effort when set, else its starter's for a
worker, else the realm's `effort` setting, else the model's own default. An
effort the model cannot be asked for becomes the nearest it can. A chief may
also turn on an agent's fast tier, which asks the model's provider for its
faster, dearer tier where it has one; it is off unless turned on.

## Roads and letters

**H3.** Agents reach each other along roads, of four kinds:

- **Tree roads**: an agent to its parent and to each agent under it, and the
  Hand to each lead.
- **Leads' roads**: between each two leads.
- **Laid roads**: between two agents of one hamlet, laid by a parent over both,
  or by a chief. Each costs the hamlet [$0.01], up to the most the limits
  allow [24] in a hamlet, and is charged to the task that laid it; a chief's
  road has no task. What a road cost is not given back.
- **Agreed roads**: between agents of different hamlets, once both leads
  agree (`agree_road`), and only while the realm allows roads between
  hamlets [yes].

A road nobody used for [7] days is shown grown over. A chief or a parent can take
up a laid or agreed road. Roads of an agent that was merged go to the agent
that absorbed it, and are not charged again.

**H30.** An agent sends messages only along its roads, in two ways:

- **A letter** (`tell`): sent without waiting. The other agent works on it
  in its own conversation with the sender, and the reply comes back later as a
  new message.
- **A question** (`ask`): the sender waits while the other agent's turn runs
  in its own lodge, and gets the reply as the tool's answer. That turn's tool
  calls pass the same gate as the sender's.

Each letter, question and reply names its sender (C11, C30). A question
that would leave a conversation waiting on its own answer, directly or
through other questions, is refused.

A letter or a question can name `paths`: files or folders in the sender's
lodge or its hamlet's folder, which the server copies into the receiver's lodge
under `inbox/from-<sender>/`, links not followed, as far as the sender's
sandbox lets it read. The message says where each landed. An image among them
is also given to the receiver's model when that model takes images. One send
holds at most [32 MiB and 1000 files], images among them; a larger one waits for
a chief, who is told what is sent and how large, and who allows it or refuses
it, and the sender is told of a refusal. No send holds more than [256 MiB] in
one file, [1 GiB] or [10000 files] in all. A remote workspace gives at most 4
MiB of a file at once, so a sender working there sends no larger file.

An agent can ask its parent for a road to another agent of its hamlet
(`request_road`).

**H31.** While another agent waits on an agent's reply, that agent cannot
send letters: they would reach the waiting agent after it stopped
listening.

**H12.** Every message a chief sends starts a task, and the task follows
every message and turn it causes, in every agent it reaches. A retry, a
reflection, a librarian's call, a parent's review, a dream and a worker start a
task of their own. A task has no caps of its own: what it spends counts
against its hamlet's budget (H8).

## Budgets

**H8.** A hamlet has a budget for a period of a [day] or a month, on cost
[$50], or none, and on model time; tokens are counted but not capped. The realm has one
for the Hand and for the workers started outside a hamlet. Every road and
model call is charged, including reflections, summaries and recorded failed
or aborted attempts. A response with no reported tokens is estimated unless
it failed or was aborted; an attempt with no reported usage costs nothing.
The provider's billed cost is used when available, else the model's catalogue
rates or the chief's assigned price. An unpriced model is charged $0.10 per
million tokens on this machine, $1 per million elsewhere. Subscription models
are charged at catalogue rates even when the chief pays no per-token bill.
Spending follows the server's local day.

Before every model call the budget is checked, and a turn that has run out
stops, saying what ran out and who can change it. An agent reads what it may
spend with `read_budget`: its hamlet's or the realm's spend against the
budget, its own and its allowance. A chief can start the period again from
nothing spent. No summary starts once the budget has run out.

**H44.** A parent or a chief can give an agent an allowance for the period.
Before a model call, what the agent and everything under it have spent must be within
its own allowance and that of every parent above it. A worker counts as its
starter.

**H46.** A chief of its hamlet, the Hand or a parent above it can give an agent
a nickname of at most 24 letters, kept in the record. The world shows an
agent by its nickname, else by its profile's name; the window keeps its
address. A chief can nickname a worker too, which is shown by its address
without one. Everyone but the Hand can be renamed: a profile with
`/name-profile`, a hamlet with `/name-hamlet`. In the window, a double
click on the name at the top of its page in the Hub renames it.

**H16.** An agent that does nothing costs nothing.

## Memory and dreams

**H43.** What an agent learns stays on its branch until a dream brings it home.
A dream is a conversation of its own, with a task and a cost, kept as a
record of what ran and the commits it made. Dreams are off until a chief turns
them on, and run one at a time in the hours the chief sets [1 to 6], each
costing at most a cap [$0.20].

- **An agent's dream** runs when the agent is idle and its branch holds
  something `main` lacks, at most every [6] hours. It is given the agent's
  lodge and a checkout of `main`, both writable, and decides what goes to the
  profile and what stays with the agent. The server commits the profile's
  side to `main`, the agent's side to its branch, and takes the new `main`
  into the branch.
- **A profile's dream** tidies `main` itself, as one commit, at most once a
  day, for a profile with living agents.
- Ending an agent runs one more dream for it within a day, when dreams are on.
- A chief can have any dream run now (`/dream`).

## Between hamlets

**H14.** Between hamlets:

- There is a road between each two leads from the start.
- Agents of different hamlets have a road once both leads agree to it
  (`agree_road`), and only while the realm allows such roads [yes].
- A profile's name is a label and need not be unique (C10). An agent is
  reached by an address: `hand`; `lead`, which is the local office, and
  `lead@<hamlet id>`; `agent#<id>`; and `worker#<id>`. A profile is
  `profile#<id>`. No name finds an agent or a profile. Wherever an agent is
  shown, its address comes first and its name beside it, as
  `agent#3 (Mira)`.
- A message between hamlets is paid for by the sender's hamlet.

The chiefs of each hamlet concerned are told, in their conversation with its
lead, when a hamlet is founded or removed, its lead changes, or a road
between hamlets is agreed.

**H19.** A chief, or the Hand on a chief's order, can remove a hamlet. Its
agents end, each with a final commit of its lodge, and their lodges and
branches stay, archived with its record, with its tree kept so that restoring
the hamlet (H20) brings them back. Erasing it deletes the lodges and the
agents' branches. No hamlet is special: any can be removed.

**H32.** A chief can give a hamlet another name to be shown by, while its ID
stays for commands, and set the hamlet down elsewhere on the hamlet island.
It can also set a profile's house down elsewhere in the city.

## The library

**H33.** The library holds the skills every agent of the realm shares, as a
repository whose `main` is checked out read-only for agents, starting with the
starter skills. An agent offers one of its own skills with a reason
(`submit_skill`). The librarian judges it in one model call, with no tools.
`ACCEPT` as the first word, without regard to case, shelves the skill;
anything else refuses it. An accepted skill is committed to the library whole.
Its unchanged lodge copy is removed when possible. Later changes stay in the
lodge. An agent can ask the librarian which skill does what it needs
(`ask_librarian`), also in one call without tools. Each call is kept and
charged to the asking agent and its task. The library keeps a record of what
was shelved, refused and taken out, and a chief can take a skill out
(`/unshelve`).

## What waits for a chief

**H11.** A tool call waits for a chief when:

- it is a command that needs the network and its agent has none;
- nothing confines it (no sandbox), and it looks destructive, or its agent may
  not run unconfined;
- it is something the chief's limits reserve (H22), or one of the Hand's
  acts on what chiefs own (H37);
- its hamlet is in manual mode and it is an edit of the folder or a
  command, not an edit of the agent's own lodge (C21).

A chief answers: allow it once; allow edits or commands for the rest of the
agent's conversation; allow it always, which for the network or running
unconfined is a lasting permission kept by the agent until taken back or the
agent ends; or refuse. Every refused approval counts toward the turn's cap,
whatever its danger. After [3] refusals in one turn, further requests for
approval are refused without asking.

**H10.** An agent can talk back:

- An agent a chief instructs may object once to the instruction (`object`).
  Its whole task is held, including the work of other agents it reached,
  until the chief says go ahead or takes the instruction back. Taking it
  back stops the task's work; after a go ahead it may not refuse.
- When a chief instructs an agent under a parent directly, its parent is told and [for a model
  served from another machine] reviews the instruction in one model call
  beside the work, charged to its task, in which it may object by answering
  `OBJECT: <reason>`, or answer `pass`.
- While a chief oversees it, an agent can ask the chief a question it cannot
  go on without (`ask_chief`), at most [3] in one turn, and waits for the
  answer.

**H34.** What waited for a chief, and how it ended, is kept. A wait lapses
when a chief stops its turn, when the conversation is taken back past it,
or when its agent leaves. What was waiting when the server stopped waits
again when it opens; none of it was carried out.

## The tools of the society

**H35.** The society's own tools are offered by role. A one-off call has only
the tools in its own row:

| Who | Tools |
|---|---|
| Every agent but a worker | `tell`, `ask` (H30), `start_worker` (H42), `submit_skill`, `ask_librarian` (H33), `ask_chief` (H10), `read_budget` (H8) |
| Every agent of a hamlet | `request_road` |
| A parent, and the Hand | `create_profile`, `add_agent`, `end_agent`, `merge_agents`, `set_model`, `set_tools`, `set_allowance`, `nickname`, `lay_road`, `take_up_road` |
| A lead, when other hamlets live | `agree_road` |
| The Hand | `found_hamlet`, `set_lead`, `remove_hamlet`, `read_record`, `read_profile`, `edit_profile`, `tag`, `revert` |
| Every agent but a worker, in a chief conversation | `object` (H10) |
| A worker | None of the society's |
| A dream | None of the society's |
| A librarian's call or a review | No tools |

The offered tools follow the agent's current role when the tree or the realm
changes. A tool called by the wrong role refuses in words. Offering `object`
does not grant another objection, and `ask_chief` refuses when no chief
oversees the task (H10). A parent's review objects in its answer, not through a
tool.

## Retired

These requirements are gone with the things they were about, and their IDs
are not used again: **H13** (votes), and with it the rules of ballots, the
pass fraction and the hearing of a lead. What H18, H19, H14 and H6 said
about votes, about hamlets asking for another's removal, and about moving an
agent to another hamlet (`agree_move`) is gone from them.
