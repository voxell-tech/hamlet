# What an agent's tools can reach

## The process sandbox

The crate `hamlet_sandbox` owns tool confinement: bubblewrap on Linux,
Seatbelt on macOS. It holds the `Policy` of a command, the `Env` that
carries out file operations and commands under it, the `Leftovers` a
command leaves running, and `scrub`, which takes secrets out of what
commands give back (C32). Startup probes the sandbox and refuses to serve
if it cannot run. Linux needs `bwrap` and a kernel and host policy that
allows its namespaces; the executable alone is not enough.

`tools/builtin.rs` builds the policy from the server-owned `Place`: the
agent's worktree (or a worker's scratch folder), the folder its hamlet
works on, the realm's folder (never read), and what is only read: the
library and the tools added by hand. An MCP server runs under the policy
its entry in `mcp/servers.json` declares, writing its own folder; see
[the MCP servers](mcp.md). Images and files saved with a line
are in the agent's own worktree under `inbox/`, so an agent reads only
what was given to it. What another agent sends lands under
`inbox/from-<sender>/`, read from the sender's own reach without
following links. The realm's `images/` and `files/` folders are never in
reach. The pi harness has no host-tool implementation; it submits
operations to the server (see [the harness](harness.md)).
`society/oversight/gate.rs` decides admission and `calls/execution.rs`
checks permissions at execution, including after approval or replay.

Commands read the machine's files but the realm's folder, as Codex's and
Claude Code's sandboxes let theirs, and run with the chief's own `PATH`
and `HOME`, so the chief's programs and toolchains are found. They write
their own worktree, the folder when the hamlet has one, and a private
temporary space. Other worktrees, credentials and the database are in
the realm's folder, and not readable. On macOS they also reach the
system's logging, trust, preferences, directory and audio services, and
shared surfaces, as those sandboxes' commands do; neither opens a GPU
device, and nor does this. Network use is
gated separately by the settings and permissions; a shell request that
needs the network while it is off can wait for the chief's grant. An
approval does not remove filesystem confinement.

## Terminals

`crates/hamlet_server/src/terminals/shell.rs` starts shells on
pseudo-terminals. A terminal a person opens is the chief's own login
shell, run by the server's account with no sandbox, as any terminal
emulator's is. Only the chief opens one; no agent tool reaches it, and
nothing the chief types is gated. Agents have no shells of their own:
each `bash` call runs under the sandbox, and the agent's commands
terminal only shows it, read-only. The chief's shell always runs on the
server's machine, whichever workspace the agents use.

## A folder a hamlet works on

The chief chooses a local folder for a hamlet. Its agents then work
there, with their own memory, skills and inbox still in their worktrees:
relative paths go to the folder, except those whose first part is
`memory/`, `skills/` or `inbox/`.

Manual mode asks before edits to the folder and commands; reading and
worktree edits stay available. Auto mode allows work without those waits
but does not remove the sandbox. With a folder, commands also write the
folder.

Agents use the chief's own git and machine credentials in that folder,
and network still needs permission. Agents can write git hooks and config
in the folder, including nested repositories, and those can later run
under the chief's own git (C19, H11, ADR 0007).

## File operations and leftovers

`Env` anchors file operations to opened directory descriptors and rejects
unsafe traversal and replacement; the server's `tools/builtin/files.rs`,
`search.rs` and `find.rs` use it. Shell commands use the platform process
sandbox, with bounded output, time and resources; cancellation kills
their process group. A command interrupted around an effect is not
replayed as a safe transaction.

What a command leaves running in its process group when it ends keeps
running under Seatbelt. The server keeps those per agent (`AllLeftovers`)
and kills them when it stops. A process that calls `setsid` or joins
another group is not among them. Under bubblewrap, `--die-with-parent`
ends them with their command. A server that crashes, or exits on a second
signal, leaves Seatbelt's leftovers running.

The server owns the credentials and the database and exposes only the
confined operations it implements. Credentials are scrubbed before tool
output or provider text is persisted or sent to clients.

## Insecure mode on Windows

Windows has no sandbox, so a Windows server starts only with `--insecure`
(ADR 0008). `Sandbox::open` turns that into the `Kind::Insecure` that
only a Windows build declares, and refuses `Isolation::Sandboxed`. Linux
and macOS refuse the flag before the server opens its realm. The server
prints a warning banner before it serves.

In insecure mode, commands run with the server account's full reach, in
the worktree as their working folder. The built-in file tools keep their
reach, because they walk the policy themselves. Nothing enforces the
network switch: it decides only which commands wait for a chief. The
prompt says commands are not confined, and the policy hash names
`insecure`. Each command runs in a Job Object that holds its CPU time and
process count, so a timeout or a leftover's end reaches every process it
started. No job limit bounds a file's size, so each result names
`max_file_mb` in `details.degraded`.

On Windows the server offers `powershell`, and `bash` when Git Bash is
installed. `bash` runs the `bin\bash.exe` beside the `cmd\git.exe` on
`PATH`, or in Git for Windows' install folders; the server never
searches by name and refuses anything under `%SystemRoot%`, since
`System32\bash.exe` is the WSL launcher.

## Remote workspaces

Settings can name a workspace URL and token (kept in `credentials.json`).
The server then runs the agents' tools on that machine: `realm/workspace/`
sends authenticated requests to the worker (`hamlet_workspace`, typed in
`hamlet_workspace_wire`), which gives each agent a `/worktree`, its
hamlet's `/project` and the library at `/library`.

The server's worktree holds what git keeps; the remote `/worktree` is
where the tools write during a turn, so the two are kept in step
(`realm/workspace/worktrees.rs`). A push sends what the server changed
since the last match, and a pull brings home what the remote copy
changed; neither undoes the other's changes. A worktree not in step since
the server started is pushed whole before its first remote call. A dream
works on the server's checkout of its profile, never remotely. Remote
execution never falls back to the host's files or commands.

The worker's deployment notes are in
[`crates/hamlet_workspace/README.md`](../crates/hamlet_workspace/README.md).
