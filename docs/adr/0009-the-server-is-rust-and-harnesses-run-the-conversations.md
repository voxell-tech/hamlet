---
status: accepted
---

# The server is Rust, and harnesses run the conversations apart from it

The server is rebuilt in Rust, as the crate `hamlet-server`, and keeps
everything Hamlet means: agents, hamlets, the mayor's office, roads,
letters, questions, votes, tasks and their caps, budgets and spend, what
waits for a chief, standing permissions, objections, the charter,
characters and settings, which tools an agent may run and where, protocol
v0 to the window and the terminal, and the connection's token. It keeps
these in a store of its own.

An agent's conversations run in a harness: a separate process the server
starts and watches, which makes the model calls and keeps the history, the
tool results, compaction and forks in a store of its own. The first harness
is a Node process running `@earendil-works/pi-durable` and
`@earendil-works/pi-ai`, pinned at 1.2.0. The two talk through a private,
versioned interface of Hamlet's own; neither opens the other's store.

The chief wants most of Hamlet in Rust, beside the window and the `hamlet`
command, and wants agents free to run on different harnesses that change
on their own schedule. Keeping pi-durable behind a process boundary also
keeps its experimental API out of Hamlet's rules.

A prototype with real processes and real pi-durable 1.2.0 found that
pi-durable, recovering a tool call it had already begun, can carry it out
without running `beforeTool` again. So a hook in the harness is never the
last word: the server allows every tool call and every change to the
record when it is carried out, under the permissions and limits as they
are then. A change commits with its charge and the marker of its result
in one transaction of the server's store, so a call that runs again gets
the marked result back. The proposal is
[`docs/proposals/rust-server.md`](../proposals/rust-server.md).

This supersedes ADR 0001's choice of TypeScript for the server's rules,
and ADR 0005's one storage for a home. ADR 0002 holds for the pi harness,
and ADR 0003 holds: tools still run apart from the conversation, now
through the server.

## Considered options

- Keep the TypeScript server. It holds the rules and pi-durable's hooks in
  one process and needs no recovery between processes. Rejected: it keeps
  Hamlet in pi's language, with pi's storage, for good.
- Rust rules over pi-durable's one storage, shared with the harness.
  Rejected: Hamlet's records would be shaped by pi's storage, and no other
  harness could be added.
- One harness process per agent. Rejected as a rule: agents whose harness
  and version match may share one; separate processes are for a different
  harness, a different version, or asked-for isolation.
- `pi --mode rpc` or the coding agent's CLI. Rejected, as in ADR 0001:
  Hamlet's requests would pass through pi's dialog protocol.
- pi in V8 embedded in the Rust process. Rejected: it adds the upkeep of
  running pi's JavaScript dependencies inside Rust, and is not needed to
  put Hamlet in Rust.
- Pi's `beforeTool` hook as the gate. Rejected by the prototype: recovery
  can skip it.

## Consequences

- The project still has two languages: Rust for Hamlet, TypeScript for the
  pi harness. The harness interface is a second protocol to version and
  test, beside protocol v0.
- Every tool call and every change to the record crosses from the harness
  to the server, and each must be safe to ask for twice.
- A conversation stays with the harness and version it began in. Changing
  an agent's harness changes only its new conversations; moving an existing
  one is a migration, which is left for later.
- A new harness says what it can do, and is offered only if it meets what
  Hamlet never gives up: the server's permissions and budget decide every
  tool call and model call.
- `server/` stays, as the reference for behaviour, until the Rust server
  does everything it does. Then it is removed; there are never two
  servers for good.
- A home is claimed the same way by both servers, so the old and the new
  can never open one home at once.
