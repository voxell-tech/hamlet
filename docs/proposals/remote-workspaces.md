# Remote workspaces behind existing tools

What remains to build and prove for remote workspaces. What works now,
locally, is in [`sandbox.md`](../sandbox.md#remote-workspaces); nothing
has run on GKE yet. ADR 0003 and S12 place tool execution behind one
interface; the remote workspace must preserve the applicable C18-C21, S3 and
S7 contracts.

## Outcome

Agents use the existing bash, read, write, edit, find and search tools. File
and process operations reach the same remote filesystem. Agents never see
Substrate, actor identities, or lifecycle tools. Hamlet's server has no
Substrate lifecycle client or Kubernetes credentials.

A separate Rust workspace service manages the actor. One crate,
`hamlet_workspace`, contains the service and an in-actor worker.
The worker performs file/process operations; it is not another control-plane
service. Keep the local workspace as the default.

Local evidence informs the implementation; live validation is required
before its isolation guarantees are accepted.

## Ownership

- Hamlet owns conversations, agents, permissions, prompts, model credentials,
  and the domain behavior of houses, skills, gifts and archives.
- The workspace service owns authentication, workspace routing, Substrate
  lifecycle and active-operation tracking. It understands files and processes,
  not agent conversations, gifts or archives.
- The worker enforces each operation's approved filesystem and network reach.
  Model-authored arguments cannot choose another workspace, actor, house, or
  permission grant.
- Substrate owns placement and checkpoint/restore mechanics.

The server's ExecutionEnv adapter is infrastructure plumbing. It must not
expose a second set of agent tools or silently fall back to host file access.

## Initial workspace model

Use one actor per hamlet as a conditional candidate: one shared project and
separate private agent houses. Use a stable workspace identity independent of
the server's absolute home path. Persist the actor's immutable UID as well as
its name. A missing actor is not permission to replace lost files with a new,
empty workspace.

Agent-visible files have one authoritative remote location. Server records,
conversations and model credentials remain in Hamlet. Prompt readers, chief
edits, gifts and archive operations must address the same files as the tools;
their domain logic stays in Hamlet. No background local-folder synchronization
or credential copying is part of this work.

Remote paths must not be represented as paths on the chief's machine. During
incremental development, unsupported house or lifecycle operations must be
explicitly unavailable rather than silently using stale local copies.

## Service contract

Define a small versioned workspace protocol for the operations existing tools
need: text and binary files, metadata, directories, mutations, command output,
status and cancellation. Preserve the existing tool-visible results, bounds,
scrubbing, output-spill behavior and error meanings. Shared wire fixtures keep
the server and the workspace aligned.

Hamlet authenticates for an authorized workspace. File/process requests do
not expose actor templates or lifecycle verbs. A first authorized operation
may cause the manager to create the workspace's actor automatically. There is
no manual provisioning script in the product path.

The manager uses the pinned Substrate control protocol directly, not kubectl
subprocesses. Request-triggered resume is already implemented by atenet; reuse
it rather than adding a competing resume scheduler. All worker requests must
be authenticated and authorized, including ones received on loopback.

Choose and record the smallest protocol needed for the first end-to-end slice
before allowing another implementation owner to change either end. Avoid a
multi-provider framework or a standalone SDK project.

## Lifecycle and recovery

Start with one manager replica and small durable manager storage. SQLite on a
dedicated persistent volume is the current proposal, not a reason to modify
Substrate's PostgreSQL schema. Prevent overlapping manager instances from
acting on the same state.

Persist workspace bindings, actor UID, lifecycle intent and enough operation
state to reconcile interruptions. Assign operation IDs before mutations.
Never automatically repeat a command or uncertain write after a timeout,
disconnect or restart. Reconcile its known outcome or return an uncertain
result with safe recovery advice.

Each operation holds the workspace active. After an idle interval, initially
five minutes, the manager may suspend it only when operations and descendant
processes are quiescent. Serialize suspension against new operations. A request
during a transition waits for the transition to resolve.

Hamlet shutdown or client disconnection never deletes the workspace. Deletion
is an explicit administrative action with actor-identity protection. Automatic
destructive retention and persistent background jobs are out of scope.

Successful suspension must preserve files across resume. A successful write is
not necessarily a completed checkpoint. Unexpected worker loss can lose changes
since the last successful snapshot; do not promise synchronous storage durability.

## Isolation evidence and outstanding requirements

The pinned runtime was exercised locally in an OCI fixture. Independent
reproduction matched all 13 expected outcomes, including failing controls:
private direct house access, shared project writes, read-only shared files,
denial of a reachable parent-loopback listener, and cleanup of detached
descendants on cancellation and normal command completion.

Default capabilities were insufficient. The candidate grants SETFCAP to the
trusted worker, creates a separate user/network namespace for denied commands,
and drops all capabilities before running untrusted code. It did not need
SYS_ADMIN or a privileged host helper in those local experiments. This does
not authorize changing a deployed actor template.

The implementation must establish:

- Filesystem reach enforced for both file helpers and arbitrary shell commands,
  including traversal, symlinks, root replacement, proc access and inherited FDs.
- A denied command cannot reach network endpoints. Transport to the workspace
  is infrastructure traffic, not an agent's network grant.
- Network-enabled commands cannot invoke unauthorized worker/control operations
  or acquire infrastructure credentials.
- Bounded output, actor resource limits and cancellation of all descendants.
- A completion barrier before reporting that a process tree is gone. A delayed
  marker test is not proof of zero post-completion writes; the inspected gVisor
  source has an explicit task-teardown waiting TODO.
- Actual GKE networking, durable mounts and suspend/resume preserve these rules.

Pinned upstream includes EgressPolicy and credential injection. Google's GKE
documentation lists those capabilities as unsupported. Our deployment has an
egress gateway configured, but enforcement has not been established. Actor-wide
egress also cannot replace per-command grants for agents sharing an actor.

If a required boundary cannot be enforced, fail closed and report the blocked
contract. Do not silently relabel the remote workspace safe or replace isolation with
agent naming conventions. Any weaker trust model or different actor layout
needs a chief decision.

## Acceptance and delivery

The first slice is a real Hamlet Harness with a faux model calling existing
tools through a real workspace service and worker: write, execute/read, edit,
and read back the result. Run locally first using disposable data and the same
worker implementation intended for the actor. This is not evidence of GKE
behavior until the approved live run succeeds.

Extend the existing ExecutionEnv/tool behavior tests. Keep filesystem and
execution real; doubles may stand in for the external Substrate control/router
protocol in offline tests. Test through public interfaces, not private call
sequences. Live GKE tests remain explicit opt-in and must not call a model API.

Subsequent slices cover isolation, lifecycle recovery, consistent house file
consumers, archive/restore/moves, and final integrated acceptance. Evidence must
distinguish local, simulated control-plane, and actual GKE results.

The coordinator owns plan changes, integration and live cloud actions. One
implementation owner builds the first vertical slice. At most two disjoint
owners work in parallel once the protocol is settled. Existing prototype work
is preserved as reference, not extended as the target interface.

## Out of scope

- Deploying the Hamlet server itself to GKE.
- Multiple manager replicas, multiple regions, node autoscaling or new clusters.
- A general multi-provider framework, automatic local-folder synchronization,
  or automatic git/model/cloud credential injection.
- Persistent background jobs, automatic destructive retention, or new agent tools.
- Rewriting houses, skills, gifts or archives as workspace-service concepts.

## Sources

- [ADR 0003](../adr/0003-agents-and-their-tools-run-apart.md)
- [Core requirements](../spec/core.md) and [server requirements](../spec/server.md)
- [Pinned Substrate](https://github.com/agent-substrate/substrate/tree/756c2a53741121e728f4cc3066c8a19e575b4919)
- [GKE limitations](https://docs.cloud.google.com/kubernetes-engine/ai-ml/about-agent-substrate?hl=en)
