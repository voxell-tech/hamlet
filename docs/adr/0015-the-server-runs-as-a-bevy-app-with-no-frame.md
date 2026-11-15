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

**No frame: one input, one update.** `ServerPlugin` adds every area and
sets the runner. The runner sleeps until an input comes (a runtime's line,
a client's message, a tool run that ended, a task that finished, the jobs
runner's pass, a change to the tools added by hand, a timer that is due),
writes it as a message, and updates the app once through Bevy's own
`Main`; then it sleeps again. Nothing updates on a tick. The first update
runs `Startup`. A timer is added by its area as a system that says when
it is next due, or as a period, with the system that runs then
(`timers.rs`).

**One input, one transaction, then what follows.** `First` begins the
input's transaction and every change it makes is in it. What it sets
off waits for its end: the lines to the runtimes and the messages to the
clients are queued, a tool run is spawned as an entity whose task starts
later, and an input that ended in a runtime is written as a message.
`PostUpdate` then keeps the input and runs, in a declared order
(`ServerSystems`): the ended inputs are triggered for their observers,
which follow them; tools are narrowed to each role; those changes are
kept; the jobs the kept changes left run; cancelled runs stop and the
asked ones start. `Last` sends the runtimes what they are owed, the
clients the state and then their replies, and the runtimes their queued
lines. Each step is worked out again from the database, so one that
fails is made good by a later input.

**Handlers are systems, and none awaits.** `PreUpdate` reads an input
into a message of its kind: a runtime's message into a `FromRuntime<M>`,
a client's request into a `FromClient<M>`. Each kind has a system of its
own in `Update`, run only when such a message came. A chief's command is
added with its usage and the system that runs it in one call, so what a
client lists and what runs cannot drift. A model's tool name is read
once into a typed tool. What awaits the network, the remote workspace or
files off the loop runs on a task, and its result comes back as an
input; a call that waited on one is asked again in the same update and
finds it.

**An input that fails does not stop the server.** Systems return Bevy's
`Result`, and the world's error handler records an error or a panic
instead of stopping. The first step of `PostUpdate` then rolls the whole
transaction back and drops what the input queued, so no runtime or client
hears of a change that was not kept and no run it asked for starts; it
tells the client or the runtime that sent the input, and the runner logs
the failure with secrets taken out and goes on. Only a `Fatal` error, or
a database SQLite calls corrupt or not a database, stops the server. The
jobs runner survives a job that panics, which runs again later.

**Considered.** Keeping one struct that owned every field and a loop that
matched every message: it is what the server was, and adding a feature
meant editing central matches. Running each handler as a one-shot system
the runner picks, with schedules of the server's own around it: the
runner then held every area's dispatch, and the schedules were named
after an input no schedule stood for. Mirroring rows as entities: an entity is
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
