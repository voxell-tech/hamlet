# The core: the hamlet as entities

How the records on disk become Bevy entities, who writes them and who reads
them. This is about `hamlet_core`, the runtime.

## The runtime's modules

| Module | What it holds |
|---|---|
| `core` | The settings, the model, the sandbox and the home as resources |
| `tools` | The tools of one agent |
| `agent` | The loop one agent runs, written to its log |
| `agents` | The hamlets, agents and roads as entities, kept in step with the files |
| `session` | A conversation as an entity, and the worker that owns its agent |
| `oversight` | What waits for a chief |
| `terminal` | The terminal channel |
| `awake` | Keeps the machine awake while an agent works |

A module uses only those above it. `CorePlugin` adds `AgentsPlugin` and
`awake::plugin`, so `core` and `agents` know each other, and `core` knows
`awake`.

## A typed line

A channel triggers `UserMessage`. `session::deliver` reads the line once
with `hamlet_rules::commands::read` and goes by the command's `Handler`:

| Handler | Carried out by |
|---|---|
| `Hamlet` | `Hamlet::carry_out`, then the entities are brought in step |
| `Realm` | `chief::carry_out`, and every other open chat of the chief is told |
| `Oversight` | The event `Typed`, which `oversight/typed.rs` observes |
| `Switch` | The channel that shows the chat |
| `Session` | `deliver` itself |

A line that is no command is put to `Typed` as words, which may answer an
agent's question, and is otherwise a turn for the worker. Only a human's
line is ever a command: a letter between agents is words whatever it opens
with.

## Identity

An agent is its **number in the hamlet's record**, not its name. The number
is the component `Citizen(u64)`, and the entity that carries it is the
handle everything live uses. The entity is kept for the app's whole run,
through removal, merge and restore. A later agent of the same name has
another number and another entity.

A name is what a chief types and what a session's log is filed under.
`agents::named` turns a name into the living agent's entity at the edge,
once. Systems ask `agents::Roster` the same.

## Components

| Entity | Component | What it says |
|---|---|---|
| Hamlet | `Settlement { home, name }` | Where its record is kept |
| | `Seen` | How far its files had come when last read |
| | `Treasury(Report)` | Spend against the budget and how much of the period has gone by |
| | `Charter(Option<Limits>)` | Its limits as last read; nothing while the file cannot be read |
| Agent | `Citizen(number)` | Its identity |
| | `Name`, `Purpose` | |
| | `LivesIn(hamlet)` | Relationship to its hamlet |
| | `Mayor` | It holds the mayor's office |
| | `Left`, `MergedInto(agent)` | It was removed or merged away |
| | `Account(AgentBudget)` | Its model and its spend |
| | `Busy`, `Activity` | A turn is running; the step it is on |
| Road | `Road { ends }` | The two agents it joins |
| | `Traffic` | Its use |
| Conversation | `TalksTo(agent)` | The agent it is with |
| What waits | `RaisedBy(agent)` | The agent that raised it |

Everything the record decides is an immutable component: it can only be
replaced by inserting it again, and only `sync` and `account` do. `Busy`
and `Activity` are the two facts that are in no file, and the session
writes them.

A road is not a Bevy relationship, because it has two equal ends.

Events: `Carried { road, from }` when a road's count of messages has risen
since the sync before, and `Synced` after each sync. Leaving and returning
are `On<Add, Left>` and `On<Remove, Left>`.

## How the record and the entities stay in step

The files decide: `hamlet.jsonl`, the ledger under `spend/`, `limits.toml`.

- `agents::sync` reads the record and makes the entities say the same. It
  then runs `agents::account`, which does the same for spend and models.
- At start `AgentsPlugin` repairs the record and calls `sync`. A restart is
  the same call on the same files.
- While running, `agents::watch` compares each frame how far the three
  files have come with what `Seen` holds, and queues `sync` or `account`
  when one has moved. It costs three `stat` calls a frame.
- When a worker's event or a chief's command says the record changed, the
  session code calls `sync` at once, so observers of the same event see
  the entities already in step.

So there is one writer, it only ever copies from the files, and it runs
whenever the files have changed, whoever changed them. An event that is
lost costs one frame.

## What reads the entities

- **Sessions** write what a turn's events say an agent is doing to its
  `Busy` and `Activity`.
- **Oversight** relates each item that waits to its agent. When an agent
  leaves, its items lapse and its grants are revoked.
- **Panels** read again only when the resource `ui::Roll` has moved, never
  on a timer. `turn_roll` moves it from Bevy's change detection, with one
  count for the agents, one for the hamlets and their files, and one for
  what waits.
- **The chat** holds its agent's entity in `ui::Me`, and goes to the first
  mayor when its agent is gone.
- **The world**, below.

## The bridge to the world

`hamlet_world/src/hamlet.rs` puts the generator's id on the core's own
entity: `Housed(AgentId)` on an agent, `Laid(Pair)` on a road,
`Sited(TownId)` on the hamlet. Systems give a plot to an agent without
one and lay a road without one. Observers on `Left` and `Carried` turn a
house to a ruin, bring it back, and carry a letter down a road.

The generator's `Society` is its own model of towns, plots and roads, and
the `town_gen` example drives it with no core at all.

## The window's states

`Layout` (full, minimal) and `Showing` (shown, hidden) are two Bevy states.
With two, a hidden window keeps its layout and nothing has to remember
where to return to. A second launch sets `Showing::Shown` again, which
takes the focus.

## Shared ownership

`code_convention.md` asks for plain ownership first. The core holds an
`Arc<dyn Provider>`, because sessions share the model and an agent hands
it to the agent it messages, and a boxed approval hook (`Gate`) for the
same reason. The other boxes are what async recursion and async methods on
a `dyn` trait need.

Settings are owned by each job, so a changed setting reaches a worker with
its next job. The network switch is the exception: it is read for every
tool call and applies at once.
