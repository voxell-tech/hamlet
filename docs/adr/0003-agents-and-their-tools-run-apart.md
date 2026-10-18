---
status: accepted
---

# Agents and their tools run apart

An agent's conversation runs in its harness, and its tools touch files and
processes only through the server, which runs each under the agent's
policy: in the sandbox on the chief's machine, bubblewrap on Linux and
Seatbelt on macOS, or in a remote workspace. In the pi harness, Hamlet's
`ExecutionEnv` sends each operation to the server.

Keeping the tools behind that one interface means where they run can
change without the agent noticing.

## Considered options

- Running the harness inside the sandbox, next to the tools. Rejected: an
  agent's life would be tied to wherever its commands run.
- A Linux container for local runs, so bubblewrap works on every OS.
  Rejected for now: local runs would need Docker.

## Consequences

- The reach rules in `docs/sandbox.md` are enforced by the server's
  policy, and any other place tools run needs the same rules and its own
  tests.
- A harness holds no host tools of its own.
