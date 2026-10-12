# What a robot does while its agent works alone

Each house has a work yard, and its robot does a chore there that matches
what the agent is doing. The chores are `hamlet_world/src/keeper/work.rs`,
their motions `keeper/acts/chores.rs` and the yard `generator/yard.rs`.

See it with `cargo run --example town_gen`, then B to set the town to
work.

## The yard

Every house has a paved strip along one side wall, with a desk and a
stool, a rack of scrolls for the agent's skills, and an engine with a
wheel. The yard takes the side of the house that faces the default camera,
so a working robot is not behind its own house.

## The chores

| The agent is | Chore | What is seen |
|---|---|---|
| Waiting for the model | `Ponder` | Walks up and down the yard, hand to chin |
| Reading notes or a file | `Study` | Sits on the stool with a book |
| Writing notes | `Jot` | Sits and writes in the book |
| Writing a file | `Write` | Stands bent over the desk with a quill |
| Running a command | `Run` | Works the engine's wheel, and rings of steam rise. On a long command it steps back and watches |
| Fetching from the network | `Fetch` | Flies a kite |
| Reading a skill | `Recall` | Holds a scroll up at arm's length |
| Saving a skill | `Stow` | Stamps the scroll at the desk |
| Using a tool added by hand | `Tinker` | Hammers at the desk |
| Waiting for an agent it asked | `Await` | Stands at its door and listens |
| Reflecting | `Reflect` | Looks up, and its light turns moon white |
| A tool call failed | `Oops` | Jumps back, and its light goes red for a moment |
| Finished | `Done` | Tips its hat, then walks home |

Each chore reads from far off: steam means a command, a kite means the
network. A robot never leaves its own plot while working alone. One with a
letter and a ring on a road is carrying a message.

## What the core tells the world

A `Work` for each agent: its `Task`, the count of steps begun and the
count of tool calls that failed. The world works out the rest: how long a
step has run, that work has ended, and when to show a failure.

- A chore is shown for at least 2.4 s once the robot has reached its
  station, so a 50 ms `ls` is still seen as a few turns of the wheel.
- A tool that starts and ends while the robot is busy with another is kept
  and shown next.
- A failure and the end of work cut in at once.

With `Task::Unknown` a robot keeps a round of its own, which has the shape
of an agent's loop but is invented.

## Variety

Which chore follows the agent's task. Within a chore, small choices come
from a hash of the robot's seed and the time. Between robots, each has its
own clock offset and tempo. Nothing is random at run time: with the clock
frozen, a still is the same whatever came before.

## The way home

A robot that stands away from its house, as when the chief's hand has set
it down somewhere, finds its way home once as it sets out
(`generator/homing.rs`).

- Over open ground it goes straight where that is dry, bending round plots
  and held ground.
- Where the straight way is wet it joins a road and crosses by its
  bridges.
- Where there is no dry way it does not travel: it is at its door again
  and plays `Appear`.
