# What an agent's tools can reach

## The process sandbox

`crates/hamlet_server/src/sandbox.rs` and `sandbox/` own tool confinement:
bubblewrap on Linux, Seatbelt on macOS. Startup probes the sandbox and
refuses to serve if it cannot run. There is no `--insecure` mode and no
Windows server sandbox. Linux needs `bwrap` and a kernel/host policy that
allows its namespaces; installing the executable alone is not sufficient.

`tools::environment` builds the policy from the server-owned `Place`:
the house, optional project folder, shared library and hand tools.
The pi harness has no host-tool implementation; it submits operations to
the server. `gate.rs` decides admission and `chats/calls.rs` checks
permissions at execution, including after approval or replay.

Without a project folder, commands write in their own house and private
temporary space. Other houses, credentials and stores are not readable.
Network use is separately gated by the settings and permissions. A shell
request that needs network while disabled can wait for the chief's grant.
An approval does not remove filesystem confinement.

## A folder a hamlet works on

The chief chooses a local folder for a hamlet. Its agents then work there,
with their own notes, skills and inbox still in their houses. The server's
documents retain the folder and earlier choices. Tools resolve house
notes and skills to the house even while the current directory is the
project.

Manual mode asks before project edits and commands; reading and house
edits remain available. Auto mode allows work without those approval
waits, but does not remove the sandbox. With a project folder, commands
can read the machine's ordinary files and write the project and house;
the Hamlet home is hidden except for specifically granted paths.

Agents use the chief's own git and machine credentials in that project
context. Network still needs permission. Agents can write git hooks and
config in the project, including nested repositories, and those can later
run under the chief's own git (C19, H11, ADR 0007).

The common's workbench shows the chosen folder. Its clockwork shows manual
or auto mode; selecting them opens the folder panel or changes the mode.
The window sends requests and owns no confinement policy.

## File operations and house readers

`sandbox/env.rs` anchors file operations to opened directory descriptors
and rejects unsafe traversal and replacement. `tools/files.rs`, `search.rs`
and `find.rs` use that environment. Shell commands use the platform process
sandbox, with bounded output, time and resources; cancellation kills their
process group. A command interrupted around an effect is not automatically
replayed as a safe transaction.

Server-owned house operations use `papers::Houses`, `papers/tree.rs` and
`gifts/tree.rs`. They open relative to a retained home or house descriptor
with no-follow checks, reject special files where regular files are
required, and bound tree sizes. Notes, prompt shelves and gift delivery do
not use a persistent Node file-helper process. The skill shelf reads
bounded `SKILL.md` papers and renders commands starting in the house.

These boundaries differ from running the entire server in a sandbox:
the server itself owns the credentials and databases, and only exposes
the confined operations it implements. Credentials are scrubbed before
tool output or provider text is persisted or sent to clients.

## Gifts

The chief sends a manifest and parts through [the client protocol](protocol.md).
`gifts::Uploads` owns unfinished uploads by connection; an upload ID from
another connection is refused. The manifest is checked before staging:
at most 32 MiB and 4096 files, no unsafe or duplicate normalized paths,
and no file/directory conflicts. A connection has at most three pending
gifts; an agent has at most 32 MiB reserved across uploads.

Staging stays under the home, outside the houses. Disconnect or 60 seconds
of inactivity drops an unfinished upload. Startup clears stale gift
staging. Descriptor-relative delivery creates destinations without
clobbering existing files or following inbox links. On completion, the
server admits the chief's notification as an input to the receiving agent.

Skill gifts copy bounded regular-file trees from the giver's house or
embedded starters. The checked copy preserves file modes. The resolved
giver's name is recorded; typed `/give` and structured requests use the
same owner. Gift transport and the agent's subsequent model work are
separate: a complete delivered file is not undone by stopping its turn.

## Remote workspaces

Settings chooses a workspace URL and token for the next server start.
The running server keeps its startup-selected connection; an environment
override can select it for one run. `Houses` in `papers.rs` chooses the
backend once, and every house read, house write and tool call goes through
it. Remote execution never falls back to host files or commands, so
hand-added tools are refused there, and operations a remote workspace
cannot do (founding, moving, removing, restoring or erasing a hamlet,
erasing an archived house, moving a house) are refused as capabilities of
the backend. The server skips its local sandbox probe only when a remote
workspace is selected.

`workspace.rs` sends authenticated workspace-v1 requests with the agent,
project, network and read-only policy; the requests and their replies are
typed once in `hamlet_workspace_wire`. Tools see virtual `/house` and
`/project` paths. A tool call reads the shared library once and names it
by digest; the worker asks for its files only when it does not hold that
snapshot. The worker refuses a link anywhere in a no-follow path, walks a
folder for `search` and `find` in one request, and sets folder modes with
its own operation. Transfers are limited to 4 MiB per file. Interrupted
requests are cancelled by request ID; an uncertain result asks the chief
to inspect before retrying, and is not automatically replayed.

Houses can be copied between local and remote backends, preserving regular
files, modes and empty directories while skipping links. Copying does not
switch the active backend: restart afterward, and avoid changing files in
the old backend between copy and restart. Copies, gifts and house moves
write a tree the same way on both backends: each path must be free, what
was written is taken back if a later write fails, and an uncertain write is
reported rather than undone.

House work runs outside the service dispatcher on both backends. Requests
on one connection retain their order; other connections, disconnect
cleanup and shutdown remain independent of a held response. Prompt and
permission preparations are checked against current state before use.
Merges and restorations share a home-wide lane and apply their completions
to the current record.

A merge or restoration records its batch before changing files. Locally
every change is known to have happened or not, so a batch a crash cut
short is finished at the next start. A remote one records its attempt
first: if a reply is lost, the server keeps its record, plan and snapshot
and does not replay the batch on ticks or restart. Earlier files may
already have changed even when a later request is definitely refused.
Unknown effects also prevent restoration: inspect the remote houses
through the workspace manager and the preserved record and plan. There is
no safe resume or clear command for an unconfirmed batch.

`hamlet_workspace` supplies the separate Linux manager/worker. Its APIs,
configuration, restrictions and experimental deployment notes
are in [`crates/hamlet_workspace/README.md`](../crates/hamlet_workspace/README.md),
with the design in [the proposal](proposals/remote-workspaces.md).
