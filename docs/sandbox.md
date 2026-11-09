# What an agent's tools can reach

## The process sandbox

The crate `hamlet_sandbox` owns tool confinement: bubblewrap on Linux,
Seatbelt on macOS. It holds the `Policy` of a command (what it may read,
write and reach), the `Env` that carries out file operations and commands
under it, the `Leftovers` a command leaves running, and `scrub`, which takes
secrets out of what commands give back (C32). Startup probes the sandbox and
refuses to serve if it cannot run. Windows has no sandbox, so a Windows
server runs only in insecure mode, below. Linux needs `bwrap` and a
kernel/host policy that allows its namespaces; installing the executable
alone is not sufficient.

`work/tools.rs` builds the policy from the server-owned `Place`: the agent's
lodge (or a worker's scratch folder), the folder its hamlet works on, the
realm's folder (never read) and what is only read, the library and the tools
added by hand. The pi harness has no host-tool implementation; it submits
operations to the server (see [the harness](harness.md)). `oversight/gate.rs`
decides admission and `calls.rs` checks permissions at execution, including
after approval or replay.

Without a folder, commands write in their own lodge and a private temporary
space. They run the system's programs and Homebrew's (`/opt/homebrew` on
macOS, `/home/linuxbrew/.linuxbrew` on Linux). Other lodges, credentials and
the database are not readable. Network
use is separately gated by the settings and permissions. A shell request that
needs the network while it is off can wait for the chief's grant. An approval
does not remove filesystem confinement.

## A folder a hamlet works on

The chief chooses a local folder for a hamlet. Its agents then work there,
with their own memory, skills and inbox still in their lodges. Relative
paths go to the folder, except those whose first part is in the lodge
(`memory/`, `skills/`, `inbox/`), which go to the lodge even while the current
directory is the folder.

Manual mode asks before edits to the folder and commands; reading and lodge
edits remain available. Auto mode allows work without those waits but does
not remove the sandbox. With a folder, commands can read the machine's
ordinary files and write the folder and the lodge; the realm's folder is
hidden.

Agents use the chief's own git and machine credentials in that folder.
Network still needs permission. Agents can write git hooks and config in the
folder, including nested repositories, and those can later run under the
chief's own git (C19, H11, ADR 0007).

The window sends requests and owns no confinement policy.

## File operations and leftovers

`hamlet_sandbox`'s `Env` anchors file operations to opened directory
descriptors and rejects unsafe traversal and replacement. The server's
`work/tools/files.rs`, `search.rs` and `find.rs` use it. Shell commands use
the platform process sandbox, with bounded output, time and resources;
cancellation kills their process group. A command interrupted around an
effect is not automatically replayed as a safe transaction.

What a command leaves running in its process group when it ends, its
leftovers, keeps running under Seatbelt. The server keeps them per agent
(`AllLeftovers`) and kills them when it stops. A process that calls `setsid`
or joins another group is not among them. Under bubblewrap,
`--die-with-parent` ends them with their command, since the sandbox's init
dies with it. A server that crashes, or exits on a second signal, leaves
Seatbelt's leftovers running.

These boundaries differ from running the entire server in a sandbox: the
server itself owns the credentials and the database, and only exposes the
confined operations it implements. Credentials are scrubbed before tool
output or provider text is persisted or sent to clients.

## Gifts

The chief sends a manifest and parts through [the client protocol](protocol.md).
`gifts.rs` owns unfinished uploads by connection; an upload ID from another
connection is refused. The manifest is checked before staging: at most 32 MiB
and 1000 files, no unsafe or duplicate normalized paths. Staging is under the
realm's `staging/` folder, outside the lodges. Files are delivered to the
agent's `inbox/`, which is never committed, and the server then admits the
chief's notification as an input to the receiving agent.

A skill gift is a commit to a scholar's `main`: the skill comes from
another scholar or a hire's lodge, and the chief is the author. Typed `/give` and structured requests use the same owner. Gift
transport and the agent's subsequent model work are separate: a complete
delivered file is not undone by stopping its turn.

## Insecure mode on Windows

A Windows server starts only with `--insecure` (ADR 0008).
`Sandbox::open` turns that `Isolation` into the `Kind::Insecure` that only a
Windows build declares, and refuses `Isolation::Sandboxed` with words that
name the flag. Linux and macOS refuse the flag before the server opens its
realm. The server prints a warning banner before it serves.

In insecure mode, commands run with the server account's full reach, in the
lodge as their working folder. The built-in file tools keep their reach,
because they walk the policy themselves. Nothing enforces the network switch:
it decides only which commands wait for a chief. The prompt says commands are
not confined, and the policy hash names `insecure`. Each command runs in a Job
Object that holds its CPU time and process count, so a timeout or a leftover's
end reaches every process it started. No job limit bounds a file's size, so
each result names `max_file_mb` in `details.degraded`.

The `bash` tool runs Git Bash: the `bin\bash.exe` beside the `cmd\git.exe` on
`PATH`, or in the folders Git for Windows installs to. A search by name would
start `System32\bash.exe`, the WSL launcher, so the server never searches by
name and refuses anything under `%SystemRoot%`. Without Git Bash the server
offers no shell tool.

## Remote workspaces

Settings can name a workspace URL and token (kept in `credentials.json`). The
server then runs the agents' tools on that machine: `workspace.rs`
and `workspace/` in `hamlet_server` send authenticated requests to the worker, which gives each agent a `/lodge`,
its hamlet's `/project` and the library at `/library`. The requests and their
replies are typed once in `hamlet_workspace_wire`, and `hamlet_workspace`
supplies the Linux worker. The server's lodge holds what git keeps; the remote
`/lodge` is where the tools write during a turn, so the two are kept in step
(`workspace/lodges.rs`) against the files they last held alike: a push sends
what the server changed since, after the server changed it, and a pull brings
home what the remote copy changed since, after a tool may have written it.
Neither undoes the other's changes. A lodge not in step since the server
started is pushed whole before its first remote call, so the server's copy
stands. Moving every lodge to the workspace happens at a chief's word. A
dream works on the server's checkout of its scholar, never remotely. Remote
execution never falls back to the host's files or commands.

The worker's configuration and deployment notes are in
[`crates/hamlet_workspace/README.md`](../crates/hamlet_workspace/README.md),
and what remains to build in [the proposal](proposals/remote-workspaces.md).
