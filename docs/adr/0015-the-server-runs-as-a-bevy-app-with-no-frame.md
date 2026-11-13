---
status: accepted
---

# The server runs as a Bevy app with no frame

The server's logic runs on `bevy_ecs` and `bevy_app`, as the window's
does. Bevy organises the logic; it holds none of the realm. `realm.sqlite`
stays the truth (ADR 0012): the world holds what lives in this process
alone, such as the connections, the uploads, the tool runs, the runtimes
and the conversations in memory. No row of the database is mirrored in
the world.

**No frame.** `serve::run` builds an `App` and sets its runner. The runner
sleeps until an input comes (a runtime's line, a client's message, a tool
run that ended, a task that finished, the jobs runner's pass, a change to
the tools added by hand, a timer that is due) and runs that input alone,
then sleeps again. Nothing runs on a tick. A timer is added by its area
as a system that says when it is next due, or as a period, with the
system that runs then (`deadline.rs`).

**One input, one transaction, then what follows.** Every change an input
makes is in its one transaction, kept at its end. What it sets off waits
for that: the lines to the runtimes and the messages to the clients are
queued, a tool run is spawned as an entity whose task starts later, and
an input that ended in a runtime is written as a message. The `Apply`
schedule then runs once, in a declared order: the ended inputs are
triggered for their observers, which follow them; tools are narrowed to
each role; those changes are kept; the jobs the kept changes left run;
cancelled runs stop and the asked ones start; the runtimes are sent what
they are owed, the clients the state and then their replies, and the
runtimes their queued lines. Each step is worked out again from the
database, so one that fails is made good by a later input.

**Handlers are systems, and none awaits.** Each runtime message and each
client request runs a one-shot system of its own, chosen by its type
where the message is read. A chief's command is added with its usage and
the system that runs it in one call, so what a client lists and what
runs cannot drift. A model's tool name is read once into a typed tool.
What awaits the network, the remote workspace or files off the loop runs
on a task, and its result comes back as an input; a call that waited on
one is asked again and finds it.

**An input that fails does not stop the server.** Every input goes
through one wrapper: an error or a panic rolls its whole transaction back,
including what a panic left open, drops what it queued, so no runtime or
client hears of a change that was not kept and no run it asked for
starts, is logged with secrets taken out, and the server goes on. The
world's error handler records a failing system instead of panicking.
Only a `Fatal` error, or a database SQLite calls corrupt or not a
database, stops the server. The jobs runner survives a job that panics,
which runs again later.

**Considered.** Keeping one struct that owned every field and a loop that
matched every message: it is what the server was, and adding a feature
meant editing central matches. Mirroring rows as entities: an entity is
updated only after the commit, so a decision read from it inside a
transaction can contradict what the transaction already wrote. The
conversations in memory as entities of their own: a spawned entity
appears only once its input's commands apply, so the input that loads
a conversation could not reach it; the conversations are a resource.
The lines to a runtime are entities, since no input reads back one it
sent: a reply finds its request among those sent before. Bevy's assets for
the tools added by hand: its loader and its file watcher are read by
frame schedules, so the server would have to tick; a watcher of its own
wakes the runner instead.
