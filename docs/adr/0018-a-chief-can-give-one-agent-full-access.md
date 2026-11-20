---
status: accepted
---

# A chief can give one agent full access

ADR 0008 let commands run without a sandbox only on Windows, for a whole
server, chosen at every start. An agent's permissions are now its own, and
the top level, **Full access**, runs that agent's commands with no sandbox
on any platform: they reach whatever the account running the server can,
and the network is open to them. Only a chief gives it, one agent at a
time, and a chief can take it back at once. No agent can give it to itself
or to another, and an agent a parent adds never starts with it: it starts
with its parent's level, Full access lowered to Auto. A worker goes as far
as the agent that started it.

Full access changes nothing else. A folder an agent proposes still waits
for a chief, an act a chief reserved still waits, the built-in file tools
keep the reach the policy gives them, and secrets are still scrubbed from
what commands give back. A server that has no sandbox still refuses to
start, and Windows still needs `--insecure` for the commands of agents
that do not have Full access. A remote workspace runs its own policy, so
there Full access opens the network and nothing more.

## Considered options

- Keep ADR 0008 as it was, with Full access meaning only "no prompts".
  Rejected: a chief who trusts one agent with the machine had to trust all
  of them, or none, and the sandbox is the only thing that kept "Auto" and
  "no prompts" apart.
- A per-hamlet switch. Rejected: a hamlet holds agents of unlike trust,
  and a chief asked for the control per agent.
- Let a parent hand Full access down. Rejected: a parent that is not
  trusted with it would then hold it through a child.

## Consequences

- C18's "tools run in a sandbox" gains a second exception beside ADR 0008's
  insecure mode: an agent a chief gave Full access.
- A command of such an agent can read the chief's credentials and the
  other agents' worktrees, as in insecure mode (ADR 0008).
- The agent's prompt says its commands are not confined, so it does not
  plan around a sandbox it no longer has.
- The policy hash names `unconfined` for its commands, so the record of a
  call shows which kind it ran in.
