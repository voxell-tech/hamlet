# What a robot does while its agent works alone

Each house has a work yard, and its robot does a chore there that matches what
the agent is doing. The chores are `hamlet_realm/src/keeper/work.rs`, their
motions `keeper/acts/chores.rs` and the yard `generator/yard.rs`. The task
to chore mapping is in `work.rs`.

See it with `cargo run --example town_gen`, then B to set the hamlet to work.

A robot never leaves its own plot while working alone. One with a letter and a
ring on a road is carrying a message.

## What the core tells the world

A `Work` for each agent: its `Task`, the count of steps begun and the count of
tool calls that failed. The world works out the rest: how long a step has run,
that work has ended, and when to show a failure.

- Most chores are shown for at least 2.4 s once the robot has reached its
  station, so a 50 ms `ls` is still seen as a few turns of the wheel; each
  chore's own minimum is `Chore::least`.
- A tool that starts and ends while the robot is busy with another is shown
  next. Only the latest such tool is kept, so one that came before it is
  not shown.
- A failure and the end of work cut in at once.

With `Task::Unknown` a robot keeps a round of its own, which has the shape of
an agent's loop but is invented.

Within a chore, small choices come from a hash of the robot's seed and the
time. Nothing is random at run time.

## The way home

A robot that stands away from its house, as when the chief's hand has set it
down somewhere, finds its way home once as it sets out
(`generator/homing.rs`). Over open ground it goes straight where that is dry;
where the straight way is wet it joins a road and crosses by its bridges;
where there is no dry way it does not travel: it is at its door again and
plays `Appear`.
