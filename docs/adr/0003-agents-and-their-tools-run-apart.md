---
status: accepted
---

# Agents and their tools run apart

An agent's conversation runs in the server, and its tools touch files and
processes only through pi-durable's `ExecutionEnv`, which the server builds
for each tool call. Hamlet's `ExecutionEnv` runs them in today's sandbox,
ported: bubblewrap on Linux and Seatbelt on macOS, under the same policy.

Keeping the tools behind that interface means a deployed server can run them
somewhere else without the agent noticing. pi's own `@earendil-works/pi-env`
already runs an `ExecutionEnv` on another machine over SSH. Where tools run
when deployed is decided with the deployment.

## Considered options

- Running the harness inside the sandbox, next to the tools. Rejected: an
  agent's life would be tied to wherever its commands run.
- A Linux container for local runs, so bubblewrap works on every OS.
  Rejected for now: local runs would need Docker.

## Consequences

- The reach rules in `docs/sandbox.md` are enforced by the ported policy,
  and any other place tools run later needs the same rules and its own
  tests.
