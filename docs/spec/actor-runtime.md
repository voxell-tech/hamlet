# The actor runtime: how the realm is kept

Hamlet's backend is made of actors. Each actor owns one SQLite file and
changes it only while it handles a message. This page says what the actors
are, where they keep their data, who may call whom, and what survives a
stop. The decisions behind it are
[ADR 0015](../adr/0015-each-actor-keeps-its-data-in-one-plain-sqlite-file.md)
and [ADR 0016](../adr/0016-crates-that-hold-ported-code-are-apache-2-0-only.md).
What the design names and is not built is in
[`docs/proposals/actor-runtime.md`](../proposals/actor-runtime.md).

## The actors

**A1.** A realm is kept by actors of five kinds. An actor is found by its
kind and its key, and its key never changes.

| Kind | Key | Process | What it owns |
|---|---|---|---|
| `realm` | `local`, one per realm | The Rust host | The chiefs, hamlets, the tree of hires, workers and errands, roads, letters and their files, tasks, charges, budgets and allowances, waits, objections and grants, the chief's documents and settings, the record, and the world a client sees |
| `agent` | The agent's number: the Hand is 1 | The Node host | Every conversation of that agent, the turn in flight, its timers and its own filesystem. A worker and an errand are agents too |
| `workspace` | `local`, one per machine | The Rust host | The sandbox, the agents' commands and file changes in the project folders, each agent's reach, the calls that wait for a chief, and the workers' scratch folders |
| `profile` | The profile's id | The Rust host | A profile's repository: its character, memory and skills with their history, its versions and its hires' branches |
| `library` | `local` | The Rust host | The library's repository and its checkout, which every agent reads |

An agent actor is per agent, not per conversation: an agent's conversations
with each chief, with each agent that writes to it, and its reflections all
live in its one file. A command is not an actor; it is a row of the
workspace. The mayor is an office the realm records, not a kind. A dream, a
librarian's call and a lead's review are errands: short-lived agents the
realm starts and ends.

**A2.** Two processes run the actors. The server, `hamlet_server`, is the
Rust host: it runs the realm, the workspace, the profiles and the library,
and serves the client protocol. The Node host, `hosts/node`, is the Rust
host's child and runs the agents on pi-durable. A window or the CLI connects
to the Rust host only, and the Node host never outlives it (S21).

**A3.** The two hosts talk over the child's pipe. The Node host says hello
with its link protocol's version, the fingerprint of the link's frames, its
kinds and its process id. The Rust host welcomes it with its own and the
route table, or refuses it in words: a version not compatible by semver's
caret, another fingerprint, or other kinds. When the Node host ends, the
Rust host starts another a second later, and doubles the pause, up to a
minute, while each one ends before it has stayed ready for a minute. A call
to an agent fails at once while no Node host is ready.

## Where things are stored

**A4.** Everything is in the realm folder, `~/.hamlet` unless `HAMLET_HOME`
names another:

```text
~/.hamlet/
├─ actors/
│  ├─ realm/local.sqlite            the realm actor
│  ├─ realm/local.files/            images, files and parcels, each named by its SHA-256
│  ├─ workspace/local.sqlite        the workspace actor
│  ├─ workspace/local.output/<n>/   agent n's spilled command output
│  ├─ profile/<id>.sqlite           a profile actor's store
│  ├─ library/local.sqlite          the library actor's store
│  └─ agent/
│     ├─ <n>.sqlite                 agent n: its conversations and timers
│     └─ <n>.fs/                    agent n's own filesystem
│        ├─ memory/
│        ├─ skills/<name>/SKILL.md
│        └─ inbox/
├─ profiles/<id>.git                each profile's repository
├─ library.git, library/            the library's repository and its checkout
├─ dreams/<n>/                      the checkouts a dream writes
├─ workers/<n>/                     a worker's scratch folder
├─ tools/                           the tools a chief adds by hand
├─ claude/                          the Claude program's own folder
├─ credentials.json                 credentials, mode 0600, written whole
└─ server.json, server.log          the running server's address, and its log
```

The project a hamlet works on stays where the chief cloned it, and nothing
is copied between it and the realm folder (C20).

**A5.** Each actor keeps its durable data in its one file. The host opens it
in write-ahead mode with `synchronous=NORMAL`, in SQLite's exclusive locking
mode, so no other program opens it while the host runs. The runtime's tables
are named `_actor_*`, and the host's one row per file is `_hamlet_actor`:
the actor's generation, whether it is running or destroyed, the deadline its
last clean stop left, and the input it was created with. After a clean stop
the file is a plain SQLite database that `sqlite3 -readonly` reads.

**A6.** A host locks the realm folder under its name, `host.lock` for the
Rust host and `node.lock` for the Node host, and `actors/<kind>/.lock` for
each kind it runs, so no two hosts hold one kind. The server also claims the
folder by its canonical path (S2), so a second server on the same realm is
refused.

## Who may call whom

**A7.** A host checks every call against a fixed table of which caller may
call which kind. A call outside it fails, and the host reports it.

| Caller | May call |
|---|---|
| A client | The realm, an agent, a profile and the library |
| The realm | An agent, the workspace, a profile and the library |
| An agent | The realm, the workspace, a profile and the library |
| The workspace | The realm and an agent |
| A profile, the library | An agent |

No entry lets an agent call another agent. `tell` and `ask` are tools that
call the realm, which checks the road (H30), keeps the letter, charges the
road and makes the delivery. The society's hierarchy is rows of the realm,
so ending or merging a hire changes rows and moves no agent.

**A8.** Each call names its caller, as the host stamps it, and every handler
checks it. A call has a deadline and is not kept: one that is lost with its
host fails, and its caller does not know whether it ran.

**A9.** A call made again does what it did once (S19). The realm keeps a
marker of each act under its request id or its tool call's id; the
workspace keeps each command and file change under its agent and call id;
a profile and the library keep each change under its id. A repeat with the
same request gets the first answer. The same id with another request is
refused.

## How work survives a stop

**A10.** After a crash, Hamlet starts again and its saved data is intact.
Anything in flight may be lost.

**A11.** At its start a host starts every actor that a kill left marked
running and every actor whose deadline has come, without waiting for a call,
and arms a timer for every later deadline. An actor that does nothing sleeps,
and a sleeping actor with a deadline is started by it.

**A12.** A change an actor makes is one transaction of its own file, and a
refused change leaves nothing. Work that spans actors, such as a hire, is a
row of steps in the realm: each step is safe to run again, and each is
recorded done by a transaction of its own. At the realm's start, each row of
steps not closed goes on from its last done step. A hire, its end, a merge,
a split, an erasure, a worker, an errand, a review, a dream and a letter's
files each run this way.

**A13.** The realm gives one agent its deliveries in the order their acts
committed. A delivery that only the realm can make again, such as a wait's
notice or the librarian's answer, is tried again on a growing pause, 1 s
doubled up to 60 s, until the agent takes it, refuses it, has left, or the
realm stops.

**A14.** A one-shot timer is tried at most five times. Each try is claimed in
the actor's file before its handler runs, and the timer's row goes once the
handler answers. A timer left with five tries is set aside, runs no more,
and is reported. A timer that came due while every process was down fires
at the next start.

**A15.** A command is a row of the workspace before it runs. A command that
was running when its host stopped is marked cut off at the workspace's next
start, and its agent is told of the end once (C16).

**A16.** An agent's turn keeps each tool call and its result in the agent's
file. A tool call cut off by a stop runs again at the next start under the
same id, so the realm and the workspace answer it from what they kept (S3).

## The licence of ported code

**A17.** The crates and packages that hold code ported from the reference
project are licensed Apache-2.0 alone, and each carries the licence and a
`NOTICE` of its origin (ADR 0016). Everything else is under the workspace's
`MIT OR Apache-2.0`.
