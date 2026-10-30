# What an agent's tools can reach

## The process sandbox

`crates/hamlet_server/src/sandbox.rs` and `sandbox/` own tool confinement:
bubblewrap on Linux, Seatbelt on macOS. Startup probes the sandbox and
refuses to serve if it cannot run. Windows has no sandbox yet, so a
Windows server runs only in insecure mode, below. Linux needs `bwrap` and
a kernel/host policy that allows its namespaces; installing the
executable alone is not sufficient.

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

## Terminals

`crates/hamlet_server/src/terminals.rs` runs shells on pseudo-terminals
(`hamlet_os::process::pty`). A terminal of a hamlet runs `bash` under the
same policy as a shell tool call: an agent's with that agent's reach and
the network only when its call asked for it and the gate allowed it; one
the chief opens with the mayor's reach and the network. bubblewrap runs
it without `--new-session`, which would cut it from its terminal; the
shell has no ulimits or wall time, since it runs for as long as it is
used, and ends with its terminal. `terminal_open` and every
`terminal_write` pass the gate as a shell command does, so a chief who
is asked about commands is asked about each line an agent types. What
the chief types is not gated.

An unbounded terminal, with no hamlet, is the chief's own login shell
(`$SHELL -l` in their home folder), run by the server's account with no
sandbox, as any terminal emulator's is. Only the chief opens one; no
agent tool reaches it.

A remote workspace has no terminals yet: opening one there is refused.
Windows has none yet either.

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

What a command leaves running in its process group when it ends, its
leftovers, keeps running under Seatbelt. `papers::Houses` keeps them per
agent and kills that group when the server stops and before that agent's
house, or its whole hamlet, is erased or moved. A process that calls
`setsid` or joins another group is not among them. The kill reaches only
that group: the server leaves the command's own process unreaped until it
drops the record, and an id stays taken while its process is unreaped, so
no other group can take the group's id first. On Linux the server reads
`/proc` to tell whether the group still has a live process. On macOS it
asks `killpg`, which skips the unreaped leader. Records of ended groups are
dropped each time a command ends, which reaps their leaders.
Under bubblewrap, `--die-with-parent` ends them with their command, since
the sandbox's init dies with it. A server that crashes, or exits on a
second signal, leaves Seatbelt's leftovers running.

Server-owned house operations use `papers::Houses` and `papers/tree.rs`.
They open relative to a retained home or house descriptor
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

## Insecure mode on Windows

A Windows server starts only with `--insecure` (ADR 0008).
`Sandbox::open` turns that `Isolation` into the `Kind::Insecure` that only
a Windows build declares, and refuses `Isolation::Sandboxed` with words
that name the flag. Linux and macOS refuse the flag before the server
opens its home, and so does a server whose workspace is remote, where
commands do not run on this machine. The server prints a warning banner
before it serves.

In insecure mode, commands and hand tools run with the server account's
full reach, in the house as their working folder. The built-in file tools
keep their reach, because they walk the policy themselves. Nothing
enforces the network switch: it decides only which commands wait for a
chief. The prompt says commands are not confined, and the policy hash
names `insecure`. Each command runs in a Job Object that holds its CPU
time and process count, so a timeout or a leftover's end reaches every
process it started. No job limit bounds a file's size, so each result
names `max_file_mb` in `details.degraded`.

The `bash` tool runs Git Bash: the `bin\bash.exe` beside the `cmd\git.exe`
on `PATH`, or in the folders Git for Windows installs to. A search by name
would start `System32\bash.exe`, the WSL launcher, so the server never
searches by name and refuses anything under `%SystemRoot%`. Without Git
Bash the server offers no shell tool.

## Remote workspaces

Settings chooses a workspace URL and token for the next server start.
The running server keeps its startup-selected connection; an environment
override can select it for one run. `Houses` in `papers.rs` chooses the
workspace once, and every house read, house write and tool call goes
through it. Remote execution never falls back to host files or commands, so
hand-added tools are refused there, and operations a remote workspace
cannot do (founding, moving, removing, restoring or erasing a hamlet,
erasing an archived house, moving a house) are refused as capabilities of
the workspace. The server skips its local sandbox probe only when a remote
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

Houses can be copied between the local and a remote workspace, preserving
regular files, modes and empty directories while skipping links. Copying
does not switch the workspace in use: restart afterward, and avoid
changing files in the old workspace between copy and restart. Copies,
gifts and house moves write a tree the same way in both workspaces: each path must be free, what
was written is taken back if a later write fails, and an uncertain write is
reported rather than undone.

House work runs outside the service dispatcher in both workspaces. Requests
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
Unknown effects also prevent restoration: inspect the remote houses on
the worker's machine and the preserved record and plan. There is no safe
resume or clear command for an unconfirmed batch.

`hamlet_workspace` supplies the Linux worker the server connects to. Its
configuration, restrictions and deployment notes are in
[`crates/hamlet_workspace/README.md`](../crates/hamlet_workspace/README.md),
and what remains to build in [the proposal](proposals/remote-workspaces.md).
