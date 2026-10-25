# The hamlet: the society of agents

Chiefs, mayors and agents, the roads and letters between them, the tasks and
budgets that bound their work, votes, the realm of several hamlets, the
library, and what waits for a chief. One agent on its own is in
[`core.md`](core.md).

## Chiefs

**H23. Rule one.** A chief can overrule anything in its hamlets: stop any
or all of their agents, reverse a vote that passed or stop an open one,
remove and restore their agents or the hamlets themselves, take back any
permission, and undo the latest thing that can be undone.

**H22.** A chief's limits bound a hamlet, and only a human changes them;
every check reads them as they are now. They can reserve to a chief what the
mayor may otherwise do alone: creating agents, building roads, removing
agents and merging them. A reserved act waits for a chief (H11). Vote limits
belong to each hamlet. The realm's hamlet cap, roads between hamlets and
number of mayors needed to ask for removal are governed by the first
hamlet, `home`, and shown only in its limits.

**H24.** Only a line a human types can be a command. A letter between agents
is words, whatever it begins with.

**H25.** A chief can give an agent a file or folder, which lands in the
agent's inbox (at most 32 MB at once), or a skill: a starter, or one from
another agent's house.

**H37.** A chief can buy agents, hamlets and tools at the Agent Bazaar's
server and bring them home: an agent moves into a hamlet the chief
chooses with its character, notes and skills; a hamlet is founded as one
from a template is (H6); tools join those added by hand (C17). A newer
version of one bought is brought up where it lives, and an agent's notes,
its own memory, are kept. A chief can pack an agent, a hamlet or tools
added by hand to sell there, an agent's notes only when the chief says
so.

**H26.** A chief has these commands:

| Command | Does |
|---|---|
| `/agent <name>`, `/hamlet <name>` | Talk to that agent, or to that hamlet's mayor |
| `/to <agent> <instruction>` | Instruct an agent directly |
| `/new` | Move at once to a new conversation while the old one finishes its queue (C24) |
| `/resume [n]` | List earlier conversations, or go back to one by its stable number (C24) |
| `/retry`, `/reflect` | Carry on a failed or stopped turn as a visible new input, reflect now (C26, C5) |
| `/stop [agent\|all]` | Stop an agent's work, or all agents of the chief's hamlets (H23) |
| `/queue`, `/allow`, `/always`, `/deny`, `/go`, `/withdraw`, `/reply`, `/dismiss` | Answer what waits (H11, H10) |
| `/undo` | Reverse the latest thing that can be reversed |
| `/grants`, `/revoke <agent> <network\|unconfined>` | The lasting permissions, and taking one back |
| `/remove <agent> [reason]` | Remove an agent and archive its house (H5) |
| `/merge <absorbed> into <kept> [reason]` | Merge two agents, or finish a pending merge named by its archive ID (H5) |
| `/restore <agent> [as <new-name>] [reason]`, `/erase <agent>` | Restore, split or permanently erase an archived agent (H20); an archive ID also names it |
| `/road <agent> <agent> [reason]`, `/unroad <agent> <agent> [reason]` | Build or take up a road between two agents (H3) |
| `/give <agent> <skill> [from <agent>]` | Give a skill (H25) |
| `/folder <folder\|none>`, `/mode <manual\|auto>` | The hamlet's folder and its mode (C20, C21) |
| `/budget`, `/reset-budget` | The hamlet's spend, and starting its period again (H8) |
| `/record` | Who was created, removed, merged and restored (H17) |
| `/mayor <agent> [reason]`, `/vote <agent> [reason]`, `/votes`, `/overturn <vote> [reason]` | The mayor's office and votes (H18, H13) |
| `/hamlets`, `/found <hamlet> <mayor> <purpose>` | List hamlets, or found one under a new mayor (H6) |
| `/remove-hamlet <hamlet> [reason]`, `/restore-hamlet <hamlet>`, `/erase-hamlet <hamlet>` | Close, restore or permanently erase a hamlet (H19, H20) |
| `/name-hamlet <hamlet> <new-name>`, `/move-hamlet <hamlet> <x> <z>` | Give a hamlet a display name or move its town (H32) |
| `/unshelve <skill> [reason]` | Take a skill out of the library (H33) |
| `/chief` | Show the commands about hamlets and votes |

The server advertises its supported command names. The window offers only
the deeds it advertises.

## The mayor and the agents

**H27.** The mayor is the agent that holds a hamlet's office, and the
address `mayor` reaches whoever holds it. The mayor creates agents, each with
a purpose, a reason, the roads to lay at once, a model and a character,
until the hamlet holds the most the chief allows [12], not counting the mayor.

**H18.** A chief, or a vote of the hamlet's agents (H13), can give the
mayor's office to another agent of the hamlet. The agent who held it stays,
as an ordinary agent.

**H17.** Every decision of a mayor or a chief is kept in the hamlet's record
with its reason, and a chief can read the record in words. Each agent is told
the decisions of the last [7] days, at most six with each reason cut to 160
characters, and those that touched it, such as its road requests the mayor
refused.

**H5.** The mayor can remove an agent, or merge one into another, at once,
subject to the chief's limits (H22) and tasks (H7). A departure needs a
reason. The record archives the departed agent; its house stays where it
is, including after a merge, until a chief erases it.

In a merge the kept agent takes copies of both houses' contents and their
roads. When the mayor merges, the kept agent takes the purpose the mayor
gives, and their notes are rewritten into one by a model, at most [6000]
characters, or else joined end to end. A chief's merge keeps the kept
agent's purpose and joins the notes. Files of the same name that differ
are kept aside. The archive keeps the merge's manifest, the notes before
and after, and the inherited roads for splitting (H20).
Both agents are stopped while their houses are being merged or split.
A merge whose absorbed house exceeds 512 MiB or 20000 file and folder
entries is refused, with words saying to remove the agent instead.

A merge or split that cannot finish stays recorded as pending and tells
the chief how to inspect it and retry safely: a pending merge with
`/merge <archive> into <kept>`, a pending split with `/restore <archive>`.
A conflict never overwrites changed files or notes. Pending work is tried
again when the home opens and does not stop it opening. Its archived
house cannot be erased until it finishes.

**H7.** An agent working on a chief's task cannot be removed or merged by
the mayor until it has answered, nor for [0] minutes after.

**H20.** A chief can bring back what left: a removed agent, a merged one
split out of the agent that absorbed it (under a new name when its own was
taken anywhere in the realm since), or a removed hamlet (H19). Restoration
keeps the agent's number. A split uses the stored manifest, removes only
unchanged copies brought by the merge, and preserves what the kept agent
learned since. The kept agent must be living, and later merges into it
must be split first. Archived houses stay in place until erasure; a chief
can delete what is archived of an agent or a hamlet for good, after which
it cannot come back.

A removed hamlet comes back as it was only while none of its agents' names
belongs to a living agent elsewhere and the realm is under its hamlet cap
(H6). Otherwise it is refused, saying which name or limit stands in the
way.

**H28.** A message to an agent that has left is refused, with how it left
and why. What it waited for lapses and what it was granted is taken back; if
it comes back, it asks again.

**H29.** An agent runs the chief's choice of model for it, else the mayor's
choice from the models the chief allows the hamlet, else the hamlet's own
model, else the home's default. Each choice is kept in the record with its
reason.

## Roads and letters

**H3.** Every agent has a road to the mayor's office for as long as it lives
in the hamlet. The mayor builds roads between agents, each costing the hamlet
[$0.01], up to the most the chief allows [24]. A road nobody used for [7]
days is shown grown over. A chief can build roads, which count like the
mayor's, and take them up; what a road cost is not given back. A built road
is charged to the task that built it; a chief's road has no task. Restored
or inherited roads are not charged again.

**H30.** An agent sends messages only along its roads, in two ways:

- **A letter** (`tell_agent`): sent without waiting. The other agent works
  on it in its own conversation with the sender, and the reply comes back
  later as a new message.
- **A question** (`ask_agent`): the sender waits while the other agent's turn
  runs in its own house, and gets the reply as the tool's answer. That turn's
  tool calls pass the same gate as the sender's.

Each letter, question and reply names its sender (C11, C30). A question
that would leave a conversation waiting on its own answer, directly or
through other questions, is refused.

An agent other than the mayor can ask the mayor for a road to another agent
(`request_road`).

**H31.** While another agent waits on an agent's reply, that agent cannot
send letters: they would reach the waiting agent after it stopped
listening.

**H12.** Every message a chief sends starts a task, and the task follows
every message and turn it causes, in every agent it reaches. A task may send
at most [12] messages between agents and spend at most [$1]. A reply is
delivered whatever the count, so a task can end. A vote is a task of its
own; its hearing and every ballot share that task's caps and spend.

## Budgets

**H8.** A hamlet has a budget for a period of a [day] or a month, on cost
[$5] and on model time; tokens are counted but not capped. Every road and
model call is charged, including reflections, summaries and recorded
failed or aborted attempts. A response with no reported tokens is estimated
unless it failed or was aborted; an attempt with no reported usage costs
nothing. The provider's billed cost is used when available, else the model's
catalogue rates or the chief's assigned price. An unpriced model is charged
$0.10 per million tokens on this machine, $1 per million elsewhere.
Subscription models are charged at catalogue rates even when the chief
pays no per-token bill. Spending follows the server's local day.

Before every model call the hamlet's budget and the task's caps are checked,
and a turn that has run out stops, saying what ran out and who can change
it. A chief can start the period again from nothing spent, without resetting
a task's spend. No summary starts once the budget or the task's cap has run
out.

**H16.** An agent that does nothing costs nothing.

## Votes

**H13.** Any agent of a hamlet, or a chief, can call a vote to give the
mayor's office to another agent (H18) or to found a new hamlet (H6):

- The voters are the hamlet's agents besides the mayor, fixed when it is
  called, and there must be at least [3].
- A vote to replace the mayor hears it once before the ballots, unless the
  mayor called the vote itself.
- Each voter votes in its own house, with no tool but its ballot, in a few
  model steps and [5] minutes. A ballot carries a reason and evidence: words
  copied from the voter's own notes or history, checked against them.
- Ballots cast so far are [hidden] from each voter. Hidden ballots are
  asked for together; otherwise voters are asked one at a time and see
  earlier ballots. A yes can be made to count only when its evidence was
  found [no].
- A vote passes with [2/3] of the voters; a voter with no ballot has not
  said yes.
- A vote still open after [60] minutes lapses. An agent whose vote failed
  waits [60] minutes before it calls another of the same kind.
- While a vote is open, nothing else is decided in the hamlet.

These limits are the voting hamlet's own. The chief receives notices of
the vote, ballots and result in its conversation with that hamlet's mayor.

## The realm

**H6.** A chief, or a vote of a hamlet's agents, can found a new hamlet under
a new mayor. A hamlet founded by a vote belongs to the chiefs of the hamlet
that voted (C7). A home holds at most [6] hamlets. A chief can also found
one from a template, as the Agent Bazaar offers: its mayor and agents come
with their purposes, characters, notes and skills, and nothing is founded
unless all of them can be.

**H36.** A hamlet's ID is never reused, even after the hamlet is erased.
Its spend and records belong only to it.

**H14.** Between hamlets:

- There is a road between each two mayors from the start.
- Agents of different hamlets have a road once both mayors agree to it
  (`agree_road`), and only while the chief allows such roads [yes].
- An agent moves to another hamlet with its house once both mayors agree
  (`agree_move`), keeping only the roads both named. It takes the next
  number in the receiving hamlet. Its character, chief and peer
  conversations and instructions move with it in one recorded change;
  an unfinished house move is completed when the home opens again.
  Its waits lapse and its lasting permissions are taken back, so it asks
  again under its new hamlet's chiefs. A mayor never moves, and an agent
  that absorbed another in a merge not yet split cannot move until a chief
  splits or erases that archive (H20).
- Living agent names are unique across the realm (C10). An agent is
  reached by `name` or `name@<hamlet>`; `mayor` reaches the local office,
  and `mayor@<hamlet>` reaches another hamlet's office. A wait from the
  mayor of any hamlet but `home` names it as `mayor@<hamlet>`; `home`'s
  mayor is named `mayor`.
- A message between hamlets is paid for by the sender's current hamlet;
  after a move, its new hamlet pays for the conversations it started.

The chiefs of each hamlet concerned are told, in their conversation with
its mayor, when a hamlet is founded or removed, its mayor changes, a road
between hamlets is agreed, or an agent moves. A hamlet that answers to
whoever runs the server (C7) sends these notices to that human.

**H19.** A mayor can ask for its own hamlet to be removed, and [3] mayors of
other hamlets together can ask for another's. Nothing is removed until a
chief confirms, and a chief's word alone is enough. Removal requests
arrive as notices in the chief's conversation with the affected hamlet's
mayor. A ready request names `/remove-hamlet <hamlet>` to confirm it; it
is not an approval wait. Every house of a removed hamlet stays where it
is, archived with its record.

The first hamlet, `home`, cannot be removed. Every founder, `home`'s
included, is an ordinary agent once it no longer holds the office, and
can leave like any other agent. A chief can bring back a removed hamlet
under H20's conditions, or delete it for good.

**H32.** A chief can give a hamlet a name to be shown by, while its own name
stays for commands, and set its town down elsewhere in the world.

## The library

**H33.** The library holds the skills every agent of the home shares,
starting with the starter skills. An agent offers one of its own skills with
a reason (`submit_skill`). The librarian judges it in one model call,
with its own notes shown but no tools and no way to write those notes.
`ACCEPT` as the first word, without regard to case, shelves the skill;
anything else refuses it. An accepted skill appears on the shelf whole.
Its unchanged house copy is removed when possible; if it cannot be removed,
the agent is told. Later changes stay in the house. An agent can ask the
librarian which skill does what it needs (`ask_librarian`), also in one call
without tools. Each call is kept and charged to the asking agent and its
task. The library keeps a record of what was shelved, refused and taken out,
and a chief can take a skill out. A chief can shelve a skill too, as
brought from the Agent Bazaar, under a name the library does not have; the
librarian does not judge it.

## What waits for a chief

**H11.** A tool call waits for a chief when:

- it is a command that needs the network and its agent has none;
- nothing confines it (no sandbox), and it looks destructive, or its agent
  may not run unconfined;
- it is something the chief's limits reserve (H22);
- its hamlet is in manual mode and it is an edit of the folder or a
  command, not an edit of the agent's own house (C21).

A chief answers: allow it once; allow edits or commands for the rest of the
agent's conversation; allow it always, which for the network or running
unconfined is a lasting permission kept by the agent's number until taken
back; or refuse. Every refused approval counts toward the turn's cap,
whatever its danger. After [3] refusals in one turn, further requests for
approval are refused without asking.

**H10.** An agent can talk back:

- An agent a chief instructs may object once to the instruction (`object`).
  Its whole task is held, including the work of other agents it reached,
  until the chief says go ahead or takes the instruction back. Taking it
  back stops the task's work; after a go ahead it may not refuse.
- When a chief instructs one of the mayor's agents directly, the mayor is
  told, and [for a model served from another machine] reviews the
  instruction in one model call beside the work, charged to its task, in
  which it may object by answering `OBJECT: <reason>`, or answer `pass`.
- While a chief oversees it, an agent can ask the chief a question it cannot
  go on without (`ask_chief`), at most [3] in one turn, and waits for the
  answer.

**H34.** What waited for a chief, and how it ended, is kept. A wait lapses
when a chief stops its turn, when the conversation is taken back past it,
or when its agent leaves. What was waiting when the server stopped waits
again when it opens; none of it was carried out.

## The tools of the society

**H35.** The hamlet's own tools are offered by role. A one-off call has
only the tools in its own row:

| Who | Tools |
|---|---|
| The mayor | `create_agent`, `build_road`, `set_model`, `remove_agent`, `merge_agents` |
| The mayor, when the home has other hamlets | `agree_road`, `agree_move`, `ask_removal` |
| Every agent | `tell_agent`, `ask_agent` (H30), `call_vote` (H13), `submit_skill`, `ask_librarian` (H33) |
| An agent other than the mayor | `request_road` |
| A voter, during its ballot | `cast_ballot` alone |
| Every agent, in a chief conversation | `object` (H10) |
| Every agent, in a conversation other than a one-off call | `ask_chief` (H10) |
| A one-off call other than a ballot | No tools |

The offered tools follow the agent's current role when the office or the
realm changes. A tool called by the wrong role refuses in words. Offering
`object` does not grant another objection, and `ask_chief` refuses when no
chief oversees the task (H10). The mayor's review objects in its answer,
not through a tool.
