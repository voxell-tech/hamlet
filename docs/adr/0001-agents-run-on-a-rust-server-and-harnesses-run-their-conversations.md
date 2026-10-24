---
status: accepted
---

# Agents run on a Rust server, and harnesses run their conversations

Hamlet is a cloud software factory with a game for its interface: agents
keep working while the chief's machine is off, and their commands never
run on it. So agents run on a server, and the window, in Rust and Bevy, is
a client of it (ADR 0004).

The server is Rust, the crate `hamlet_server`, and keeps everything Hamlet
means: agents, hamlets, the mayor's office, roads, letters, questions,
votes, tasks and their caps, budgets and spend, what waits for a chief,
standing permissions, objections, the charter, characters and settings,
which tools an agent may run and where, the client protocol and the connection's
token.

An agent's conversations run in a harness: a separate process the server
starts and watches, which makes the model calls and keeps the history,
the tool results, compaction and forks. The first harness runs
`@earendil-works/pi-durable` and `@earendil-works/pi-ai` in Node, pinned
at 1.2.0. pi-durable commits every turn and tool call before anything is
shown, so work cut off mid-turn carries on when its process opens again;
agents that work while nobody watches need that. The server and a harness
talk through a private, versioned interface of Hamlet's own, and neither
opens the other's store (ADR 0005).

The chief wants Hamlet in Rust, beside the window and the `hamlet`
command, and wants agents free to run on different harnesses that change
on their own schedule. A process boundary also keeps pi-durable's
experimental API out of Hamlet's rules.

A harness's own hooks are never the last word. A prototype tried the
split before it was built: a Rust process keeping roads, approvals, a
budget and markers, and a Node worker running real pi-durable 1.2.0 with
its own SQLite file and a faux model. A script killed either process, or
both, at fixed points and checked both stores and the kept transcript.
It found that pi-durable, recovering a tool call it had already begun,
can carry it out without running `beforeTool` again; that a change, its
charge and the marker of its result must commit together, since killing
the server between a change and its marker lost the result; and that
pi's task IDs are unique only beside the runtime's ID. So the server
allows every tool call and every change to the record when it is carried
out, under the permissions and limits as they are then.

## Considered options

- Hamlet's rules in TypeScript, inside the pi-durable process. One process
  and no recovery between processes, but Hamlet would stay in pi's
  language, with pi's storage, for good.
- Rust rules over one pi-durable storage shared with the harness.
  Rejected: Hamlet's records would be shaped by pi's storage, and no other
  harness could be added.
- The coding agent's `AgentSession` (`@earendil-works/pi-coding-agent`),
  `pi --mode rpc` or the coding agent's CLI. Rejected: each session is its
  own file with nothing durable between turns, and Hamlet's requests would
  pass through pi's dialog protocol.
- pi in V8 embedded in the Rust process. Rejected: it adds the upkeep of
  running pi's JavaScript dependencies inside Rust, and is not needed to
  put Hamlet in Rust.
- One harness process per agent. Rejected as a rule: agents whose harness
  and version match may share one; separate processes are for a different
  harness, a different version, or asked-for isolation.

## Consequences

- The project has two languages: Rust for Hamlet, TypeScript for the pi
  harness. The harness interface is a second protocol to version and test,
  beside the client protocol.
- Every tool call and every change to the record crosses from the harness
  to the server, and each must be safe to ask for twice.
- A conversation stays with the harness and version it began in. Changing
  an agent's harness changes only its new conversations.
- A new harness says what it can do, and is offered only if the server's
  permissions and budget decide every tool call and model call it makes.
- pi-durable's version is pinned and upgraded on purpose, as a new harness
  version.
- The server is built to run on the chief's own machine first. Where it is
  deployed, and what that changes, is decided later.

How the server and a harness work together is in
[`docs/harness.md`](../harness.md).
