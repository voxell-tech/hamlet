# The harness

An agent's conversations run in a harness, a process apart from the
server ([ADR 0001](adr/0001-agents-run-on-a-rust-server-and-harnesses-run-their-conversations.md)).
The pi harness's files are in [the tour](tour.md#the-pi-harness).

## Harnesses, runtimes and bindings

A harness is a kind of program that runs conversations; pi, in
`harnesses/pi/`, is the only one. A runtime is one running process of a
harness, with a stable ID and its own store under `harnesses/<instance>/`,
which only it opens (ADR 0012).

- An agent has a harness for its new conversations; they go to that
  harness's one runtime. An agent is not a process.
- A conversation has a binding: its runtime, the runtime's ID for the
  conversation, and the adapter and harness versions it began with. It
  never moves to another runtime (S17), and a runtime that reports other
  versions than it began with is refused.
- A conversation has a kind, told to the runtime at creation: `chat`,
  `worker`, `errand` or `dream`.

## The runtime protocol

The server and a runtime talk over the runtime's standard input and
output, which only the server holds: one JSON message per line. The
messages are the Rust types in `hamlet_protocol::runtime`; pi takes its
types and checks from the schema `cargo test -p hamlet_protocol` writes
to `harnesses/pi/src/protocol-schema.ts` (commit it when it changes). A
runtime is taken on when its version is compatible with the server's by
semver's caret.

Every message carries the protocol version and the sender's ID for it. A
reply names the ID it answers. Errors are messages, not closed
connections.

### Starting

1. The server starts the runtime with `--runtime <id>`, `--store <path>`
   and `--transport stdio`, and `--fresh` only the one time its store may
   be made.
2. The runtime says `hello`: its protocol and harness versions, the
   runtime ID its store records, the store's state and its capabilities.
3. The server rejects an unknown protocol version, a missing store, a
   store that records another runtime or none, changed versions, and a
   missing capability. The rejection is in words for a chief, with no
   fallback, and the runtime is not started again.
4. Otherwise the server answers `welcome`, then creates and submits what
   it owes.

A runtime that fails stops only its own conversations (S22). The server
starts it again with a growing delay. A runtime whose standard input
closes stops its work and exits.

### Earlier entries

A `create` may carry `earlier`, the entries of a conversation brought in
from another program (C35). The runtime keeps them in the commit that
makes the conversation, answers `created`, then sends `kept` for each,
so the server keeps them in order. Asking again sends them again, and
the server keeps each once.

### Admitting inputs

The server keeps every input it owes a conversation, in order, before
submitting it. A runtime admits a request ID once: submitting it again
admits nothing new. The server marks an input admitted only when the
runtime says it kept it, and submits it again after a restart or a lost
reply (S23). It submits the next input only once the turn before has
ended, and none while a chief holds the chat stopped; `write` and
`compact` are not turns and are sent at once.

### IDs

The server makes the runtime ID and each input's request ID, which it
keeps before submitting; the runtime makes the conversation ID, unique
within it. An operation or attempt identity is the runtime ID with a key
local to the runtime. A provider's tool-call ID is never treated as
unique.

## Capabilities

A runtime declares what it can do in its `hello`. A runtime that lacks
what Hamlet never gives up is rejected (S24). Only `history` is
optional. The rest are `operations` (every tool call and change to the
record goes through the server), `attempts` (every model call allowed
first, its usage reported), `admission`, `cancel`, `recovery` and
`updates` (sent only once kept). A new harness needs its own tests
against these.
