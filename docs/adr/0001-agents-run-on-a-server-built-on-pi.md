---
status: partly superseded by ADR 0009
---

# Agents run on a server, built in TypeScript on pi-durable

Hamlet is becoming a cloud software factory with a game for its interface:
agents keep working while the chief's machine is off, and their commands
never run on it. So the agent runtime and harness move to a server and are
rebuilt in TypeScript on pi-durable (`@earendil-works/pi-durable`), pi's
durable harness, and the window stays Rust and Bevy, as a client of that
server. The concepts stay (chief, mayor, agents, hamlets, roads, votes,
budget, realm, oversight); how they are built changes.

[ADR 0009](0009-the-server-is-rust-and-harnesses-run-the-conversations.md)
supersedes the choice of TypeScript for the server: Hamlet's rules move to
Rust, and pi-durable runs the conversations in a harness apart from them.
The server, the window as its client, and pi-durable for conversations
still hold.

pi-durable commits every turn, tool call and piece of state before anything
is shown, so a server that stops mid-turn picks the work up when it opens
again. Agents that work while nobody watches need that.

## Considered options

- The coding agent's `AgentSession` (`@earendil-works/pi-coding-agent`). It
  brings more ready-made (skills, context files, `grep` and `find`), but
  each session is its own file with nothing durable between turns, so
  Hamlet's records, its waits between agents and the window's view would all
  be our own code beside it.
- Keep the Rust loop and use pi only for its model providers. The smallest
  change, but agents neither run remotely nor are built on pi.
- Keep the Rust runtime and run pi beside it as a child process. Rejected:
  every model call and tool call would cross back to Rust for the rules,
  the gate, the sandbox and the log, and the transcript, compaction, models
  and spend would each exist twice.
- Stock `pi --mode rpc` with an extension. Rejected: Hamlet's own requests
  would have to pass through pi's dialog protocol.

## Consequences

- pi-durable is marked experimental and its API changes between releases,
  so its version is pinned and upgraded deliberately, and Hamlet's code
  reaches it through Hamlet's own extensions.
- The window no longer reads the records on disk (ADR 0004).
- The runtime in `hamlet_core`, and `hamlet_rules`, `hamlet_model`,
  `hamlet_sandbox` and `hamlet_home`, are rebuilt on the server. The window
  keeps the components and events its world and panels read.
- The project has two languages, with a protocol between them.
- The server is built to run on the chief's own machine first. Where it is
  deployed, and what that changes, is decided later.
