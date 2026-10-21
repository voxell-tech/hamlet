# The harness

An agent's conversations run in a harness, a process apart from the
server ([ADR 0001](adr/0001-agents-run-on-a-rust-server-and-harnesses-run-their-conversations.md)).
This page explains how the server and a harness work together: runtimes
and bindings, the runtime protocol between them, the IDs each side makes, and what a
harness must be able to do. The pi harness's files are listed in
[the tour](tour.md#the-pi-harness).

## Harnesses, runtimes and bindings

A harness is a kind of program that runs conversations. pi, in
`harnesses/pi/`, is the only one. A runtime is one running process of a
harness, with a stable ID and its own store under `harnesses/<instance>/`,
which only it opens. Neither the server nor a runtime opens the other's
store (ADR 0005).

- An agent has a harness for its new conversations, kept by the server.
- New conversations of a harness go to its one runtime. An agent is not a
  process.
- A conversation has a binding: its runtime, the runtime's ID for the
  conversation, and the adapter and harness versions it began with. A
  conversation never moves to another runtime (S17).
- A runtime's versions never change. A runtime that reports other
  versions than it began with is refused.
- A one-off conversation, such as a ballot, the mayor's review or a
  library call, is a conversation like any other, bound to a runtime.

## The runtime protocol

The server and a runtime talk through one private interface of Hamlet's.
Its transport is the runtime's standard input and output, which only the
server holds: one JSON message per line. The messages are the Rust types
of `crates/hamlet_runtime_protocol`, which a harness written in Rust can
use directly. `cargo test -p hamlet_runtime_protocol` writes their JSON
Schema to
`harnesses/pi/src/protocol-schema.ts`, and fails when it was stale so the
new file is committed; pi takes its types and its checks from that file,
so both sides are checked against one schema. Version 1 is the only one.

Every message carries the protocol version and the sender's ID for it. A
reply names the ID it answers. Errors are messages, not closed
connections.

### Starting

1. The server starts the runtime with `--runtime <id>`, `--store <path>`
   and `--transport stdio`. It adds `--fresh` only when the runtime has never
   begun, the one time its store may be made.
2. The runtime says `hello`: the protocol versions it speaks, its adapter and
   harness versions, the runtime ID its store records, whether the store
   was created, opened or is missing, and its capabilities.
3. The server refuses a protocol version it does not speak, a missing store, a
   store that records another runtime or none, versions other than those
   the runtime began with, and a runtime that lacks a needed capability.
   The refusal is in words for a chief, never a fallback, and a refused
   runtime is not started again.
4. Otherwise the server answers `welcome` with the tools the runtime's
   conversations may offer and each bound conversation's context window,
   then creates the conversations it owes and submits the inputs it owes.

A runtime that fails stops only its own conversations (S22). The server
starts it again after 250 ms, doubling with each failure up to 30 s, and
forgets the failures once it has run for a minute. A runtime whose
standard input closes stops its work and exits, so no agent runs without
its server.

### Admitting inputs

The server keeps every input it owes a conversation, in order, before
submitting it. A runtime admits a request ID once: submitting it again
admits nothing new and answers as the first time did. The server marks an
input admitted only when the runtime says it kept it, and submits it
again after a restart or a lost reply (S23).

### IDs

| ID | Made by | Unique within |
|---|---|---|
| Runtime ID | The server, kept in its store and the runtime's | The home |
| Conversation ID | The runtime | Its runtime |
| Request ID of an input | The server, kept before it is submitted | Its conversation |
| Operation identity | The runtime ID with a key local to the runtime | The home |
| Attempt identity | The runtime ID with a key local to the runtime | The home |

A request ID begins with its kind, such as `chief:`, `retry:`, `reflect:`,
`notice:` or `reply:`. Those of letters, questions, replies and ballots
come from the server's record of them. pi's task IDs are numbers local to
its store, so an operation identity is the runtime ID and the task ID
together. A provider's tool-call ID is never treated as unique.

## Capabilities

A runtime declares what it can do in its `hello`. What Hamlet never gives
up is not optional: a runtime that lacks it is refused (S24).

| Capability | Promises | Needed |
|---|---|---|
| `operations` | Every tool call and change to the record goes through the server as an operation | Always |
| `attempts` | Every model call is allowed by the server first, and its usage reported with an identity | Always |
| `admission` | Inputs admitted once by request ID, in order | Always |
| `cancel` | Stopping a running turn and dropping its queue | Always |
| `recovery` | Carrying on its own work after a restart | Always |
| `updates` | Updates of its conversations, sent only once kept | Always |
| `history` | The transcript in pages | No |

A new harness needs its own check against this table and its own tests;
what is shown for pi does not carry over to it.
