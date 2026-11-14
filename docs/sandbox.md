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
realm's folder (never read) and what is only read, the library, the tools
added by hand. The images saved with a line are in the agent's own lodge, under
`inbox/images/`, and the files under `inbox/`, so an agent reads only what was
given to it; what another agent sends with `tell` or `ask` lands under
`inbox/from-<sender>/`, read from the sender's own reach without following
links. The realm's `images/` and `files/` folders are never in reach. The pi
harness has no host-tool implementation; it submits operations to the
server (see [the harness](harness.md)). `oversight/gate.rs` decides admission and `calls.rs` checks permissions at execution, including
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

## The workspace actor

The `workspace` actor in `hamlet_actors`, which the server does not use
(see [the tour](tour.md#the-workspace-actor)), builds its policies
itself from the reach the realm set, with `placement` in
`workspace/reach.rs`. It uses `hamlet_sandbox`'s `Policy`, `Env` and
`scrub`, as the server does.

An agent's own place is its filesystem, `actors/agent/<n>.fs/`, or a
worker's scratch folder, `workers/<n>/`. Every policy reads the realm's
`library/` and `tools/` and the folder of the agent's spilled output,
and a worker's also reads its starter's filesystem (a chief's worker has
no starter). The folder its hamlet works on is writable. The own place
is writable for a command, and for a worker's file tools too; an
agent's file tools never write its filesystem. With a folder, the
policy is `open`: the machine is readable but the realm's folder, which
is `hidden`, and the environment is the chief's own (`host_env`), so the
project's tools are found; the call works in the folder. A folder
that holds the realm's folder, at any depth, keeps it hidden: of the
roots that hold a path, the deepest decides, so the realm's folder
inside the project stays hidden and the own place, `library/` and
`tools/` inside the realm's folder stay granted. A folder that is the
realm's folder or lies inside it, also through a link, is refused with
`ProjectInRealm`, and nothing runs. Without one,
only the listed paths are reached, the environment is `base_env` with
`HOME` at the own place, and the call works there. `library/` and
`tools/` are also `locked`, so no write root inside them makes them
writable (C18). `net` is on only for a command the gate gave
the network.

The gate reads the same policies, through `Policy::can_read` and
`Policy::can_write`, where the path's links lead: a read passes where the
file tools' policy reads, and a change where it writes and nothing
locks the path.
A change in an agent's filesystem is always refused, and a read there
unless the policy reads it, as a worker reads its starter's.

A tool added by hand (`workspace/tools.rs`, C17) runs under the policy
of its manifest's `sandbox`: `fs_ro` and `fs_rw`, resolved from the
tool's folder; its own folder readable; `tools/` and its own folder
locked; `base_env` with the manifest's `env`; and its `limits`
(`wall_secs`, `cpu_secs`), which `check_limits` bounds. It works in its
own folder, and its `net` holds only when the gate gave it the network.

Commands run through `Env::private` and `Env::stream`, which take the
realm's secrets out of the output as it comes. The file tools run on
`Env::new` in the host's process, so their reach is the `Env`'s own
walk from opened folders, not bubblewrap's or Seatbelt's. A change reads
the file with `Env::read_for_change` and replaces it with
`Env::replace_file`: the new content goes to a new file beside it, which
is renamed over it only while the path leads where it led and the file
holds what was read.

What holds on each OS:

- **Linux**: bubblewrap. Its `--die-with-parent` ends the sandbox when
  the thread that started it ends, since the parent-death signal
  follows the thread. So each command runs on a thread of its own,
  which lives until the command ends, and a command dies with its host,
  a kill included.
- **macOS**: Seatbelt, which has no parent-death rule. A host that
  stops ends its commands: the stop drops each command's stream, which
  kills its process group, and the workspace ends what commands left
  running when it is dropped and when an agent's reach ends. A command,
  and what it left running, can outlive a killed host. Its row still
  reads cut off at the next start.
- **Windows**: no sandbox. `Sandbox::open(Isolation::Sandboxed)` fails,
  so the demo gives the workspace no sandbox, and the workspace does
  not start.

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

On Windows the server offers a `powershell` tool, which runs PowerShell 7
(`pwsh.exe` on `PATH`) or else Windows PowerShell 5.1, and a `bash` tool when
Git Bash is installed. `bash` runs the `bin\bash.exe` beside the `cmd\git.exe`
on `PATH`, or in the folders Git for Windows installs to. A search by name
would start `System32\bash.exe`, the WSL launcher, so the server never
searches by name and refuses anything under `%SystemRoot%`. Without Git Bash
the server offers only `powershell`. On Linux and macOS it offers `bash`.

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
[`crates/hamlet_workspace/README.md`](../crates/hamlet_workspace/README.md).
