---
status: accepted
---

# The server runs as a Bevy app with no frame

The server's logic runs on `bevy_ecs` and `bevy_app`, as the window's does.
Bevy organises the logic; it holds none of the realm. `realm.sqlite` stays
the truth (ADR 0012): the world holds only what lives in this process, such
as the connections, the uploads, the tool runs, the runtimes and the
conversations in memory. No row of the database is mirrored in the world.

**No frame: one input, one update.** `create_app` adds every area and sets
the runner. The runner sleeps until an input comes (a runtime's line, a
client's message, a tool run that ended, a finished task, the jobs runner's
pass, a change to the tools added by hand, what a terminal printed, a due
timer), writes it as a message, and updates the app once through Bevy's own
`Main`. Nothing updates on a tick. The first update opens what the server
serves in `PreStartup` (the sandbox, the realm, the credentials, the
providers, the jobs runner, the harness, the clients and the listener, each
a resource) and starts it in `Startup`; the address is published only once
that is over. An area adds a timer as a system that says when it is next
due, or as a period with the system that runs then (`app/timers.rs`).

**One input, one transaction.** `First` begins the input's transaction and
every change the input makes is in it. What it sets off waits for the end:
lines to runtimes and messages to clients are queued, and a tool run is
spawned as an entity whose task starts later. `PostUpdate` then keeps the
input and runs, in the order declared by `FollowUpSystems`: the ended
inputs are triggered for their observers; tools are narrowed to each role;
those changes are kept; the jobs they left run; cancelled runs stop and
asked ones start. `Last` sends the runtimes what they are owed, the clients
the state and then their replies, and the runtimes their queued lines. Each
step is worked out again from the database, so one that fails is made good
by a later input.

**Handlers are systems, and none awaits.** `PreUpdate` reads an input into
a message of its kind (`FromRuntime<M>`, `FromClient<M>`). Each kind has a
system in `Update`, run only when such a message came. A chief's command is
added with its usage and its system in one call, so what a client lists and
what runs cannot drift. What awaits the network, the remote workspace or
files runs on a task, and its result comes back as an input; a call that
waited on one is asked again in the same update and finds it.

**An input that fails does not stop the server.** Systems return Bevy's
`Result`, and the world's error handler records an error or a panic. The
first step of `PostUpdate` then rolls the whole transaction back and drops
what the input queued, so no runtime or client hears of a change that was
not kept; it tells the sender, and the runner logs the failure with
secrets taken out and goes on. Only a `Fatal` error, or a database SQLite
calls corrupt or not a database, stops the server. The jobs runner
survives a job that panics, which runs again later.

**Stopping is a state, and its steps are inputs.** A stop signal takes the
`Serving` state from `Open` to `Closing`, whose systems close the clients
and end the sign-ins; the server keeps running inputs while a credential
rotation it began finishes. Then `Stopping` ends the tool runs and stops
the runtimes on a task whose end is an input, and the app exits. A `Fatal`
error stops the server the same way.

## Considered

- One struct owning every field and a loop matching every message.
  Rejected: adding a feature meant editing central matches.
- Each handler as a one-shot system the runner picks. Rejected: the runner
  held every area's dispatch.
- Rows mirrored as entities. Rejected: an entity is updated only after the
  commit, so a decision read from it inside a transaction can contradict
  what the transaction already wrote.
- Conversations in memory as entities. Rejected: a spawned entity appears
  only once its input's commands apply, so the input that loads a
  conversation could not reach it; they are a resource.
- Bevy's assets for the tools added by hand. Rejected: its loader and file
  watcher are read by frame schedules, so the server would have to tick; a
  watcher of its own wakes the runner instead.
