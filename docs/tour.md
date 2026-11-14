# A tour of the code

Where things are and where to change them. Start with
[`README.md`](../README.md) for running Hamlet and [`AGENTS.md`](../AGENTS.md)
for the checks and conventions.

## How a change travels

The Rust server owns the realm and permissions. Its Node pi harness owns
conversations and model calls. The window and CLI are its clients, over the client protocol;
neither opens the server's database nor runs an agent.

```text
Window / terminal -- client protocol --> hamlet_server -- runtime protocol --> pi
                                               |                               |
                                  realm.sqlite, scholars/*.git           harness store
                                  lodges, sandbox                        conversations
```

A typed line reaches the server through `clients/requests.rs`, whose one
match names the owner that answers each client message. A line that begins
with `/` is a command (`commands.rs`); other words become a kept input of the
agent's chat (`talk/chats.rs`). Every change to the realm is made by the loop
in `core.rs` in one transaction, which also keeps the jobs the change needs;
after the commit the loop starts them and sends the clients the entities the
transaction touched. Pi asks the server for its prompt, tool permissions and
model budget. The server checks calls again when executing them, including
replayed calls. Kept harness updates feed watched transcripts and the
server's state. `hamlet_client` turns `state`, `change`, `transcript` and
`turn` into Bevy entities; world and panels read them.

## The crates and packages

Rust crates live under `crates/`. The root `Cargo.toml` is only the
workspace: it holds no package, so `cargo build`, `cargo test` and
`cargo clippy` there cover every crate. `crates/hamlet_app` supplies
`hamlet_app` and the visual examples.

`xtask/` is a workspace of its own, run as `cargo xtask`. Its `check`
is nine steps, `format`, `clippy`, `cross`, `deny`, `test`, `acceptance`,
`harness`, `doc` and `node`, each a task of its own;
`.github/workflows/rust.yml` runs them by name, and spells out only what
no task covers: the mingw clippy of the server and CLI, and the macOS and
Windows jobs. `deny.toml` at the root configures cargo deny,
which `deny` runs. `doc` documents `hamlet_actor_core` apart from the
rest, at its default features, because its two runtime features exclude
each other. `killkit` and `idle` are the tools of the actor runtime,
described below.

`vendor/fynix` is the submodule that supplies `bevy_fynix`, the panels'
view layer, with the rich field the chat is typed in; the workspace excludes
it, and it is not edited here.

| Crate or package | Owns |
|---|---|
| `hamlet_cli` | `hamlet`: process lifecycle, terminal chat, editors and login, without Bevy; and `hamlet_launch`, what an installed app opens (`src/launch.rs`) |
| `hamlet_server` | `hamlet_server`: the realm's society, its conversations, the gate, budgets, tools, clients, credentials and harness supervision |
| `hamlet_store` | The realm's database: `schema.sql` with Diesel's tables generated from it, opening and checking it, transactions, markers and jobs, and the typed ids |
| `hamlet_git` | Scholars' and the library's repositories through gix, with no git program: refs, commits from files, reads, diffs, merges, reverts, versions, history, purging, and the writes that check a lodge out |
| `hamlet_sandbox` | What confines a command: the bubblewrap and Seatbelt policies, launching, a command's leftovers, the secrets taken out of what it gives back, and a file replaced only while it holds what was read |
| `hamlet_os` | The system calls whose form differs by OS, with no C code below them: `fs`, directories held open and what is reached from them one name at a time, never through a link at that name, a file replaced by a new one renamed over it, and the tools' walk, path text and error words; `process`, a command's process tree, started inside its limits and killed whole |
| `hamlet_app` | `hamlet_app`, the window: its panels, menus, chat and pointer, and the eight examples that draw the world and the panels |
| `harnesses/pi/` | Node pi-durable/pi-ai runtime; conversations, provider calls, compaction and forks |
| `hamlet_protocol` | Both protocols' types: `client` and `runtime`; `Version`, the semver version each protocol carries; `RequestId`, which holds only a hyphenated UUID; `State::apply` for a client's changes; recorded frames, the schema pi reads and the client schema's fingerprint |
| `hamlet_acceptance` | Real-process protocol, CLI and OAuth scenarios on scratch realm folders, through the typed `Client`, the `hamlet` command and `hamlet_testing`'s server |
| `hamlet_client` | Connection, outbox, realm, waits and watched chats as Bevy entities, the `Index` that finds them by their protocol ids, and images and files read in parts off the frame |
| `hamlet_desktop` | File/link openers, editor files, dialogs (`choose.rs`), the clipboard's images and files, wake locks and window icon |
| `hamlet_update` | A release updating itself: how this copy was installed, the signed manifest, the download and putting it in place; its `check` feature, the window's alone, is what touches the network |
| `hamlet_assets` | Embedded assets and Bevy asset loading |
| `hamlet_realm` | The two islands, the city, the harbor, hamlets, houses, roads, robots, the Hand, the library, chief, sound and camera |
| `hamlet_ui` | The widget kit: theme, styles, controls and their effects, with no client state |
| `hamlet_vfx` | Visual effects |
| `hamlet_testing` | Fixtures shared by tests: scratch folders, protocol values, recorded sessions' server messages, faux scripts, the loopback OpenAI endpoint, and a server process on a scratch realm folder with a raw connection to it (`send`, `until`, `answers`) |
| `hamlet_workspace` | The Linux remote-workspace worker |
| `hamlet_workspace_wire` | The workspace protocol for both sides: one type per operation with its reply, error codes, limits, the walk order and the shared fixture |
| `hamlet_actor_core` | The actor runtime: an actor's lifecycle, sleep and wake, schedules and alarms, queues, connections, state, SQL and its transactions, the inspector and metrics; `host::ActorHost`, what it needs from a host, and `host::RunningActor`, one started actor |
| `hamlet_actor` | Writing an actor in Rust: actions, events, queues and connections on top of the core |
| `hamlet_actor_client` | A Rust client that calls actors |
| `hamlet_actor_wire` | The actor runtime's shared types and versioned wire formats, generated from the schemas in `schemas/` |
| `hamlet_actor_error`, `hamlet_actor_error_macros` | The actor runtime's structured errors and the derive that declares them |
| `hamlet_host` | Runs actors in this process, each in one plain SQLite file; the `demo` example, its counter actor and its stand-ins for the realm and an agent; fault points behind the `faults` feature |
| `hamlet_actors` | Hamlet's actors on the actor host: the `workspace`, which runs the agents' commands and file changes within each agent's reach |
| `hamlet_messages` | The calls Hamlet's actors make to each other and their answers, one module per actor that takes them, and the CBOR fixture of each |
| `hamlet_sqlite` | The actor runtime's SQL requests run on a rusqlite connection, for `hamlet_host` and for the core's tests |
| `hamlet_killkit` | Kills the demo host at a scenario's step and checks what survived, the workspace's commands included, as `cargo xtask killkit` |

The workspace's `bevy` entry turns on no feature: `hamlet_app` turns on
the product's set, and every other crate names only the ones its own
code needs. `hamlet_client`, `hamlet_assets` and `hamlet_desktop` use
Bevy's parts (`bevy_app`, `bevy_ecs`, `bevy_asset`, ...) rather than
`bevy`, so `cargo test -p hamlet_assets` builds no renderer.

`hamlet_client/src/connection.rs` applies the server's frames in
`PreUpdate` (`ClientSystems::Receive`), so the realm and the panels see a
frame in the update it arrives in, and sends the outbox in `Last`
(`ClientSystems::Send`). The connect task calls the `Wake` resource after
each frame it hands over; `hamlet_app/src/window.rs` inserts one that wakes winit's
reactive loop, so a frame is drawn at once rather than at the loop's
next timeout. `connection::Contact` is the window's standing
with its server: connecting, connected, lost or refused. `index.rs` finds
a hamlet, an agent, a scholar, a wait or a chat by its protocol id, kept by the
components' hooks.

`RealmPlugin` in `hamlet_realm/src/lib.rs` adds the scene, then the
generator, the chief's interactions and the bridge from client entities.
`scene::plugin` adds the scene's nine plugins in order; examples that
only draw a scene add it themselves, and the hamlet generator also adds
`generator::plugin` and an `Origin` for its seed. The realm's steps
before the world are `RealmSystems` in `hamlet_realm/src/stage.rs`
(`Steer`, `Aim`, `Act`, `Bridge`, then `WorldSystems`), and
[`bevy.md`](bevy.md#order) says what goes in each. In the generator,
`society::Ground` is what can be picked up and set down (a hamlet, the
chief's place, the library), and `Trips` the letters on the roads right
now.

`hamlet_app/src/screens/` groups the panels drawn from the client's
state; the other panels live beside that folder, and `tuning.rs` loads
their sizes and limits from `assets/ui.toml`. They are built from
`hamlet_ui`, which reads no client state: `theme.rs` and `styles.rs`
define how the panels look, and `widgets/` holds the shared controls:
`effects/`, `icons.rs`, `tabs.rs`, `platform.rs` and `keys.rs`, the one
builder of a shortcut: the chord a command is bound to, shown as the
keyboard prints it (`⌘⇧K` on a Mac, `Ctrl+Shift+K` elsewhere). `markdown/` parses
Markdown and draws it. A control asks for a sound with `ControlSound`
and lights its panel from `Attention`; the app plays the one and keeps
the other. A document finds a written path's file through `PathLinks`
and draws a picture through `PictureView`, which the app's `links` and
`pictures` screens set. The panels' steps before fynix mounts are `PanelSystems` in
`hamlet_app/src/lib.rs` (`Follow`, `Bind`), in `PreUpdate`;
`panel_sets` orders them, and the apps in `testing.rs` add it, so a UI
test settles in one update.

## The store and the repositories

`hamlet_store` is the only crate that knows the database's layout. Its
`schema.sql` is the one schema, with a comment on every table; `schema.rs`
is generated from it by `cargo test -p hamlet_store`
([ADR 0010](adr/0010-the-server-queries-its-store-through-diesel.md)). `Store`
opens or creates `realm.sqlite`, with WAL, full sync and foreign keys, and
refuses a database that is not a realm's or whose version is not compatible
with its own ([ADR 0013](adr/0013-every-protocol-and-the-realm-database-are-semver-at-0-1-0.md)).
`Store::immediate` begins a transaction. `marker` keeps the outcome of a
request under its identity and refuses a changed request under a used one;
`job` keeps an effect to run after the commit
([ADR 0012](adr/0012-the-realm-is-a-database-of-tables-changed-in-transactions-with-a-jobs-outbox.md)).
`ids` holds the typed ids and the `Actor` that makes a change.

`hamlet_git` is the only crate that touches a scholar's repository
([ADR 0011](adr/0011-scholars-are-git-repositories-hires-are-branches-lodges-are-checkouts.md)).
`Repository` creates and opens a bare repository, reads refs and files, commits
changes or a whole listing of files, merges, reverts, tags, diffs, reads
history and starts a branch or a repository from another's commit. `purge.rs`
rewrites a repository without a path. It reads and writes no folder of a lodge
itself: the server lists a lodge's files with their blob ids and `plan` says
which writes make the lodge hold a commit.

## The actor runtime

The server does not use the actor runtime. Six crates hold it:
`hamlet_actor_core`, `hamlet_actor`, `hamlet_actor_client`,
`hamlet_actor_wire`, `hamlet_actor_error` and `hamlet_actor_error_macros`.
Their code is ported from another project, so each is
licensed `Apache-2.0` alone, with a `LICENSE` and a `NOTICE` that names
the origin, and each file changed from the origin says so on its first
line. The rest of the workspace is `MIT OR Apache-2.0`. Code written for
Hamlet, such as `hamlet_host`, `hamlet_actors` and `hamlet_messages`,
stays under the workspace licence.

The core reaches its host through the `ActorHost` trait in
`hamlet_actor_core/src/host.rs`: an actor that asks to sleep or to stop,
the alarm it pushes, and the SQL to serve. `host::RunningActor` starts one actor on a host, calls its actions
through `host/dispatch.rs` within the caller's deadline or the actor's
action timeout, whichever is shorter, and stops it. `host/http.rs` keeps
the helpers of an actor's HTTP routes that need no engine; nothing calls
them.

`hamlet_host/src/local.rs` is a host on that trait. `Host::open` locks
the realm folder with `<name>.lock`, `host.lock` for the default name,
and refuses a second host of that name; hosts of different names, each
with kinds of its own, share the folder. Each host also locks
`actors/<kind>/.lock` for each of its kinds, so no two hosts hold one
kind.

`get_or_start` finds an actor by kind and key and starts it on the file
`actors/<kind>/<key>.sqlite`, making it if it has none. A kind, a key and
a host's name take only lower-case letters, digits, `-`, `_` and `.`, and
do not start with `.`, so no two of them name one file on a disk that
ignores case. Every entry point refuses another, as `Host::open` refuses
such a kind.
`local/create.rs` holds `create`, the other way to make an actor: it
makes the actor with an input, kept in the file's `_hamlet_actor` row
and given to each start, and fails with `actor.duplicate_key` for a key
that holds an actor. A create whose own start fails removes the actor it
made, so the key is free again. A create of a key whose destroy is unfinished
never opens the old data. An actor makes another as itself with
`Host::create_from(&ctx, ...)` and `Host::get_or_start_from(&ctx, ...)`.
Nothing else makes an actor.

`local/call.rs` holds the calls: `call` calls an action
of the actor at an `Address`, starting it from its file first, within
the caller's deadline, and logs the call with its caller, its target
and its action. A call that belongs to a realm task carries that task
in its own message. A call to a key with no
actor fails with `actor.not_found` and
makes no file. Each call carries its `Caller` as its connection's
parameters: none
for `call`, a client for `call_for_client`, and the calling actor's
address for `Host::call_from(&ctx, ...)`, which takes it from the
actor's context. One check, `admit`, runs before every call reaches the
directory, and before every create or get-or-start by an actor, and
refuses an actor's call, create or get-or-start of itself. A call fails at once
when its host begins to shut down. `Host::open` refuses a kind that
cannot read the stamp: the authoring layer gives each factory the
decode its connection preflight runs. `local/access.rs` gives an actor,
through `HostRef::of(ctx)`, the realm folder and the values the program
gave with `HostOptions::with_value`. `directory.rs` maps each actor
kind to where its actors run, here always this host; a call to a kind it
does not hold fails at once with `actor.not_registered`.

Each actor that is not stopped with no deadline is one `Life` in the
host's map: read by boot, stopped with a deadline, starting, running,
stopping, being destroyed, or stuck. The table on `Life` in `local.rs`
lists every move between them.
A start or a stop runs on a task of its own and waits only on its own
actor, and each start gives the core a handle that reaches that
generation's file alone. A start that waits for another start of its
actor, a call's included, cancels a sleep the starting generation asked
for, so that generation answers it. A start must end within
`HostOptions::start_deadline`, 60 seconds by default. Past it, the start
fails with `actor.not_ready` and closes the file. An open of the file
still waiting then, for a file another program holds, is given up: it
writes nothing, however late it ends. A stop must end within
`HostOptions::stop_deadline`, 45 seconds by default. An actor that does
not stop by then is stuck: its stop fails with `actor.shutdown_timeout`,
as does every later start or stop of it in this host, and its file keeps
its running mark, so the next boot starts it or finishes its destroy. A
failed start of an actor with a reason of its own to run, a run a kill
cut off or a stored deadline, is tried again by a timer: at that
deadline if it is still ahead, else `HostOptions::start_retry` later,
30 seconds by default, for as long as it fails. A start that a call, a
create or `get_or_start` began is not tried again, and its failure keeps
the deadline the actor held. `shutdown` stops every actor at once, ends
the starts under way after a grace of one second, waits until each file
is closed, lets the folder go, and leaves the host starting none.

`plain_sqlite.rs` keeps each file on a thread
of its own and opens it with `rusqlite` in exclusive locking mode, in
write-ahead mode with `synchronous=NORMAL`. `hamlet_sqlite` runs the
actor's SQL on that connection. The actor runtime's tables are named
`_actor_*`, and the host's one row per file is `_hamlet_actor`, with the
actor's generation, a mark that it is running, a mark that it is
destroyed, and the deadline its last clean stop left: the earliest of
its schedule rows, its run wake deadline and the last alarm its core
pushed, read once the core has stopped, and the input a create gave
it, which each start gives the core. `Host::stop` sleeps an actor with
`ShutdownKind::Sleep` and destroys one with `ShutdownKind::Destroy`. A
destroy reaches an actor that is stopped, or absent from the map, as
well as a running one. A destroy writes its mark
before it removes the actor's file, write-ahead file and shared-memory
file, and a boot or a start that finds the mark finishes the destroy.

The host keeps no timer for a running actor. A stopped actor
with a deadline has one, which starts it from the file it has and never
makes a file.
`boot` reads the files of the host's own kinds, but for
the actors it holds already, begins the start of each actor a kill left
marked running or whose deadline has come, without waiting for it, and
arms a timer for each other stored deadline. While boot reads an actor's
file, a start of that actor waits, and a file boot cannot read is
reported on `Host::events` as `HostEvent::UnreadableFile` and passed
over.

A one-shot timer, armed with `ActorContext::at` in
`hamlet_actor_core/src/actor/schedule.rs`, is tried at most five times,
`MAX_ONE_SHOT_TRIES`. Each try is a claim of the timer's row, committed
before its action runs. The core deletes the row once the handler
answers, with its result, an error of any code or a panic, or the
handler deletes it with `end_one_shot` in its own transaction. A
handler that did not answer, because a stop cut it short or its reply
was dropped unsent, keeps the row for the next try. The drain that finds a row with
five tries runs nothing for it: it moves the row to
`_actor_schedule_set_aside`, and the host reports the timer on
`Host::events` as `HostEvent::TimerSetAside`. `Host::set_aside_timers` lists an
actor's set-aside timers, and `Host::rearm_timer` puts one back with its
tries counted from none. A set-aside timer leaves only by a re-arm that
succeeds, or with its actor's file at a destroy.

`examples/demo.rs` drives
the host from the command line, under a host name and with counter kinds
of its options, each line run by `examples/demo/command.rs`, with the
counter actor in `examples/demo/counter.rs`; a counter calls another as
itself, and `calls --count <n>` times those calls. The demo also
registers the [workspace](#the-workspace-actor), given a sandbox where
`Sandbox::open(Isolation::Sandboxed)` gives one, and two stand-ins that
call it: `examples/demo/realm.rs`, the actor `realm/local`, which sets
reaches, answers for the realm's secrets with one made-up secret and
answers waits as a chief; and `examples/demo/agent.rs`, the actor
`agent/<n>`, which makes `actors/agent/<n>.fs/` as it starts, runs
scripts and tools, reads and writes files, and keeps each `Finished` it
is told. `tests/local.rs` tests the host,
`tests/create.rs` tests `create`, and `tests/access.rs` tests what an
actor reaches of its host, its calls to other actors included.
`tests/timer.rs` tests one-shots that come due as their actor sleeps or
is destroyed, handlers that panic, and a one-shot set aside after five
tries. `tests/drain.rs` tests the drain of an actor's events when its
event loop panics, and `tests/kill.rs` kills a child
host at a fault point and boots the folder again.

The core's own tests are in `hamlet_actor_core/tests/`. Each file there,
apart from the `integration` target's three, is compiled into the
library by a `#[path]` line, most of them in the module they test.
`lib.rs` pulls in two whose module is gone: `envoy_callbacks.rs` tests
`hamlet_actor_wire`'s endpoint parser, and `registry_http.rs` tests
`host/dispatch.rs`, `host/http.rs` and the request buffering in
`actor/messages.rs`. `metrics.rs` pulls in `metrics_helpers.rs`.
`integration.rs` is a test target of its own, which pulls in
`integration/metrics.rs` and `integration/tokio_runtime_metrics.rs`.
Most of the library's tests run on `TestHost` in `src/testing.rs`, a
host that serves one in-memory database through `hamlet_sqlite`.

`cargo xtask killkit` runs `hamlet_killkit`. Its scenarios are rows of
`src/scenario.rs`: the commands that set up, the fault point or the time to kill at,
the commands after each restart, and what each restart must show.
`workspace-command` kills the host while a command answered `Running`
still runs, and checks after each restart that the command did not go
on, that its row reads cut off and that the agent was told nothing.
`cargo xtask idle` is `xtask/src/idle.rs`. It reads `/proc` for the
resident memory of a process tree and the growth of each thread's
`voluntary_ctxt_switches`.

## The workspace actor

The server does not use it. `hamlet_actors` holds Hamlet's actors on the
actor host. Its one kind is `workspace`, in `src/workspace.rs` and
`src/workspace/`: one actor per machine, keyed `local`, that runs the
agents' commands and file changes under the sandbox, within each agent's
reach (C18 to C21). `hamlet_messages` holds the calls between actors and their
answers, one module per actor that takes them: `workspace` (with
`workspace/reach.rs` and `workspace/files.rs`), `realm` and `agent`.
Each crosses the host as CBOR. `fixtures/` holds the bytes of each
message, and `tests/fixtures.rs` rewrites a fixture whose bytes changed
and fails once, so the change is committed.

The realm, the actor `realm/local`, sets each agent's reach
(`SetReach`), answers calls that wait for a chief (`ChiefAnswer`) and
may cancel any agent's commands. An agent, the actor `agent/<n>` keyed
by its `AgentNumber`, calls for its own tools: `Run`, `CommandStatus`,
`Cancel`, `ReadFile`, `Search`, `Find`, `WriteFile` and `EditFile`.
`agent_of` in `workspace.rs` takes the agent from the caller the host
stamped and refuses any other caller, or an agent that names another.
`ListTools` checks no caller. The workspace calls the realm with
`Secrets` and `NeedsChief`, and an agent with `Finished`. In this
repository the realm and the agent are only the demo's stand-ins. The
host gives the workspace its `Sandbox` as a value of `HostOptions`, and
the workspace does not start without one; `workspace::Options` there
sets how long a call waits for its command.

`workspace/reach.rs` keeps the reach: one row per agent in the table
`reach`, with its version and its `Reach`, an agent's, a worker's (with
its starter, none for a chief's worker) or ended. Only the realm sets
it. `apply` keeps a `SetReach` only when its version is newer than the
one in force, and the answer is `InForce` with the version in force
after the call, so a reach that arrives late changes nothing. A row is
never deleted, so an ended reach stays ended.

`gate` in `reach.rs` weighs each call under the reach in force. It
refuses a call with no reach or an ended one, and a path out of reach or
in an agent's filesystem. It answers `NeedsSecrets` while the workspace
holds a list of the realm's secrets older than the version the reach
names: the call then fetches the list from the realm once (`Secrets`)
and passes the gate again. The list is held in memory only, so each
start fetches it again. It answers `NeedsChief` with what the call
waits for (H11): the network for a command that asks for it, or names a
word of `NETWORK_WORDS`, while the reach has none; a command or an edit
of the folder its hamlet works on in manual mode, without trust for
the conversation (C21). Otherwise the call runs.

A command's life is in `workspace.rs` and `workspace/run.rs`. `Run`
carries a `Program`: a bash script, which runs at most 120 seconds or
its shorter `timeout_ms` (C16), or a tool added by hand. A call that
passes the gate opens a row in the table `calls` (`calls.rs`), keyed by
the agent and the call's id, with the hash of its request; the row's id
is the `CommandId`. The command runs on a thread of its own
(`run::start`). The call waits 5 seconds (`run::ANSWER_WITHIN`): it
answers `Done` with the `CommandEnd` if the command ended by then, else
`Running` with its id. After `Running`, the workspace calls the agent
once with `Finished` when the command ends; a failed call is logged and
dropped. The actor stays awake while a command runs. A command whose
host stopped or was killed keeps its row `running`, and the next start
of the workspace marks it cut off (`run::CUT_OFF`); no one is told.
`CommandStatus` answers by command id, for the agent's own commands. A
repeat of a call with the same id and request answers from its row, or
waits with the first call while the command runs; the same id with
another request is refused as `CallReused`.

`workspace/run/output.rs` keeps a command's output, the realm's secrets
taken out as it streams. Its end carries the tail, the last 2000 lines
or 50 KiB. Once the output passes the tail, all of it goes to a spill
file, `actors/workspace/local.output/<agent>/<command>.log`, up to
512 MiB (`SPILL_CAP`), which the end's `Cut` names and the agent may
read.

`workspace/files.rs` passes the file tools through the gate and runs
them on the sandbox's `Env`, off the runtime, with the secrets taken out
of the answer. Paths arrive absolute. `ReadFile` (`files/read.rs`),
`Search` and `Find` (`files/walk.rs`) keep no row. `WriteFile`, at most
16 MiB (`MOST_CONTENT`), and `EditFile`, exact replacements
(`files/edit.rs`) shown as a diff (`files/diff.rs`), are changes, in
`files/change.rs`. One change at a time holds a path, by the path as
written. A change reads the file, writes its row as `changing` with the
path of the new file it is about to make, writes the content to
`.hamlet-<uuid>.new` beside the file and renames it over the file with
`Env::replace_file`, only while the file still holds what was read;
otherwise it is refused as `FileChanged`. Its row then holds the
answer, so a repeat by call id answers from it. A start removes the new
files and the rows of changes a kill cut off (`cut_off_changes`), so a
repeat of one runs afresh.

`workspace/pending.rs` keeps calls that wait for a chief (H11, H34).
Such a call's row is `waiting`, and its own request is in the table
`waits`, with the reach version it was weighed under and, for a change,
the hash of what the file held. A request that holds one of the realm's
secrets is refused as `HoldsSecret`, since no secret is stored. After
the commit the workspace calls the realm with `NeedsChief`, the call in
words with the secrets taken out, and answers `WaitsForChief`. The
realm answers with `ChiefAnswer`. A refusal or a lapse ends the wait. An
allow checks the call again under the reach in force (`recheck`): it
runs when the gate lets it, or when it needs only what the chief was
asked; else it is refused as `ReachChanged`. A change also needs the
file to still hold what hashes to what the chief was shown, else
`FileChanged`. An allow's scope is in the reach the realm sets before
it answers, not in the answer. The agent hears how the call ended with
`Finished`. A wait stays through a restart.

`Cancel` ends an agent's running commands, one call's, one
conversation's or all of them, as `Exit::Cancelled`, and answers once
each has stopped. It ends no wait. A `SetReach` to `Reach::Ended` ends
the agent's waits as lapsed and tells the agent of each, cancels all its
commands, ends what they left running under Seatbelt, and removes a
worker's scratch folder.

`workspace/workers.rs` keeps the workers' scratch folders,
`workers/<n>/` (H42): made when a worker's reach arrives, and again
before a call when one is missing; removed when its reach ends; and
swept at the workspace's start for each worker whose reach ended. An
agent's filesystem is `actors/agent/<n>.fs/` (`RealmFolder::filesystem`
in `reach.rs`). The agent makes it, and the workspace's file tools never
change it. When its hamlet has no folder, an agent's commands run
there, and a worker's in its scratch folder.

`workspace/tools.rs` holds the tools a chief adds by hand (C17): each a
folder of `tools/` with a `manifest.json`. `ListTools` lists those that
load, in their folders' order, and why each other folder does not; a
name that a built-in or a society tool has is reserved. `Program::Tool`
runs one: its arguments must be a JSON object its manifest's schema
takes, and it reads them on its standard input. It runs in its own
folder with the reach, environment and limits of its manifest rather
than the agent's reach, and the gate weighs it as a command whose
network is its manifest's. `tools/` and `library/` are locked in every
agent's policy, so no agent writes them. How each policy is built, and
what holds on each OS, is in [`sandbox.md`](sandbox.md#the-workspace-actor).

`tests/workspace.rs` and `tests/workspace/` run the workspace through a
host, called by test actors in the realm's and agents' places: the
reach and commands in `workspace.rs`, then `files.rs`, `changes.rs`,
`waits.rs`, `cancel.rs`, `workers.rs`, `tools.rs` and `realm_folder.rs`. `hamlet_messages/tests/fixtures.rs` checks the messages.

## The server

Paths below are under `crates/hamlet_server/src/`. Modules use a file
and same-named directory rather than `mod.rs`. Most domain APIs are
internal; the crate exports only `serve`. Its unit tests sit beside their
modules, on an in-memory or scratch database and scratch repositories
(`testing.rs` holds what several share). The `faults` feature compiles the
points where a test can kill the server (`fault.rs`), and `harness` the tests
that start the pi harness; both are for tests only. The `crashes` target,
`tests/crashes/`, starts the server built with them, kills it at each point
and serves the realm again; `remote.rs` there serves a realm in a loopback
double of the remote workspace worker.

| Module | Responsibility and interface |
|---|---|
| `serve.rs`, `realm.rs`, `claim.rs`, `layout.rs` | `serve` from start to stop: probes the sandbox, opens the realm, starts the core and the harness, publishes `server.json`, and closes in order. `Realm::open` claims a folder, opens its `Store`, checks the `realm.id` pair, and makes a new realm with its first chief and its Hand. `Layout` names every path of the realm's folder |
| `core.rs` | `Core`, the loop that owns the realm's writes: it takes in client requests, runtime messages, finished tool calls and the jobs runner's wake-ups, makes each change in one transaction (`change` and `attempt`, the second rolling back a refusal), then wakes the jobs runner and the runtimes and publishes what was touched |
| `change.rs`, `jobs.rs` | `Change`, a change in progress: its transaction, its time, its actor and what it touched. `Job` names each effect a change keeps for after its commit |
| `society.rs`, `society/` | The society's domain functions, each taking a `Change`: `agents` (what every agent has), `chiefs`, `hand`, `city` (scholars), `hamlets`, `hires`, `tree`, `workers`, `roads`, `merges`, `dreams`, `library`, `record` (decisions), `limits`, `addresses`, `names` and `authority`, the one place that says who may do what. A refusal is a `Refusal`, in words for a chief or an agent |
| `knowledge.rs`, `knowledge/` | What agents know, on disk: `run.rs` and `runner.rs` run the jobs on a thread of their own with a connection of their own (plant, lodge, commit, take-in, erase); `folder.rs` is a folder held open and reached without following links; `merging.rs`, `dreaming.rs` and `library.rs` are the repositories' side of merging hires, dreams and the library |
| `talk.rs`, `talk/` | Conversations. `runtimes.rs`, `process.rs` and `sync.rs` start the pi runtime, take it on or refuse it at its hello, start it again after a failure, and send each ready runtime what its conversations are owed; `conversations.rs`, `inputs.rs` and `items.rs` keep conversations, the inputs they are given and the entries their runtime kept; `turns.rs` answers what a runtime asks during a turn; `follow.rs` runs once after an input ends; `chats.rs`, `reflections.rs`, `letters.rs`, `errands.rs`, `briefs.rs`, `asking.rs`, `verdicts.rs` and `tasks.rs` are chats, reflection, letters and questions, one-off errands, a worker's brief, `ask_chief`, the check of a final answer, and tasks; `models.rs` picks each agent's model; `activity.rs` folds what an agent is doing; `leases.rs` holds credential leases |
| `talk/prompt.rs`, `talk/prompt/place.rs` | `sections` builds the prompt from the charter, machine, project, character, memory, skills, place, realm and scholars |
| `calls.rs`, `calls/` | What a runtime asks while a turn runs: whether a tool call may run, and the call carried out once under its key. `held.rs` keeps the gate's decision; `answer.rs` applies a chief's answer; `objections.rs` and `reviews.rs` are objections and a lead's review; `place.rs` says where an agent's tools work and which it is offered; `society.rs` and `society/` carry out the society's tools |
| `oversight.rs`, `oversight/gate.rs` | Waits, their rulings and lapses, and the gate: reach, reserved acts, manual mode, trust and grants |
| `commands.rs`, `commands/` | The chief's `/` commands: the table of them (advertised to clients as deeds) and what each does; `oversight.rs`, `shelves.rs` and `versions.rs` hold those on waits and grants, skills and a scholar's history |
| `spend.rs` | Budgets and allowances: attempts admitted against them, usage charged once, budget resets and reports |
| `dreaming.rs` | When agents dream: the hours, who is due, and `/dream` |
| `images.rs` | Images (C33, S29): uploaded in parts, checked and kept once by hash in the realm's `images/` folder, fitted to a model's limits, given to an agent in its own folder, shown by a small copy, swept when nothing uses them, and read back by id |
| `files.rs`, `inbox.rs` | Files a chief attaches (C33, S29): uploaded in parts and kept once by hash in the realm's `files/` folder; `inbox.rs` saves them, and what agents send with `tell` and `ask` (H30), in the receiver's `inbox/` and names where |
| `attached.rs` | A chief's line with its attachments: each caption set in the words at its attachment's place (C33) |
| `work.rs`, `work/` | What agents' tools do: the built-in tools in `tools.rs` and `tools/` (`files`, `shell`, `search`, `find`, `diff`, `environment` which is each agent's reach under the sandbox), tools added by hand in `added.rs`, the catalogue of society tools by name in `catalogue.rs`, and an edit's shown change in `cut.rs` |
| `clients.rs`, `clients/` | Connections (`connection.rs`), strict parsing of client messages (`check.rs`), the requests and who answers each (`requests.rs`), the state and its changes (`state.rs`), transcripts (`fold.rs`) and the chief's documents (`documents.rs`) |
| `workspace.rs`, `workspace/` | The remote workspace: each agent's session there (`serving.rs`) and a lodge's copy kept in step with the server's (`lodges.rs`) |
| `providers.rs`, `credentials.rs`, `settings.rs` | The model catalogue and provider requests, sign-ins, the atomic credentials file and secret removal, and the realm's settings with the process's overrides |
| `diagnostic.rs`, `clock.rs`, `text.rs`, `fault.rs` | The server's one diagnostic, on stderr; the clock; enums the database keeps as text; the fault points |

`realm.sqlite` holds server-owned records, chief documents, runtimes,
conversations, inputs, waits, charges, markers and jobs. It does not hold
pi's authoritative transcript. Each runtime opens only its own store under
`harnesses/<instance>/`; the server consumes committed updates through the
runtime protocol. A scholar's knowledge, the Hand's included, is in
`scholars/<id>.git`, and a lodge is a folder of the realm. See
[`realm.md`](realm.md).

## The pi harness

`harnesses/pi/src/main.ts` is the Node entry. The server starts it with
`--runtime`, `--store` and `--transport stdio`. The runtime protocol is
versioned JSON lines on stdin/stdout; diagnostics go to stderr. How
runtimes, conversations and the runtime protocol work is in
[`harness.md`](harness.md).

| File under `harnesses/pi/src/` | Owns |
|---|---|
| `protocol.ts`, `protocol-schema.ts` | The runtime protocol's types and checks, from the schema generated from the Rust types |
| `channel.ts` | Correlated protocol requests and replies |
| `runtime.ts` | Handshake, conversations, input admission, configure, cancellation, history and updates |
| `extension.ts` | The hamlet extension: tools, prompt section and allow/yield hooks, each asked until the server decides; waits end on the key the server gives |
| `recovery.ts` | Carrying on a cut-off answer: summarising and the one permitted retry |
| `items.ts` | Kept entries and answers as the protocol shows them |
| `models.ts` | Provider calls admitted and charged by the server, made with the pi model built from its selection; asks it to scrub snapshots and final responses |
| `providers.ts`, `provider-set.ts`, `credentials.ts`, `sign-in.ts` | Provider work the server requests, the supported sign-ins, credentials held in memory, and a sign-in owned by one request |
| `claude.ts`, `claude-query.ts`, `claude-history.ts` | The provider `claude` ([ADR 0014](adr/0014-claude-subscriptions-run-in-anthropics-own-program-under-pi.md)): its models, sign-in and the Claude program's environment; each call answered by a query of the Agent SDK, waiting at Claude's tool calls for pi's results; a new input resumes the Claude Code session the last query kept while pi's history still matches it, else a session written from pi's history |
| `faux.ts` | Offline scripts, with queues by agent id and by conversation kind |
| `faults.ts` | Exact process-crash pauses used by tests |

Credentials belong to the server; the harness receives only what an
authorized provider attempt needs, in memory. Its scrub requests go back
to the server. Role changes use configure/configured before the next
request. A librarian's call and a review have no tools, and a dream has the
built-in ones.

## Where to change behavior

| Change | Owner |
|---|---|
| Typed commands, command replies | `hamlet_server/src/commands.rs` and `commands/`; advertised deeds sent in `clients/requests.rs` |
| Conversation stop/retry/resume/rewind | `hamlet_server/src/talk/chats.rs` and `talk/follow.rs`; the requests in `clients/requests.rs` |
| Transcript rows, paging and peer watches | `hamlet_server/src/clients/fold.rs`, `talk/items.rs`; `hamlet_client/src/chats.rs` |
| Questions and their answer cards | `hamlet_server/src/talk/asking.rs`, `calls/answer.rs`, `hamlet_app/src/asking.rs` |
| Approvals, grants and waits | `hamlet_server/src/oversight/gate.rs`, `oversight.rs`, `calls/held.rs`, `calls/answer.rs` |
| Selecting, quoting and replying to chat text | `hamlet_app/src/select.rs`, `screens/chat/quote.rs`, `screens/chat/reply.rs` |
| Who may do what | `hamlet_server/src/society/authority.rs` |
| Founding, hiring, ending, restoring, seating a mayor, the tree | `hamlet_server/src/society/hamlets.rs`, `hires.rs`, `tree.rs`; the tools in `calls/society.rs`, the commands in `commands.rs` |
| Scholars and their versions | `hamlet_server/src/society/city.rs`, `commands/versions.rs`, `knowledge/`; the repositories in `hamlet_git` |
| Merging and splitting hires | `hamlet_server/src/society/merges.rs`, `knowledge/merging.rs` |
| Workers | `hamlet_server/src/society/workers.rs`, `talk/briefs.rs` |
| Letters, questions and roads | `hamlet_server/src/talk/letters.rs`, `calls/society/letters.rs`, `society/roads.rs` |
| Instructions and objections | `hamlet_server/src/calls/objections.rs`, `calls/reviews.rs` |
| The library and the librarian | `hamlet_server/src/knowledge/library.rs`, `calls/society/library.rs`, `society/library.rs`, `commands/shelves.rs` |
| Dreams | `hamlet_server/src/dreaming.rs`, `knowledge/dreaming.rs`, `society/dreams.rs` |
| Budgets, caps and allowances | `hamlet_server/src/spend.rs`, `society/limits.rs`, `talk/tasks.rs` |
| A prompt or chief document | `hamlet_server/src/talk/prompt.rs`, `clients/documents.rs`; default charter and the Hand's character in `hamlet_server/assets/` |
| The database's tables | `hamlet_store/src/schema.sql`, then `cargo test -p hamlet_store` |
| A job | `hamlet_server/src/jobs.rs`, `knowledge/run.rs` |
| A model connection, login or price | `hamlet_server/src/providers.rs`, `credentials.rs`; provider implementation in pi |
| Folder, mode or tool reach | `hamlet_server/src/calls/place.rs`, `work/tools/environment.rs`, `hamlet_sandbox`; [`sandbox.md`](sandbox.md) |
| CLI process discovery | `hamlet_cli/src/process.rs`; [`cli.md`](cli.md) |
| Installers, archives and what they ship | `.github/scripts/package.sh`, `hamlet.iss`, `.github/workflows/release.yml`; [`releases.md`](releases.md) |
| Updates | `hamlet_update`, `hamlet_app/src/updates.rs` and its toast in `toasts.rs`, `hamlet_cli/src/launch.rs`, `.github/scripts/manifest.sh`; [`releases.md`](releases.md#updates) |
| Client protocol | `hamlet_protocol/src/client/`, then `cargo test -p hamlet_protocol` for `client_schema.fingerprint`; [`protocol.md`](protocol.md) |
| Runtime protocol | `hamlet_protocol/src/runtime/`, then `cargo test -p hamlet_protocol`; [`harness.md`](harness.md) |
| Received entities | `hamlet_client/src/connection.rs`, `realm.rs`, `waits.rs`, `chats.rs`, `index.rs` |
| The message field and its chips: typing, sending, the place of each attachment, the chips in a sent line | `hamlet_app/src/screens/chat.rs` (`send_typed`), `chat/field.rs`, `chat/attach.rs` (the chips), `chat/bubble.rs` (a sent line); the field itself is fynix's `rich_field` |
| Images on a line: upload, fitting, what a model takes, the read tool's images | `hamlet_server/src/images.rs`, `providers.rs`, `work/tools/files.rs`; in the window `hamlet_client/src/images.rs`, `hamlet_desktop/src/clipboard.rs`, `hamlet_app/src/screens/chat/attach.rs` and, for a custom model's limits, `hamlet_app/src/models/home.rs`; in the CLI `hamlet_cli/src/chat.rs` and `terminal.rs` |
| Files and folders on a line, and sent with `tell` or `ask`: upload, the inbox, the cap | `hamlet_server/src/files.rs`, `inbox.rs`, `attached.rs`, `images.rs` (`attach`), `work/tools/parcel.rs`, `calls/society/letters.rs`, `oversight/gate.rs` (`Call::Parcel`); in the window `hamlet_client/src/files.rs`, `hamlet_desktop/src/choose.rs`, `hamlet_app/src/screens/chat/attach.rs`; in the CLI `hamlet_cli/src/client.rs`, `chat.rs` and `mentions.rs` (the `@path` tokens) |
| A panel or right-click menu | `hamlet_app/src/screens/`, `world_menu/items.rs` |
| The Hub: its rail, a thread, its details sheet, scholar and hamlet pages | `hamlet_app/src/screens/hub.rs`, `hub/` (`rail.rs`, `thread.rs`, `details.rs`, `scholar.rs`) |
| Toasts that answer petitions in place | `hamlet_app/src/toasts.rs` |
| The starting desk | `hamlet_app/src/dock.rs` |
| The two islands, where the hamlet island lies | `hamlet_realm/src/generator/land.rs`, `land/`; [`islands.md`](islands.md) |
| The city: the Hand's tower, scholars' houses, newcomers | `hamlet_realm/src/city.rs`, `city/`; the spots in `hamlet_server/src/society/city.rs` |
| The piers, boats and the sea route | `hamlet_realm/src/harbor.rs`, `harbor/` |
| What things are called in the world | `pick::Names` in `hamlet_realm/src/pick.rs`, the signs in `pick/signs.rs` |
| Panels' colors, sizes, controls and icons | `assets/themes/`, `measures.toml`, `effects.toml`; `hamlet_app/src/appearance.rs`, `screens/settings/appearance.rs`; `hamlet_ui/src/theme.rs`, `styles.rs`, `widgets/effects/`, `widgets/icons.rs` |
| World colors, size or timing | `assets/palette.toml`, `world.toml`, `vfx.toml`, `ui.toml` |
| Ground, houses and roads | `hamlet_realm/src/generator/`; `LandData::surface` is the drawn surface |
| Robot motion | `hamlet_realm/src/keeper/acts/`; [`robots.md`](robots.md) |
| Wizard, Hearth, camera or sound | `hamlet_realm/src/chief/`, `pick/camera.rs`, `sound/` |
| Helpers several modules' tests share | `testing.rs` in `hamlet_server`, `hamlet_app` and `hamlet_realm`; fixtures crates share in `hamlet_testing` |
| Window state or editor saves | `hamlet_app/src/remember.rs`, `window.rs`, `saving.rs`; `hamlet_desktop/src/editor.rs` |
| Desks and their switcher in the top bar | `hamlet_app/src/desk.rs`, `desk/switcher.rs`, `desk/switcher/motion.rs` |
| An actor's lifecycle, schedules, state or SQL | `hamlet_actor_core/src/actor/`; [the actor runtime](#the-actor-runtime) |
| Where actors' files are, deadlines and timers, shutdown, the folder lock | `hamlet_host/src/local.rs`, `plain_sqlite.rs` |
| Calls between actors, and who made a call | `hamlet_host/src/local/call.rs` |
| Making an actor with an input, or from another actor | `hamlet_host/src/local/create.rs` |
| What an actor reaches of its host: the realm folder, the program's values | `hamlet_host/src/local/access.rs` |
| Where each actor kind runs | `hamlet_host/src/directory.rs` |
| What the host reports on `Host::events` | `hamlet_host/src/event.rs` |
| A one-shot timer's tries, and the timers set aside after them | `hamlet_actor_core/src/actor/schedule.rs`; `Host::set_aside_timers` and `Host::rearm_timer` in `hamlet_host/src/local.rs` |
| A kill scenario | `hamlet_killkit/src/scenario.rs`, and a fault point with `hamlet_host::fault::point` |
| A message between actors, or its fixture | `hamlet_messages/src/`, then `cargo test -p hamlet_messages` rewrites `fixtures/` |
| An agent's reach on the workspace, and the gate | `hamlet_actors/src/workspace/reach.rs` (`apply`, `gate`, `placement`); [the workspace actor](#the-workspace-actor) |
| A command's life: its row, `Done` or `Running`, `Finished`, its status, a repeat, a cut-off command | `hamlet_actors/src/workspace.rs`, `workspace/run.rs`, `workspace/calls.rs` |
| A command's tail and spill file | `hamlet_actors/src/workspace/run/output.rs` |
| The workspace's reads, writes and edits | `hamlet_actors/src/workspace/files.rs`, `files/`; the replace by rename in `hamlet_sandbox/src/env.rs` and `hamlet_os/src/fs/walk.rs` |
| Calls that wait for a chief on the workspace | `hamlet_actors/src/workspace/pending.rs` |
| Cancelling an agent's commands, and an ended reach | `hamlet_actors/src/workspace.rs` (`cancel`, `end_agent`) |
| Workers' scratch folders on the workspace | `hamlet_actors/src/workspace/workers.rs` |
| Tools added by hand on the workspace | `hamlet_actors/src/workspace/tools.rs` |

`hamlet_realm/src/hamlet.rs` bridges received entities to the generator's
plots and roads. `Hire` marks an agent that is a hire; `HamletId` is a
hamlet's protocol id, a UUID, and its `Name` the name it is shown by.
`chief.rs` names the wizard in the world and the chief's panel in the UI;
`Chief` in `hamlet_client` is the server's chief identity. The window keeps
only its own view files under `HAMLET_VIEW`.

## Tuning and previewing the window

Assets are embedded. The `hot-reload` feature also watches their source;
`dev --hot` builds the window with it and points it at the checkout:

```sh
HAMLET_HOME=/tmp/hamlet-try cargo run -p hamlet_cli -- dev --hot
```

The generator section of `world.toml` and existing mesh colors do not
change an already-built world. The charter is a server document, edited
through the protocol rather than asset hot reload. An unreadable asset
reports an error and keeps the last good values.

| Command | Output |
|---|---|
| `cargo run --example ui_gallery -- --scene <name> --out <dir>` | Panel scenes listed in `crates/hamlet_app/examples/ui_gallery/main.rs` |
| `cargo run --release --example far_lands -- --out <dir>` | Land and hamlet images; `--bench` measures it |
| `cargo run --release --example sea_bench -- --out <dir>` | Frame times of several views, with parts turned off in turn; `--out` saves stills |
| `cargo run --example chief_sheet -- --out <dir>` | Wizard, Wickstep and library |
| `cargo run --example cursor_sheet` | Cursor sheet |
| `cargo run --release --example sound_sheet` | Generated sound WAVs |

`style_test` and `town_gen` open windows. For live work use
`HAMLET_HOME=/tmp/hamlet-try cargo xtask dev`; it builds both Rust processes and
sets `HAMLET_VIEW=<realm folder>/window`. Closing the window leaves the server;
Ctrl-C in the dev terminal stops it. Use Node from `harnesses/pi/.nvmrc`.
