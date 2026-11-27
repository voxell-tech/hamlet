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

The `workspace` actor in `hamlet_actors` runs every agent's commands and
file changes (see [the tour](tour.md#the-workspace-actor)). It builds each policy from the reach the realm set, with
`placement` in `workspace/reach.rs`, and admits each call with `gate`
there. The images saved with a line are in the agent's own filesystem,
under `inbox/images/`, and the files under `inbox/`, so an agent reads only
what was given to it. What another agent sends with `tell` or `ask` lands
under `inbox/from-<sender>/`, copied from the realm's store. The realm's
store, `actors/realm/local.files/`, is never in reach.

Without a folder, commands write in the agent's own filesystem and a
private temporary space. They run the system's programs and Homebrew's
(`/opt/homebrew` on macOS, `/home/linuxbrew/.linuxbrew` on Linux). Other
agents' filesystems, credentials and the actors' files are not readable.
Network use is separately gated by the settings and permissions. A shell
command that needs the network while it is off can wait for the chief's
grant. An approval does not remove filesystem confinement.

## A folder a hamlet works on

The chief chooses a local folder for a hamlet. Its agents then work there,
with their own memory, skills and inbox still in their own filesystems.
Relative paths go to the folder, except those whose first part is in the
agent's filesystem (`memory/`, `skills/`, `inbox/`), which go there even
while the current directory is the folder.

Manual mode asks before edits to the folder and commands; reading stays
available. Auto mode allows work without those waits but does not remove
the sandbox. With a folder, commands can read the machine's ordinary files
and write the folder and the agent's filesystem; the realm's folder is
hidden.

Agents use the chief's own git and machine credentials in that folder.
Network still needs permission. Agents can write git hooks and config in the
folder, including nested repositories, and those can later run under the
chief's own git (C19, H11, ADR 0007).

The window sends requests and owns no confinement policy.

## File operations and leftovers

`hamlet_sandbox`'s `Env` anchors file operations to opened directory
descriptors and rejects unsafe traversal and replacement. The workspace's
`files/read.rs`, `files/walk.rs` and `files/change.rs` use it. Shell
commands use the platform process sandbox, with bounded output, time and
resources; cancellation kills their process group. A command interrupted
around an effect is not automatically replayed as a safe transaction.

What a command leaves running in its process group when it ends, its
leftovers, keeps running under Seatbelt. The workspace keeps them per agent
(`AllLeftovers`) and kills them when it stops. A process that calls `setsid`
or joins another group is not among them. Under bubblewrap,
`--die-with-parent` ends them with their command, since the sandbox's init
dies with it. A server that crashes leaves Seatbelt's leftovers running.

These boundaries differ from running the entire server in a sandbox: the
server itself owns the credentials and the actors' files, and only exposes
the confined operations it implements.

## The workspace actor

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
locks the path. In manual mode a change waits for a chief when its
real path, every link followed, lies in the project folder's real
path. The realm's folder is kept as its real path, so one reached
through a link is hidden as one reached directly.
A change in an agent's filesystem is always refused, and a read there
unless the policy reads it, as a worker reads its starter's.

A tool added by hand (`workspace/tools.rs`, C17) runs under the policy
of its manifest's `sandbox`: `fs_ro` and `fs_rw`, resolved from the
tool's folder; its own folder readable; `tools/` and its own folder
locked; `base_env` with the manifest's `env`; and its `limits`
(`wall_secs`, `cpu_secs`), which `check_limits` bounds. It works in its
own folder, and its `net` holds only when the gate gave it the network.

Commands run through `Env::private` and `Env::stream`. The file tools run on
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
  so the server runs only in insecure mode, below.

## Insecure mode on Windows

A Windows server starts only with `--insecure` (ADR 0008).
`Sandbox::open` turns that `Isolation` into the `Kind::Insecure` that only a
Windows build declares, and refuses `Isolation::Sandboxed` with words that
name the flag. Linux and macOS refuse the flag before the server opens its
realm.

In insecure mode, commands run with the server account's full reach, in the
agent's filesystem as their working folder when the hamlet has no folder. The built-in file tools keep their reach,
because they walk the policy themselves. Nothing enforces the network switch:
it decides only which commands wait for a chief. The prompt says commands are
not confined, and the policy hash names `insecure`. Each command runs in a Job
Object that holds its CPU time and process count, so a timeout or a leftover's
end reaches every process it started. No job limit bounds a file's size.

Every OS runs a command as `bash -c` with its script. On Windows, `bash` is
found by name as Windows finds programs, so commands run only where a
`bash`, such as Git Bash's, is found. No `powershell` tool is offered.

## Remote workspaces

Settings can name a workspace URL and token, kept in `credentials.json`.
At a chief's word, `move-lodges` copies the filesystem of every agent that
has not left and is not a worker to that workspace (`front/answers.rs` and
`workspace/lodges.rs` in `hamlet_server`): a push makes the remote `/lodge`
hold what the server's copy holds. The agents' tools run in the local
workspace actor; running them remotely is in
[the proposal](proposals/actor-runtime.md).

The worker's configuration and deployment notes are in
[`crates/hamlet_workspace/README.md`](../crates/hamlet_workspace/README.md).
