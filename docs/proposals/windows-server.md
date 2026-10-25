# The server on Windows

A design for running `hamlet_server` on Windows 11 beside Linux and
macOS, with bash and PowerShell for agents and an insecure mode where
no sandbox can run. Today Windows has only the window, as a remote
client ([`releases.md`](../releases.md)). This proposal changes C18 and
C19, and ADR 0008 and ADR 0009 record its two lasting decisions. A
Windows sandbox is not part of it and is left for later.

## Decisions

- **Insecure mode** is an opt-in the chief types when starting a
  server, as `--insecure`, on every platform. Without it, a server
  whose sandbox cannot run refuses to start, as C18 says today, so a
  Windows server always needs it. With a remote workspace the flag
  refuses to start, because the worker confines commands.
- Bare `hamlet` attaches to whatever server is running, and says so
  when that server is in insecure mode. Only starting a server needs
  the flag.
- While a server is in insecure mode, the window shows a standing
  "Insecure mode" chip on every screen.
- The network rules stay as today. With the switch on, commands reach
  the network without an ask. With it off, a command that needs the
  network waits for a chief. In insecure mode nothing stops a command
  that reaches the network without saying so, and the switch notes
  that it is not enforced.
- The built-in file tools keep their limits in insecure mode. Only
  commands and hand tools lose their confinement.
- The house store keeps every guarantee on Windows, insecure mode
  included. It protects the server from what agents leave in their
  houses, and on Windows a junction or a hard link needs no privilege.
- Agents get two shell tools, `bash` and `powershell`. Linux and macOS
  offer `bash`. Windows offers `powershell` always and `bash` where
  Git for Windows is installed.
- A command's **leftovers** keep running after it ends. In every mode
  they end when the server stops, and before their agent's house is
  erased or moved.
- Windows 11 only. A Windows sandbox, most likely Microsoft's MXC
  process containers, waits for a prototype on a real machine.

## Why the port is more than a flag

Three things stop the server on Windows.

1. **Files.** The house store and the tools' file layer call rustix in
   `papers.rs`, `papers/tree.rs`, `gifts/tree.rs`, `workspace.rs` and
   `sandbox/env.rs`. rustix is a Unix-only dependency, so the server
   does not build. Those calls carry the store's guarantees: opens
   relative to a held folder, no link followed, no hard-linked file,
   regular files opened without blocking, size caps, a temp file
   renamed without replacing, masked modes. A path-based fallback, as
   `prompt/shelf.rs` has, would let an agent redirect the server.
2. **Shells.** `bash` is matched in eight places that must agree: the
   spec, dispatch, the gate, `UNSAFE`, activity, the reserved hand-tool
   names, the network words and the command line. Limits and the kill
   are `ulimit` and the process group. Off Unix a timeout kills
   nothing today.
3. **Confinement.** `Sandbox::platform()` gives Windows `Missing`, and
   `Houses::probe` refuses to start with it.

## One crate for what differs by OS

A new leaf crate, `crates/hamlet_os`, holds every call that differs by
operating system. It has no C code below it (rustix on Unix,
`windows-sys` on Windows, tokio, thiserror, uuid), so
`cargo check -p hamlet_os --target x86_64-pc-windows-gnu` runs on a
Linux machine with no C compiler. The whole server cannot, because
`ring` and `libsqlite3-sys` compile C.

```text
crates/hamlet_os/src/
  lib.rs                  pub mod fs; pub mod process;
  fs.rs                   Directory, Regular, Stat, Kind, FileId and the
                          small enums
  fs/name.rs              Name, TreePath
  fs/mode.rs              Mode, Mode::kept
  fs/path.rs              tool path text, from sandbox/path.rs
  fs/walk.rs              reach(), the tools' walk, from sandbox/env.rs
  fs/error.rs             system_error, an io::Error in Node's words
  fs/sys.rs               the private System trait
  fs/sys/unix.rs          rustix, today's flags
  fs/sys/windows.rs       the NT API; a declared unsafe module
  process.rs              Limits, Command, Tree, Leftovers, limited,
                          unenforced
  process/sys/unix.rs     the process group and the ulimit wrapper
  process/sys/windows.rs  the Job Object; a declared unsafe module
```

`rustix` leaves `hamlet_server`. The convention's "system calls go
through rustix" becomes "system calls go through `hamlet_os`", and its
list of unsafe modules gains the two Windows files.

### Files

Callers hold one concrete type. No caller writes `cfg`, and none can
reach a platform call, because the platform code is a private module of
another crate.

```rust
/// An opened directory. Every operation below it is anchored to it,
/// so renaming or replacing one of its ancestors with a link changes
/// nothing it reaches.
pub struct Directory(<Os as sys::System>::Handle);

impl Directory {
    pub fn open(path: &Path, last: LastLink) -> io::Result<Self>;
    pub fn try_clone(&self) -> io::Result<Self>;
    pub fn status(&self) -> io::Result<Stat>;
    pub fn stat(&self, name: &Name) -> io::Result<Option<Stat>>;
    pub fn child(&self, name: &Name) -> io::Result<Self>;
    pub fn make(&self, name: &Name, mode: Mode, exists: Exists)
        -> io::Result<Self>;
    pub fn parent<'a>(&self, path: &'a TreePath, missing: Missing)
        -> io::Result<(Self, &'a Name)>;
    pub fn file(&self, name: &Name, how: Open) -> io::Result<Regular>;
    pub fn set_mode(&self, mode: Mode) -> io::Result<()>;
    pub fn rename(&self, name: &Name, to: &Directory, to_name: &Name,
        replace: Replace) -> io::Result<()>;
    pub fn remove(&self, name: &Name, removing: Removing)
        -> io::Result<()>;
    pub fn names(&self) -> io::Result<Vec<Name>>;
    pub fn sync(&self) -> io::Result<()>;
}

impl Regular {
    pub fn stat(&self) -> &Stat;
    /// No other name, so no hard link carries the file elsewhere.
    pub fn is_lone(&self) -> bool;
    pub fn set_mode(&self, mode: Mode) -> io::Result<()>;
    pub fn file(&mut self) -> &mut File;
}

pub enum Kind { File, Directory, Link, Special }
pub enum Open { Read, Append, New(Mode), Truncate(Mode), Extend(Mode) }
pub enum Replace { Never, Always }
```

`Directory::open` is the one operation by path, for anchors the server
chooses: the home, a skill shelf, the library. Everything else takes a
`Name`, one entry, checked once. On Unix the rules are today's. Windows
also refuses `\`, `:`, `<>"|?*`, a trailing dot or space and the
device stems. `TreePath` refuses `\` on every machine, so a tree read
on Linux can be written on Windows and to the remote worker.

Each OS implements one private trait, so the contract and its
guarantees are written once.

```rust
/// Every call that takes a Name reaches exactly that one name
/// relative to the given handle and follows no link at it.
pub(crate) trait System {
    type Handle: Debug + Send;
    fn open_path(path: &Path, last: LastLink) -> io::Result<Self::Handle>;
    fn open_directory_at(dir: &Self::Handle, name: &Name)
        -> io::Result<Self::Handle>;
    fn open_file_at(dir: &Self::Handle, name: &Name, how: Open)
        -> io::Result<Regular>;
    fn read_link_at(dir: &Self::Handle, name: &Name) -> io::Result<String>;
    fn rename_at(from: &Self::Handle, name: &Name, to: &Self::Handle,
        to_name: &Name, replace: Replace) -> io::Result<()>;
    fn remove_at(dir: &Self::Handle, name: &Name, removing: Removing)
        -> io::Result<()>;
    // ... stat_at, make_directory_at, set_mode, names, sync_directory
}
```

A public trait would push a type parameter or a trait object into
`Files`, `Houses` and `Reader` and hide nothing more, since each build
has one implementation.

What Windows keeps of each guarantee:

| Unix | Windows | |
|---|---|---|
| `openat` one name | `NtCreateFile` with `RootDirectory`, one name | same |
| `O_NOFOLLOW` | `FILE_OPEN_REPARSE_POINT`, `OBJ_DONT_REPARSE`, a tag check on the handle | stricter, every reparse point |
| `nlink == 1` | `NumberOfLinks` from the handle | same |
| `O_NONBLOCK`, then `fstat` | `FILE_NON_DIRECTORY_FILE` and a handle check | same |
| `O_CREAT\|O_EXCL` | `FILE_CREATE` | same |
| `renameat2(NOREPLACE)` | `FileRenameInfo`, `ReplaceIfExists` false | same |
| `readlinkat` | `FSCTL_GET_REPARSE_POINT` on the unfollowed handle | same, for symbolic links and junctions |
| `unlinkat` | delete through the checked handle, POSIX semantics | no race at the last name |
| `fchmod` | `FileBasicInfo`, read-only only | weaker |
| folder `fsync` | nothing; NTFS journals metadata | weaker |
| `(dev, ino)` | volume serial and file index | new; `(0, 0)` today |

**Modes.** Windows keeps only the read-only attribute. `Mode::kept`
says what a mode reads back as: itself on Unix, and on Windows `0o644`,
`0o444` without the owner's write bit, or `0o700` for a folder.
`copy::is_same` asks the target tree through `Files::keeps`, so a gift
and a merge verify on every machine. Moving houses across machines is
not a goal, so nothing keeps exact modes on Windows.

**The tools' walk** moves into the crate with the path text, and errors
map from `io::ErrorKind`. It follows links as the system would, up to
40, and checks the real path against the policy, as on Linux and macOS
today. On Windows a link is a symbolic link or a junction, so a `read`
inside pnpm's junction-built `node_modules` works when its target is
within reach.

**Houses in use.** Windows refuses to rename or delete a folder while a
process holds a handle inside it, where Unix allows it. Before a house
is erased or moved, the server ends its agent's leftovers (see Process
trees), so that is rarely the case. If something else still holds the
house, the deed fails and nothing changes.

### Process trees

One call starts a command's tree inside its limits.

```rust
pub struct Limits { pub cpu_secs: u64, pub wall_secs: u64,
                    pub max_procs: u64, pub max_file_mb: u64 }

impl Tree {
    /// Unix: a new process group. Windows: started suspended, put in
    /// a Job Object holding cpu_secs, max_procs and no crash dialogs,
    /// then resumed, so nothing it starts escapes the job.
    pub fn spawn(command: Command<'_>, limits: &Limits)
        -> io::Result<(Tree, tokio::process::Child)>;
    /// Everything in the tree, now: SIGKILL to the group, or
    /// TerminateJobObject.
    pub fn kill(&mut self);
    /// The command ended. What it left running keeps running, and
    /// comes back as its leftovers, to end later.
    pub fn release(self) -> Leftovers;
}

/// What one command left running: its process group, or its job.
pub struct Leftovers { .. }
impl Leftovers {
    pub fn end(self);
}

/// Unix: today's ulimit wrapper, which runs inside the sandbox.
/// Windows: the argv unchanged.
pub fn limited(limits: &Limits, argv: &[String]) -> Vec<String>;

/// The limits this OS cannot hold: max_file_mb on Windows.
pub fn unenforced(limits: &Limits) -> &'static [&'static str];

/// Windows: a kill-on-close job the server joins at startup, so its
/// leftovers end even when it crashes. Unix: nothing.
pub fn reap_with_server() -> io::Result<Reaper>;
```

The server keeps each agent's `Leftovers` and ends them at two points,
on every platform and in every mode: when the server stops, and before
that agent's house is erased or moved. Under bubblewrap they already
die with the server through `--die-with-parent`. Under Seatbelt and in
insecure mode on Unix they do not today. On Windows the server also
joins a kill-on-close job at startup, so its leftovers end even when it
crashes. On Unix a crashed server cannot end them.

`Env::spawned` builds the command line in three steps: the shell's
argv, `process::limited`, then `Sandbox::command`. A
`disallowed-methods` entry for `Command::new` in a new
`crates/hamlet_server/clippy.toml` keeps every other spawn out of the
server. It repeats the root file's two settings, because clippy reads
only the nearest one. pi's start, the sandbox probe and one test spawn
carry an `expect` with a reason.

A result whose limits did not all hold names them in
`details.degraded`.

Hand tools stop building the quoted `exec '<a>' '<b>' < '<args.json>'`
script. They spawn `manifest.exec` as an argv with the arguments file
as stdin. On Unix the `ulimit` wrapper still starts them.

## Shells

Each shell is its own tool, so the model always knows which language
it writes. The dialects differ in quoting, chaining, exit codes and
what `>` writes. `bash` keeps its name, because kept calls, transcripts
and recorded frames use it.

```rust
pub(crate) enum Dialect { Bash, PowerShell }

impl Dialect {
    pub(crate) const ALL: [Self; 2] = [Self::Bash, Self::PowerShell];
    pub(crate) fn tool(self) -> &'static str;
    pub(crate) fn of_tool(name: &str) -> Option<Self>;
    pub(crate) fn network_words(self) -> &'static [&'static str];
    pub(crate) fn line_hint(self, path: &str, line: usize, from: usize)
        -> String;
}

/// pwsh.exe (7) or powershell.exe (5.1, no && or ||).
pub(crate) enum Edition { Seven, Windows }
pub(crate) struct Shell { dialect: Dialect, program: Program,
                          edition: Option<Edition> }
/// What a workspace offers, found once when it opens.
pub(crate) struct Shells(Vec<Shell>);

impl Shell {
    pub(crate) fn spec(&self) -> ToolSpec;
    pub(crate) fn invocation(&self, script: &str, tmp: &Directory)
        -> io::Result<Vec<String>>;
}
```

Every list that held the word `bash` derives from `Dialect`:

| Today | After |
|---|---|
| `"bash" =>` in `gate.rs` | `Dialect::of_tool` gives `Call::Shell` |
| `NETWORK` in `gate.rs` | a common list plus `network_words` |
| `UNSAFE` in `chats/calls.rs` | `shell::runs_once`, from each spec's replay |
| `"bash" \| "powershell"` in `chats/activity.rs` | `Dialect::of_tool` |
| `"powershell"` in `hand.rs` | `tools::reserved_names`, from `Dialect::ALL` |
| `["bash", "-c", cmd]` in `env.rs` and `environment.rs` | `Shell::invocation` |
| `spec.name === "bash"` in pi | `ToolSpec.retain` |
| the `sed` hint in `tools/files.rs` | `Dialect::line_hint` |

A test fails when `"bash"` or `"powershell"` appears as a string
literal in `hamlet_server/src` outside `tools/shell.rs` and tests.
`bash::run` and `bash::remote` become one `shell::run` over `Reach`,
which gains `exec`, so files and processes go through one interface as
S12 says.

**Finding shells.** The server finds each shell's path once, never by
`Command::new("bash")`, whose search puts `System32` first and would
start the WSL launcher. PowerShell is `pwsh.exe` on `PATH`, else the
5.1 every Windows has, and the tool's description names the one
found. Git Bash is the `bin\bash.exe` beside `cmd\git.exe`, or in the
default folders. Anything under `%SystemRoot%` is refused. The search
reads the machine through a small `Surroundings` trait, so it is
tested on Linux against a fake disk.

**Command lines.** Bash is `[program, "-c", script]`, and Git Bash adds
`MSYS_NO_PATHCONV=1`. PowerShell is `-NoLogo -NoProfile
-NonInteractive -ExecutionPolicy Bypass -EncodedCommand` over UTF-16LE
base64 of a preamble that sets UTF-8 both ways, the script, and an
epilogue that keeps a native command's exit code. A Windows command
line holds 32,767 characters, about 12,000 characters of script. A
longer script is written with the preamble and epilogue to a `.ps1`
file in the command's private temp folder and run with `-File`, so the
agent never meets the limit.

## Confinement

```rust
/// What the chief asked for. Only serve::Options::parse makes Insecure.
pub enum Isolation { Sandboxed, Insecure }

pub struct Sandbox(Kind);
enum Kind { Bwrap, Seatbelt, Insecure }

impl Sandbox {
    /// The only constructor. Sandboxed probes this platform's sandbox
    /// or fails with "start the server with --insecure". Kind::Insecure
    /// comes from Isolation::Insecure and from nothing else.
    pub fn open(isolation: Isolation) -> Result<Self, NoSandbox>;
    pub fn confinement(&self) -> Confinement;
    pub(crate) fn command(&self, policy: &Policy, argv: &[String],
        cwd: &str, tmp: &str) -> Confined;
}

/// papers.rs. The sandbox and the shells belong to the local
/// workspace, so a remote workspace holds neither.
enum Workspace {
    Local { sandbox: Sandbox, shells: Shells },
    Remote(Transport),
}
```

`Kind::Missing`, `Sandbox::platform()`, `Houses::probe` and
`Chats.sandbox` go. `service.rs`, `sandbox/reach.rs` and `hand.rs` take
the workspace's sandbox, and the state and prompt builders ask
`Houses::confinement()`.

The client protocol replaces `State.sandbox: String` with
`State.confinement`, and `V` moves to 2.

```rust
pub enum Confinement { Sandbox { name: String }, Insecure, Remote }
impl Confinement {
    pub fn enforces_network(&self) -> bool;
}
```

The window shows the "Insecure mode" chip for `Insecure`, and the
network switch adds "not enforced" when `enforces_network` is false.
The gate does not change. The server prints a banner before it serves:

```text
WARNING: --insecure. Agents' commands run without a sandbox, with this
account's full reach, and the network switch is not enforced.
```

The prompt says commands are not confined, and its network line says
the switch is not enforced. The policy hash names `insecure`. The CLI
writes the confinement into `server.json`. When bare `hamlet` attaches
to a server in insecure mode, it prints "Attached to a server in
insecure mode."

A Windows sandbox later is one more `Kind` with its probe and its
`command`. No caller changes.

## What changes in the spec and docs

- **C16, C17.** The shell row names `bash` and `powershell`, and the
  sandbox and gate are "the shells'".
- **C18.** "A server without a sandbox does not start, unless the chief
  starts it in insecure mode. Then commands and hand tools reach
  whatever the account running the server can. The built-in file tools
  keep the reach above, and the window warns for as long as it runs."
  It gains the leftovers rule: they keep running after their command,
  and end when the server stops and before their agent's house is
  erased or moved.
- **C19.** "Without a sandbox nothing enforces this. The switch decides
  only what waits for a chief."
- **C21.** Auto mode's "the sandbox is the guard" gains "and without
  one, nothing is".
- **S12.** Bubblewrap on Linux, Seatbelt on macOS, and on Windows only
  insecure mode for now.
- **ADR 0008**, "Tools run without a sandbox only when the chief says
  so", and **ADR 0009**, "System calls that differ by OS go through
  `hamlet_os`", both `proposed` until their PRs land. ADR 0003's
  consequence points to 0008.
- `sandbox.md`, `protocol.md`, `tour.md`, `cli.md`, `releases.md`,
  `code_convention.md` and `AGENTS.md` change with the code that makes
  them true. `CONTEXT.md` already has **Sandbox**, **Insecure mode** and
  **Leftover**.

## Delivery

Four PRs, each on the one before. Every step is one commit that passes
`cargo xtask check` on its own.

1. **Restructure on Unix.** Fold `papers.rs`'s helpers and
   `gifts/tree.rs` into `papers/tree.rs`, with the missing hard-link
   test. Portable link helpers in `hamlet_testing`. `hamlet_os::fs` on
   Unix under the house store, then the tools' walk and path text.
   `hamlet_os::process` on Unix with the spawn lint. Hand tools without
   the quoted script. Linux and macOS behave as today, but for three
   named changes: every file open is non-blocking, `clear` makes each
   folder `0o700` first, and hand tools lose the quoted script.
2. **Insecure mode on Linux and macOS.** ADR 0008 and the spec first.
   The shell table, still bash only. Leftovers kept per agent and ended
   at server stop and before a house is erased or moved. Insecure mode
   on the server. The window's chip and client protocol V2. The flag
   and the attach note in the CLI.
3. **The Windows server.** `hamlet_os::fs` on Windows, junctions
   included, with `cargo xtask cross` and a `windows-latest` job.
   `hamlet_os::process` on Windows with the server's kill-on-close job.
   The server built and tested on Windows with Git Bash.
4. **PowerShell and shipping.** `ToolSpec.retain` in the runtime
   protocol. PowerShell, with the `.ps1` fallback. The CLI starting a
   Windows server. The Windows archive with the server and pi, which
   needs Node installed, as the other archives do.

Windows is checked four ways: the crate's cross-check on Linux, Wine
as a smoke check of its tests, the `windows-latest` runner for what
needs no desktop, and a Windows 11 virtual machine on the development
machine (dockur/windows, under Docker with KVM) for full sessions with
the window. A full session in that machine closes 3 and 4.

## Tradeoffs

- A new crate, for a Windows check that runs on Linux and platform
  code nothing else can import.
- A Windows tree loses execute, group and other bits. Moving houses
  across machines is not a goal.
- No folder fsync on Windows. A crash may undo the last rename, and
  the recorded batch is finished again at the next start, where `put`
  accepts an identical file already there.
- On Unix a crashed server leaves its leftovers running.
- A Windows leftover holding a command's output parks one tokio
  blocking thread until it exits.
- `max_file_mb` is not enforced on Windows, and results say so.
- The file tools keep their limits in insecure mode while commands do
  not. It looks inconsistent, and it buys one code path that behaves
  as on Linux and macOS.

## Rejected

- **A store trait in `hamlet_server`.** A second seam beside `Files`,
  a type parameter or trait object for callers, and no cross-check
  without mingw.
- **`cfg` twins in `papers/tree.rs` and `sandbox/env.rs`.** The
  smallest diff, but platform code stays in three files and nothing
  checks the Windows half until the whole server does.
- **`cap-std`.** No rename that refuses to replace, mutations that
  rebuild paths, `set_permissions` that follows a final link, and
  links followed when they stay inside. It removes no unsafe code.
- **Path-based `std::fs` on Windows.** A junction needs no privilege,
  so an agent could redirect the server.
- **A third `Workspace` variant.** About 34 new match arms, and
  `admits` would give it no size limit. Confinement concerns commands
  only.
- **One `shell` tool with a chosen dialect.** It renames `bash`,
  breaks kept calls and frames, and leaves the model to guess the
  language.
- **Modes in an NTFS extended attribute.** Exact round trips for house
  moves, which are not a goal, at the cost of NTFS only, a second
  write per file and more unsafe code.
- **Refusing a deed on a busy Windows house.** Ending the agent's
  leftovers first gives the same result on every platform.

## Not in this proposal

- PowerShell on Linux and macOS.
- An owner-only ACL on a new Windows home. The default home sits in the
  user profile, which is already private.
- Migrating charters a chief has edited, which keep saying `bash`. The
  prompt names the shells actually offered.
- A Windows server with a remote workspace. It keeps compiling, and is
  not otherwise a goal.
- A Windows sandbox.

## Risks

- A Unix process group id can be reused once every process in it has
  ended, so ending an old group could reach a new one. Ending
  leftovers must check that the group is still the one recorded.
- MSYS may read Rust's quoting of a multi-line `bash -c` script
  differently. The fallback is a script file in the command's temp
  folder.
- The PowerShell exit-code epilogue is untested for a script that ends
  in a cmdlet.
- Job nesting when the server starts inside a terminal's or an IDE's
  job is untested.
- Wine emulates reparse points, `FileRenameInfo` and job limits only in
  part. It is a smoke check, and `windows-latest` and the Windows 11
  virtual machine decide.
- Driving the virtual machine's desktop from outside, to start the
  window and take screenshots, is unproven. Its setup proves it before
  any check relies on it.
