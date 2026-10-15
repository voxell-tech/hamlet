# The hamlet: the society of agents

Chiefs, mayors and agents, the roads and letters between them, the tasks and
budgets that bound their work, votes, the realm of several hamlets, the
library, and what waits for a chief. One agent on its own is in
[`core.md`](core.md).

## Chiefs

**H23. Rule one.** A chief can overrule anything: stop any agent or every
agent, reverse a vote that passed or stop an open one, remove and restore any
agent or hamlet, take back any permission, and undo the latest thing that
can be undone.

**H22.** A chief's limits bound a hamlet, and only a human changes them;
every check reads them as they are now. They can reserve to a chief what the
mayor may otherwise do alone: creating agents, building roads, removing
agents and merging them. A reserved act waits for a chief (H11).

**H24.** Only a line a human types can be a command. A letter between agents
is words, whatever it begins with.

**H25.** A chief can give an agent a file or folder, which lands in the
agent's inbox (at most 32 MB at once), or a skill: a starter, or one from
another agent's house.

**H26.** A chief has these commands:

| Command | Does |
|---|---|
| `/agent <name>`, `/hamlet <name>` | Talk to that agent, or to that hamlet's mayor |
| `/to <agent> <instruction>` | Instruct an agent directly |
| `/new` | Move at once to a new conversation while the old one finishes its queue (C24) |
| `/resume [n]` | List earlier conversations, or go back to one by its stable number (C24) |
| `/retry`, `/reflect` | Carry on a failed or stopped turn as a visible new input, reflect now (C26, C5) |
| `/stop [agent\|all]` | Stop an agent's work, or everyone's (H23) |
| `/queue`, `/allow`, `/always`, `/deny`, `/go`, `/withdraw`, `/reply`, `/dismiss` | Answer what waits (H11, H10) |
| `/undo` | Reverse the latest thing that can be reversed |
| `/grants`, `/revoke <agent> <network\|unconfined>` | The lasting permissions, and taking one back |
| `/remove`, `/merge`, `/restore`, `/erase` | Agents leaving and coming back (H5, H20) |
| `/road`, `/unroad` | Build or take up a road between two agents (H3) |
| `/give <agent> <skill> [from <agent>]` | Give a skill (H25) |
| `/folder`, `/mode <manual\|auto>` | The hamlet's folder and its mode (C20, C21) |
| `/budget`, `/reset-budget` | The hamlet's spend, and starting its period again (H8) |
| `/record` | Who was created, removed, merged and restored (H17) |
| `/mayor <agent>`, `/vote <agent>`, `/votes`, `/overturn <vote>` | The mayor's office and votes (H18, H13) |
| `/hamlets`, `/found`, `/remove-hamlet`, `/restore-hamlet`, `/erase-hamlet`, `/name-hamlet`, `/move-hamlet` | The realm (H6, H19, H20, H32) |
| `/unshelve <skill>` | Take a skill out of the library (H33) |

## The mayor and the agents

**H27.** The mayor is the agent that holds a hamlet's office, and the
address `mayor` reaches whoever holds it. The mayor creates agents, each with
a purpose, a reason, the roads to lay at once, a model and a character,
until the hamlet holds the most the chief allows [12].

**H18.** A chief, or a vote of the hamlet's agents (H13), can give the
mayor's office to another agent of the hamlet. The agent who held it stays,
as an ordinary agent.

**H17.** Every decision of a mayor or a chief is kept in the hamlet's record
with its reason, and a chief can read the record in words. Each agent is told
the decisions of the last [7] days, at most six with each reason cut to 160
characters, and those that touched it, such as its road requests the mayor
refused.

**H5.** The mayor can remove an agent, or merge one into another, at once.
A removed agent's house is archived, never deleted. In a merge the agent
kept takes the new purpose and the contents and roads of both; their notes
are rewritten into one by a model, at most [6000] characters, or joined end
to end, and files of the same name that differ are kept aside. A departure
needs a reason.

**H7.** An agent working on a chief's task cannot be removed or merged by
the mayor until it has answered, nor for [0] minutes after.

**H20.** A chief can bring back what left: a removed agent, a merged one
split out of the agent that absorbed it (under a new name when its own was
taken since), or a removed hamlet (H19). A chief can delete what is archived
of an agent or a hamlet for good, after which it cannot come back.

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
mayor's, and take them up; what a road cost is not given back.

**H30.** An agent sends messages only along its roads, in two ways:

- **A letter** (`tell_agent`): sent without waiting. The other agent works
  on it in its own conversation with the sender, and the reply comes back
  later as a new message.
- **A question** (`ask_agent`): the sender waits while the other agent's turn
  runs in its own house, and gets the reply as the tool's answer. That turn's
  tool calls pass the same gate as the sender's.

An agent other than the mayor can ask the mayor for a road to another agent
(`request_road`).

**H31.** While another agent waits on an agent's reply, that agent cannot
send letters: they would reach the waiting agent after it stopped
listening.

**H12.** Every message a chief sends starts a task, and the task follows
every message and turn it causes, in every agent it reaches. A task may send
at most [12] messages between agents and spend at most [$1]. A reply is
delivered whatever the count, so a task can end. A ballot is a task of its
own.

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
Summaries' model time is not yet charged (C31).

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
- The mayor is heard once, before the ballots.
- Each voter votes in its own house, with no tool but its ballot, in a few
  model steps and [5] minutes. A ballot carries a reason and evidence: words
  copied from the voter's own notes or history, checked against them.
- Ballots cast so far are [hidden] from each voter. A yes can be made to
  count only when its evidence was found [no].
- A vote passes with [2/3] of the voters; a voter with no ballot has not
  said yes.
- A vote still open after [60] minutes lapses. An agent whose vote failed
  waits [60] minutes before it calls another of the same kind.
- While a vote is open, nothing else is decided in the hamlet.

## The realm

**H6.** A chief, or a vote of a hamlet's agents, can found a new hamlet under
a new mayor. A hamlet founded by a vote belongs to the chiefs of the hamlet
that voted (C7). A home holds at most [6] hamlets.

**H14.** Between hamlets:

- There is a road between each two mayors from the start.
- Agents of different hamlets have a road once both mayors agree to it
  (`agree_road`), and only while the chief allows such roads [yes].
- An agent moves to another hamlet with its house once both mayors agree
  (`agree_move`), keeping only the roads both named.
- A message between hamlets is paid for by the sender's hamlet. A mayor is
  reached from outside its hamlet as `mayor@<hamlet>`.

**H19.** A mayor can ask for its own hamlet to be removed, and [3] mayors of
other hamlets together can ask for another's. Nothing is removed until a
chief confirms, and a chief's word alone is enough. Every house of a removed
hamlet is archived with its record.

A chief can also bring back a removed hamlet as it was, or delete it for
good (H20).

**H32.** A chief can give a hamlet a name to be shown by, while its own name
stays for commands, and set its town down elsewhere in the world.

## The library

**H33.** The library holds the skills every agent of the home shares,
starting with the starter skills. An agent offers one of its own skills with
a reason (`submit_skill`). The librarian, a model call with a house of its
own for notes, judges it, and anything but a plain yes refuses it. An agent
can ask the librarian which skill does what it needs (`ask_librarian`). The
library keeps a record of what was shelved, refused and taken out, and a
chief can take a skill out.

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
  The work is held until the chief says go ahead or takes the instruction
  back, and after a go ahead it may not refuse.
- When a chief instructs one of the mayor's agents directly, the mayor is
  told, and [for a model served from another machine] reviews the
  instruction in one model call beside the work, charged to its task, in
  which it may object.
- While a chief oversees it, an agent can ask the chief a question it cannot
  go on without (`ask_chief`), at most [3] in one turn, and waits for the
  answer.

**H34.** What waited for a chief, and how it ended, is kept. A wait lapses
when a chief stops its turn, when the conversation is taken back past it,
or when its agent leaves. What was waiting when the server stopped waits
again when it opens; none of it was carried out.

## The tools of the society

**H35.** The hamlet's own tools are offered by role:

| Who | Tools |
|---|---|
| The mayor | `create_agent`, `build_road`, `set_model`, `remove_agent`, `merge_agents` |
| The mayor, when the home has other hamlets | `agree_road`, `agree_move`, `ask_removal` |
| Every agent | `tell_agent`, `ask_agent` (H30), `call_vote` (H13), `submit_skill`, `ask_librarian` (H33) |
| An agent other than the mayor | `request_road` |
| A voter, during its ballot | `cast_ballot` alone |
| An agent a chief instructed, until it objects or works on | `object`; the mayor has its own form, about its agents' instructions |
| An agent a chief oversees | `ask_chief` |
