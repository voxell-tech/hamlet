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
run that ended, the jobs runner's pass, a timer that is due) and runs that
input alone, then sleeps again. Nothing runs on a tick. A timer is added
by its area as a system that says when it is next due, or as a period,
with the system that runs then (`deadline.rs`).

**One input, its changes, then what follows.** An input's handler makes
its changes, each in one transaction. The `Apply` schedule then runs once,
in a declared order: tools narrowed to each role, cancelled runs stopped,
the runtimes sent what they are owed, the clients sent the state, then
their replies. Each step is worked out again from the database, so one
that fails is made good by a later input.

**Handlers are systems.** Each runtime message and each client request
runs a one-shot system of its own, chosen by its type where the message is
read. A chief's command is added with its usage and the system that runs
it in one call, so what a client lists and what runs cannot drift. A
model's tool name is read once into a typed tool. A handler that awaits
the remote workspace or reads files off the loop stays an async function
over the realm's live state, which the runner awaits.

**An input that fails does not stop the server.** Every input goes
through one wrapper: an error or a panic rolls its transaction back,
including what a panic left open, is logged with secrets taken out, and
the server goes on. The world's error handler records a failing system
instead of panicking. Only a `Fatal` error, or a database SQLite calls
corrupt or not a database, stops the server. The jobs runner survives a
job that panics, which runs again later.

**Considered.** Keeping one struct that owned every field and a loop that
matched every message: it is what the server was, and adding a feature
meant editing central matches. Mirroring rows as entities: an entity is
updated only after the commit, so a decision read from it inside a
transaction can contradict what the transaction already wrote.
