# Remote workspaces behind existing tools

What remains to build and prove for remote workspaces. What works now is
in [`sandbox.md`](../sandbox.md#remote-workspaces) and the worker's
[README](../../crates/hamlet_workspace/README.md). ADR 0003 and S12 place
tool execution behind one interface; the remote workspace must preserve
the applicable C18-C21, S3 and S7 contracts.

## Outcome

Agents use the existing bash, read, write, edit, find and search tools.
File and process operations reach the same remote filesystem. Agents
never see the machine, the worker or its tokens. Hamlet's server holds
only the worker's address and its home's token, and no cloud
credentials.

One crate, `hamlet_workspace`, holds the worker: one process on a Linux
machine the chief runs, which Hamlet's server talks to directly. Keep
the local workspace as the default.

An earlier prototype put each hamlet in a Substrate actor behind a
separate manager that created, suspended and resumed it. It was
dropped; its code is in the git history.

## Ownership

- Hamlet owns conversations, agents, permissions, prompts, model
  credentials, and the domain behavior of houses, skills, gifts and
  archives.
- The worker owns the homes' authentication, the operation journal and
  each operation's filesystem and network reach. It understands files
  and processes, not agent conversations, gifts or archives.
  Model-authored arguments cannot choose another home, workspace, house
  or permission grant.
- The operator owns the machine, its storage and backups, TLS in front
  of the worker, and its network's private ranges.

The server's adapter must not expose a second set of agent tools or
silently fall back to host file access.

## Workspace model

One folder per hamlet on the worker: one shared project and separate
private agent houses, named by the home and the hamlet, never by a path
on the chief's machine. Server records, conversations and model
credentials remain in Hamlet. Prompt readers, chief edits, gifts and
archive operations address the same files as the tools; their domain
logic stays in Hamlet. No background local-folder synchronization or
credential copying is part of this work.

Operations the protocol cannot do safely remotely, such as founding,
moving, removing, restoring or erasing a hamlet and moving a house, are
refused as capabilities of the workspace rather than done on stale local
copies.

## What remains

- An automated end-to-end check: the real server, pi with a faux model
  and the real worker, through the existing tools: write, execute/read,
  edit and read back. Today the server is tested against a loopback
  double and the worker on its own.
- The worker outside Docker, and Docker hosts with AppArmor: which user
  namespace, AppArmor and seccomp settings it needs.
- Telling an emptied `root` from a new one. A missing root stops the
  worker, but an emptied one gives agents empty houses.
- A way to inspect an uncertain operation by its request id. The
  journal records it; nothing reads it for the chief.
- Pruning the journal, which keeps every operation's marker and reply.
- The isolation requirements below on each host the worker runs on.

## Isolation requirements

- Filesystem reach enforced for both file helpers and arbitrary shell
  commands, including traversal, symlinks, root replacement, proc access
  and inherited FDs.
- A denied command cannot reach network endpoints. Transport to the
  worker is infrastructure traffic, not an agent's network grant.
- Network-enabled commands cannot reach the worker's own endpoint or
  acquire infrastructure credentials.
- Bounded output and cancellation of all descendants, with a completion
  barrier before reporting that a process tree is gone.

The Docker check in the worker's README covers house and project reach,
hidden host paths and the two network cases on one host. It does not
cover the rest of this list, or resource limits, which are the
machine's.

If a required boundary cannot be enforced, fail closed and report the
blocked contract. Do not silently relabel the remote workspace safe or
replace isolation with agent naming conventions. Any weaker trust model
or different layout needs a chief decision.

## Lifecycle and recovery

Assign operation IDs before mutations. Never automatically repeat a
command or uncertain write after a timeout, disconnect or restart.
Reconcile its known outcome or return an uncertain result with safe
recovery advice. Hamlet shutdown or client disconnection never deletes
a workspace; deletion is an operator's action on the worker's machine.
The worker runs continuously; there is no suspension.

## Out of scope

- Deploying the Hamlet server itself remotely.
- Several workers for one home, or a scheduler that starts and stops
  them.
- Automatic local-folder synchronization, or automatic git, model or
  cloud credential injection.
- Persistent background jobs, automatic destructive retention, or new
  agent tools.
- Rewriting houses, skills, gifts or archives as worker concepts.

## Sources

- [ADR 0003](../adr/0003-agents-and-their-tools-run-apart.md)
- [Core requirements](../spec/core.md) and [server requirements](../spec/server.md)
