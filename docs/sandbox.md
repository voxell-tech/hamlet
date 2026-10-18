# What an agent's tools can reach

## The sandbox

Tools run in a sandbox: bubblewrap on Linux, Seatbelt on macOS. Windows has
no sandbox, and a server without one does not start; there is no
`--insecure` mode. An agent's tools can write only in its own house, and a
command that reaches past the sandbox waits in Petitions for the chief.

Tools have no network access until "Tools network access" is turned on
under Agents in Settings, or `HAMLET_NET=1` is set.

`server/src/sandbox/` holds the sandboxes, and `server/src/environment.ts`
builds the policy a command runs under. Tools added by hand reach the
paths their manifests grant, not the shell's paths.

## A folder a hamlet works on

A hamlet can work on a folder of the chief's, as a coding agent does. On
the hamlet's page, "Choose a folder" points it at one, and every agent of
that hamlet then runs its commands there and may edit its files. Nothing of
Hamlet's is put in the folder: each agent's notes, skills and logs stay in
its own house.

A hamlet starts in manual mode. Each edit and each command waits in
Petitions for the chief to allow it once, for the rest of the agent's
session, or not at all. Reading is free.

In auto mode the agents edit and run without asking, and the sandbox keeps
their writes inside the folder and their own workspace. There, a command
can read the rest of the machine, so the project's own tools run, but it
cannot read Hamlet's home. Manual mode holds folder edits and commands,
not edits of the agent's own house.

The folder is the chief's own path on the machine, with earlier folders
kept. Agents use the chief's own git and whatever credentials the machine
has, which their commands can read. Reaching a git host needs the network
like any other command (C19, H11). Agents may write git's files in the
folder, including nested repositories' hooks and config, and the chief
accepts that these may later run under the chief's own git (ADR 0007).

Both are seen on the workbench in the middle of the hamlet's common. The
bench stands for the folder: it is bare with none, and has work laid out
on it and the folder's name over it when there is one. A click on it
shows the hamlet's page. The clockwork on the bench shows the mode: its
wheels turn by themselves under a blue lamp in auto, and stand still
under an orange lamp in manual, with the crank waiting. With no folder
the lamp is dark. A click on the clockwork switches the mode.

[`tour.md`](tour.md) names the code for each of these under "The folder a
hamlet works on".

## Gifts

A chief's file gift lands in the agent's `inbox/`. Its manifest and parts
are in [protocol v0](protocol.md); these are the server's rules for them.

The server refuses more than 32 MiB in total, more than 4096 files
(including empty ones), unsafe paths, paths repeated after NFC
normalization and case-insensitive Unicode folding, file/folder conflicts,
and an absent agent. Each connection may hold at most three unfinished
gifts. Across all connections, each receiving agent may have at most
32 MiB reserved by accepted gifts, including ones being delivered. A
refusal creates no stage. The window holds none of these rules.

Bytes stay in private staging outside every house until all files are
complete. Once the last bytes have been written and the files closed, the
upload is detached from its connection, its idle timer is cleared, and
delivery begins. A disconnect before that point discards the whole upload,
waiting for any in-flight staging or write to end before removing its
stage. A disconnect after that point changes nothing: delivery and the
agent's notification finish, even though the client may not receive its
receipt. No discard races a staging step or copy. An unfinished upload
idle for 60 seconds is discarded as a disconnected one is. A new server
discards staging left by its predecessor. Upload ids cannot be used by
another connection.

Delivery runs through the receiving agent's `SandboxEnv.exec`, with only
its house writable and its stage readable. A folder target is claimed with
`mkdir --` before `cp -RP` copies its contents; a file target is claimed
with a no-clobber open. An existing target ends delivery without changing
it. The sandbox checks each write's resolved path, so a replaced inbox
link cannot direct a write outside the house. A missing inbox is
recreated. A failed copy removes only the destinations it claimed under
the same policy; staged bytes are removed after the copy ends. Failures
give words without server paths.

A skill gift records the resolved giver's name, with its hamlet when
different. A skill from another agent is copied in two sandboxed steps.
The first can read the giver's whole house, granted by its server-owned
path, and write only a private staging folder. It copies without
following links, then the server checks the staged tree for links, special
files, more than 4096 entries and more than 32 MiB. The receiving agent's
sandbox delivers the checked stage as it delivers a file gift. Shipped
starters are checked and delivered from their server-owned folder. A
missing skills folder is recreated. A 33 MiB sandbox file limit bounds a
growing source while allowing Linux `cp` to probe EOF of an exactly 32 MiB
file; the accepted gift still takes at most 32 MiB.

## Reading houses and file operations

State lists starters with shipped descriptions and each agent's skill
folder names only. State, papers and prompts share a regular-file, no-link
listing, cached for at most one second and invalidated by gifts, commits
and a turn's end. Houses are never watched recursively. House skill papers
are never opened to build state. Prompts and papers list a house's shelf
and read all its descriptions and usage in one request to a read-only
sandbox over the whole house. The reader caches papers by device, inode,
modification time, change time and size, checking those inside the sandbox
on each request. It uses a no-follow, non-blocking open, accepting only
regular files and reading at most 64 KiB each. A swapped shelf or parent
folder cannot disclose outside names or papers. The prompt's Run command
starts with `cd '<house>' &&`.

Every file operation of `SandboxEnv`, including readers, watchers, metadata
and temporary files, runs in a persistent sandboxed Node helper. The first
file operation starts it; `cleanup()` ends it. Only Node, the helper and
its runtime files are added to the policy's readable paths, and no ambient
environment is inherited. The kernel checks the path actually opened.
Environments with the same home, working directory and policy share a
helper. A dead helper is replaced on demand. Each request has at most 30
seconds, or the policy's shorter wall limit; an overrun kills the helper
and fails its in-flight requests, without retrying a possibly completed
write. A helper idle for 30 seconds exits, and readers left open then
report that they are closed. An active watch keeps its helper alive until
closed. File size and core limits still hold, but a command's CPU and
process limits do not apply to the persistent helper. Malformed frames
fail their addressed request or watch. House skill shelves have separate
read-only helpers.

## A remote workspace

Instead of the machine Hamlet runs on, an agent's ordinary tools (`bash`
and the file tools) can work in a remote workspace. There is no extra agent
tool: agents see the same tools, with paths `/house` for their own house,
`/project` for their hamlet's shared project and `/library` for shared skills.
They never see Substrate,
actors or lifecycle verbs.

The chief chooses a local or a remote workspace under Workspace in
Settings. A remote one is the address of a workspace manager and the token
its operator gave this home. One manager serves many homes, and the token
tells it which home calls, so homes never share a house even when their
hamlets have the same names. The server keeps both with its credentials
(S7) and reads them when it starts, so a change applies from its next
start. Until then, **Move houses** on the same page copies every agent's
house from the workspace the server works in to the chosen one, making each
house there an exact copy and checking it file by file. Links and special
files stay behind, and a remote workspace takes files of at most 4 MiB. The
houses stay where they were, so what agents change between the move and the
restart is not moved.

The address must be HTTPS, or plain HTTP on `127.0.0.1`, `[::1]` or
`localhost`, with no credentials or query in it, and the token at least 16
characters. `HAMLET_WORKSPACE_URL` and `HAMLET_WORKSPACE_TOKEN`, set
together, choose a workspace for one run of the server in place of the one
in Settings:

```sh
HAMLET_WORKSPACE_URL=https://MANAGER_HOST \
HAMLET_WORKSPACE_TOKEN=AT_LEAST_16_CHARACTERS \
HAMLET_HOME=/tmp/hamlet-remote hamlet server
```

The manager and the worker inside each actor are one binary,
`hamlet-workspace`. How to build, configure and run it, its keys and the
actor template are in
[`crates/hamlet_workspace/README.md`](../crates/hamlet_workspace/README.md).
The messages Hamlet sends are defined by
[`wire.rs`](../crates/hamlet_workspace/src/wire.rs), and
[`fixtures/protocol-v1.json`](../crates/hamlet_workspace/fixtures/protocol-v1.json)
holds example messages with their expected replies, which both the Rust and
the TypeScript sides are tested against. The design is in
[the proposal](proposals/remote-workspaces.md).

### What works remotely

- Commands and file reads, writes, edits, listings and searches, in the
  agent's house and, when its reach includes it, the hamlet's project.
- Each agent's house, with its notes, papers and skills, lives in the
  workspace and is read from there. Hamlet prepares it on startup and
  keeps no local copy.
- The shared project starts empty. Hamlet does not upload or synchronize
  a local folder. `/folder /project` enables it and `/folder none`
  removes it from subsequent operations' reach without deleting it.
- Chief edits and manual previews read the same remote files as tools.
  Gifts, uploads, skills, agent creation, archive and restoration, and
  same-hamlet merge and split use confined remote house operations.
  File and directory permission bits are preserved during transfers.
- Letters, questions, objections and office votes keep their normal domain
  behavior. Founding votes are refused before admission; a pending founding
  vote cannot create a hamlet after switching to remote workspaces.
- Hamlet's library remains authoritative in the server's library directory.
  Starters, librarian decisions, publication and chief removal stay in Hamlet.
  Each tool operation receives a bounded read-only snapshot at `/library`,
  so a later operation sees publications and removals. Running operations keep
  their original snapshot. No house, credential file or librarian notes are
  copied into that snapshot; links and special files are refused. Snapshot
  failure refuses the operation, with no stale or local execution fallback.
- A network grant enables HTTP and HTTPS through `HTTP_PROXY` and
  `HTTPS_PROXY`. The worker accepts public addresses on ports 80 and 443;
  direct sockets and UDP remain blocked. Substrate's egress policy further
  restricts the allowed hosts and protocols. Its gateway resolves TLS names
  again, so the worker's private-address check does not guarantee the final
  destination is public. The demo uses ordinary TLS passthrough without an
  additional gateway proxy; it is not a production network-isolation boundary.

### What does not, yet

These are refused with an explanation, and nothing falls back to the local
machine:

- Local-folder selection or synchronization, cross-hamlet moves and
  founding, whole-house erasure and hand-installed
  tools. These require capabilities beyond the mapped remote house.
- Network clients that ignore the proxy, non-HTTP protocols, and ports
  other than 80 and 443. A grant does not expose the actor's network.
- Environment overrides and raw output spill for commands.
- Running the manager or the worker anywhere but Linux. The binary built
  on other systems only reports that it is unsupported. Hamlet itself can
  connect from any system.

### How a request is handled

Authentication. Hamlet sends the bearer token to the manager. The manager
signs every message it forwards with its Ed25519 key, and the worker
accepts only a valid signature less than 180 seconds old, also on
loopback. The worker holds no secrets, and model or cloud credentials
never enter an actor.

Request ids. Hamlet gives every operation a fresh UUID. The manager
records the operation before forwarding it and the settled reply after.
An id is never executed twice: repeating one is answered `uncertain`, except
an operation cancelled before admission is always answered `aborted`.
`status` returns the recorded reply, and `cancel` asks the worker to stop
the operation. A cancellation arriving before its execution request is
recorded durably without provisioning an actor, and prevents later execution
of that id even after a manager restart. A failed or interrupted request is reported as uncertain,
with its id, and is never retried automatically, including after a
restart; inspect the files before repeating it.

Read-only. Hamlet's own reads of a house, for prompts and papers, ask for
`read_only`. The worker then mounts the house and project read-only for
that operation, for commands as well as file helpers.

Limits. A file read or write moves at most 4 MiB. A library snapshot has
at most 1024 entries and 4 MiB of file data. A request carries at most
12 MiB, including base64 file and library data; Hamlet accepts replies of
at most 6 MiB. A command's stdout and stderr together are capped at
1 MiB, its timeout at 120 seconds, its files at 32 MiB each, its open files
at 128 and its CPU time at 120 seconds. A worker runs at most 16
operations at once.

Completion. A command's result is reported only after every process it
started is gone: the supervisor kills and reaps all of them, detached ones
included, before answering. This holds in local runs; it is not yet proven
under Substrate's gVisor on GKE.

Output. Output is buffered until the command ends; nothing streams. Hamlet
then delivers all of stdout before all of stderr, so their interleaving is
lost. A command that times out, is cancelled or reaches the output cap
returns an error with none of its output, and the files it changed so far
stay changed.

### Lifecycle

The first operation for a hamlet's workspace makes the manager create its
actor. After 300 seconds without operations (`idle_seconds` in the
manager's configuration) the manager suspends the actor, and the next
operation resumes it. Outcomes that are still unknown, of operations or of
a suspension, are written down and settled by asking the worker for their
status after a restart, never by running them again.

Closing Hamlet deletes nothing in the workspace.
Deletion is an administrative command, run with the manager stopped:

```sh
hamlet-workspace delete MANAGER_CONFIG WORKSPACE EXPECTED_UID
```

It deletes the actor only if the workspace's recorded actor still has that
UID, and leaves a tombstone so the workspace is never recreated; later
requests for it are refused.

Files are only as durable as the actor's last successful snapshot. If its
worker is lost unexpectedly, files can roll back to that snapshot, even
after a reply that reported them written.

Packaging exists, but the image has not been built here, for lack of a
Docker daemon, and nothing has been run on GKE yet. The manager's own
limits and the requirements on whoever runs it are in its
[README](../crates/hamlet_workspace/README.md).
