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
each other. On Linux, `node` also runs `declarations`, which writes the
native module's `index.d.ts` and `index.js` from its Rust and fails when
either changed. `killkit` and `idle` are the tools of the actor runtime,
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
| `hamlet_host` | Runs actors in this process, each in one plain SQLite file; the `demo` example, its counter actor, its stand-ins for the realm and an agent, with `--realm-actor` the realm actor, the profile and the library with the real agent in a Node child, and its `profile` and `library` lines; fault points behind the `faults` feature |
| `hamlet_address` | An actor's address, the name rule for its kind and key, and `Caller`, who made a call, as a host stamps it |
| `hamlet_actors` | Hamlet's actors on the actor host: the `workspace`, which runs the agents' commands and file changes within each agent's reach; the `realm`, which keeps the hamlets, the tree of hires, roads, letters, budgets and waits; the `profile`, a profile's repository; the `library`, the shared skills |
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

`get_or_start` finds an actor by its `Address`, a kind and a key, and
starts it on the file `actors/<kind>/<key>.sqlite`, making it if it has
none. A kind, a key and a host's name take only lower-case letters,
digits, `-`, `_` and `.`, and do not start with `.`, so no two of them
name one file on a disk that ignores case. `Address::new` refuses any
other with `InvalidName`, and a decode of an `Address` refuses it too,
so an `Address` that exists is valid. `Host::open` refuses such a kind
or name. `Address`, `InvalidName` and `Caller` live in
`hamlet_address`, which the messages between actors use without the
host, and `hamlet_host` re-exports them.
`local/create.rs` holds `create`, the other way to make an actor: it
makes the actor with an input, kept in the file's `_hamlet_actor` row
and given to each start, and fails with `actor.duplicate_key` for a key
that holds an actor. A create whose own start fails removes the actor it
made, so the key is free again. A failed start or destroy that leaves an
actor's files behind warns, naming the actor, and `Host::kept` lists it.
A create of a key whose destroy is unfinished never opens the old data. An actor makes another as itself with
`create` and `get_or_start` on its `HostRef`. Nothing else makes an
actor.

`local/call.rs` holds the calls: `call` calls an action
of the actor at an `Address`, starting it from its file first, within
the caller's deadline, and logs the call with its caller, its target
and its action. A call that belongs to a realm task carries that task
in its own message. A call to a key with no
actor fails with `actor.not_found` and
makes no file. Each call carries its `Caller` as its connection's
parameters: none
for `call`, a client for `call_for_client`, and the calling actor's
address for `HostRef::call`. One check, `admit`, runs before every
call, create and get-or-start reaches the directory, and refuses an
actor's call, create or get-or-start of itself, and any with a caller
that the route table, `HostOptions::routes`, a `Routes` from
`route.rs`, does not allow: that fails with `actor.refused`, and the
host reports it in `HostEvent::Refused` and its log. A call fails at once
when its host begins to shut down. `Host::open` refuses a kind that
cannot read the stamp: the authoring layer gives each factory the
decode its connection preflight runs. `local/access.rs` gives an actor
its view of the host, `HostRef::of(ctx)`: its own address, the realm
folder, the values the program gave with `HostOptions::with_value`, and
the calls, creates and get-or-starts it makes as itself, each refused
with `actor.stopping` once its generation has ended. `directory.rs` maps
each actor kind to where its actors run: this host, or the host at the
other end of the link; a call to a kind it does not hold fails at once
with `actor.not_registered`.

`child.rs` and `link.rs` join a Rust host to its Node child, the Node
host, over the child's pipe. `child::open` opens a host whose other
kinds run in the child, and its `Supervisor` starts the child, ends it
with its tree, and starts another when it is lost: a second later at
first, and up to a minute later while each child ends before it has
stayed ready for a minute. The
child takes its ends with `take_link` and runs `link::serve`. The two
say hello, welcome and ready (`link/frame.rs` holds the frames and their
codec, `link.fingerprint` the frames' shapes), and then each takes the
other's calls and creates through its `Port` (`link/port.rs`). The Node
host's entry is `hosts/node/src/main.ts`: it takes the link before the
actor layer loads, serves the kinds of `hosts/node/src/demo.ts` its
`--kind` arguments name through `serve` in `hosts/node/src/entry.ts`,
which runs `serveLink` of `hosts/node/src/actor/registry/link.ts`, and
exits 1 on any failure. A
peer's request is checked against who that host speaks for (`link/peer.rs`),
then runs on the host's own call or start path, so `admit` checks it
again. A call, a create and a subscribe cross the link. A call to the
child fails with `link.down`, and nothing is sent, until the child is
ready, and with `link.lost`, its outcome unknown, when the child is lost
with the call out. Nothing sends a call again.

`local/subscribe.rs` holds `subscribe_for_client`, a client's
`Subscription` to one event of an actor. It takes the call's path: the
route table's check, then the start of a local actor from its file, or
an `Ask::Subscribe` request to the child. A local subscription is a
connection to the actor, opened with `RunningActor::subscribe`, which
keeps the actor awake. The reader's drop closes it; the actor's close
and the start of the actor's stop, which the host records on the
generation, end it. Across the link the child serves the subscribe
through the same path and writes `Event`, `Gap` and `SubscriptionEnded`
frames (`link/port/follow.rs`); the Rust host's `Unsubscribe` ends it.
When the child is lost, the subscription asks the next child once it is
ready and gives one `Item::Gap`. Each hop's queue is `local/feed.rs`: it
holds up to 1 MiB, or one larger event alone, never makes its writer
wait, and drops what does not fit, with one gap where the loss was.

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
the host from the command line, under a host name and with the kinds of
its options (`examples/demo/kinds.rs`: a host registers only those, or
by default the counter, the workspace, the profile, the library and
both stand-ins, or with `--link` the stand-in realm alone; whatever the
kinds, `examples/demo.rs` also registers the workspace, the profile and
the library with `--realm-actor`), each counter
line run by `examples/demo/command.rs`, with the
counter actor in `examples/demo/counter.rs`; a counter calls another as
itself, and `calls --count <n>` times those calls. The demo also
registers the [workspace](#the-workspace-actor), given a sandbox where
`Sandbox::open(Isolation::Sandboxed)` gives one, and two stand-ins that
call it: `examples/demo/realm.rs`, the actor `realm/local`, which sets
reaches, answers for the realm's secrets with one made-up secret and
answers waits as a chief; and `examples/demo/agent.rs`, the actor
`agent/<n>`, which makes `actors/agent/<n>.fs/` as it starts, runs
scripts and tools, reads and writes files, and keeps each `Finished` it
is told. `demo --link` runs the agents in a Node host instead, its child
(`examples/demo/link.rs`): the TypeScript agent of
`hosts/node/src/demo.ts`, reached through the realm's `tell` and, as a
client, through `watch` (`examples/demo/watch.rs`), which follows an
agent's event. Its route table lets the realm and an agent call each
other and a client call either.

With `--realm-actor` before the command, the demo runs the real realm
and the real agent together. The Rust host runs the
[realm actor](#the-realm-actor), the workspace, the
[profile and the library](#the-profile-and-library-actors)
(`examples/demo/realm_actor.rs`). Its Node child runs the
[agent actor](#the-agent-actor) (`examples/demo/agent_host.rs`). The
Rust host starts the workspace and the realm, and waits until the
realm's Hand, agent 1, is live before it boots. The route table in
`agent_host.rs` lets a client call the realm and an agent, the realm
and an agent call each other, the workspace, the profile and the
library, and those three call the realm and an agent. The child runs
`hosts/node/src/agent/demo-host.ts`, which serves `agent` on a faux
model that the line it is given drives. `HAMLET_DEMO_FAUX` names a
faux script the model answers from, and `HAMLET_DEMO_SIGN_IN=1` lists
two providers on loopback to sign in to. `demo-host.ts` and
`hosts/node/src/main.ts` share `hosts/node/src/entry.ts`, which takes
the link from the Rust host and serves the kinds until the Rust host's
goodbye.

The `realm` lines of `realm_actor.rs` act as the chief. They found a
hamlet, hire, say a line, answer a chief's waits, end an agent, lay
roads, and set budgets, review limits, modes, folders, allowances,
nicknames, the charter, models and prices. `realm live <n>` waits
until agent `<n>` is live, and `realm world` prints the world a client
reads. The `agent` lines act as an agent through `forward`, an action
that `demo-host.ts` alone registers and only the program may call,
which sends the line's message as that agent. They tell, ask,
reply, ask a chief, object, agree roads, start and end workers, ask
and charge, run commands, read copies and keep a credential.
`agent <n> inputs` and `agent <n> await` do not use `forward`. They
read the inputs in agent `<n>`'s transcript through `taken`, another
action of the demo's. The
`pi` lines of `agent_host.rs` make a client's chat calls on an agent,
follow its chat, print its entries or the Node host's process id, and
copy a file into its filesystem. `pi die-after <action>` has the Node
host kill itself once the realm answers the agent's next call of
`<action>`, before the agent reads the answer. The `realm` and `agent`
lines print `<name> <number>` lines. All three kinds of line work one
per line under `demo --realm-actor serve`, because a one-shot command
exits before a hire's steps finish.

The `faults` feature of `hamlet_actors` makes a realm action panic when
`HAMLET_REALM_PANIC` names it. `realm/act.rs` holds the fault points
`called/<action>`, as an act begins, and `committed/<action>`, after
its commit. The `profile` and `library` lines of
`demo --realm-actor`, in `examples/demo/profile.rs` and
`examples/demo/library.rs`, reach the real profile and library through
the realm: as the chief (`tag`, `revert`, `edit`, `purge`, `dream`), as
an agent's `create_scholar` (`profile make`), or as a hire's own agent
through `forward` (`fill`, `save`, `shelves`, `offer`). `tests/demo.rs` starts the demo as its
lanes do, and `hosts/node/test/link/` drives the linked demo and
`demo --realm-actor`.
`tests/local.rs` tests the host,
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

`cargo xtask killkit` installs the Node host's packages, builds the
native module and the demo, and runs `hamlet_killkit`. Its scenarios are rows,
one file of them per actor in `src/scenario/`, which `all()` in
`src/scenario.rs` lists: the commands that set up, the fault point or the time to kill at,
the commands after each restart, and what each restart must show.
`workspace-command` kills the host while a command answered `Running`
still runs, and checks after each restart that the command did not go
on, that its row reads cut off and that the agent was told the cut-off
end once (`agent.finished` is 1).

Each row names the demo it starts, so `cargo xtask killkit --runs 12`
runs every row twelve times in one invocation. A row named on the
command line runs alone. `Demo::Local` starts `demo serve`. `Demo::Linked`
starts `demo --link serve`. Under it, `link-kill-node` has an agent kill its own Node
host (`Kill::NodeHost`), and `link-kill-rust` and `ts-schedule` kill
the Rust host with the Node host running. `Demo::RealmActor` starts
`demo --realm-actor serve`, the real realm and the real agent, with a
faux script when the row names one. Its setup lines may wait with
`until <n> <count> <words>` until agent `<n>`'s conversation holds
`<count>` entries that begin with `<words>`. Eight rows run there:

- `realm-say` kills the demo host at `committed/say`, after the realm
  commits a chief's line to the mayor and before it answers.
- `realm-tell` has the Node host kill itself once the realm answers
  the Hand's `tell` to the mayor, before the tool's result is kept.
- `realm-objection` kills the demo host at a random time after a
  hire's objection and its mayor's letter on that task.
- `agent-mid-turn` has the Node host kill itself in the agent's `edit`
  of a file of its own, after the file is written and before the
  result is kept.
- `agent-timer` kills the demo host at a random time after the agent
  sets a timer 20 s ahead, and starts it again after the timer is due.
- `chat-compact` has the Node host kill itself once the realm answers
  the `tell` the model asks for after a compaction's summary.
- `chat-new` kills the demo host at `called/open_task`, as the
  reflection that `/new` asks of the session it left opens its task.
- `chat-retry` kills the demo host at `called/open_task`, in the
  mayor's `/retry` of a turn that its hamlet's spent budget failed.

The rows are in `src/scenario/realm.rs`, `src/scenario/agent.rs` and
`src/scenario/chat.rs`.
`cargo xtask idle` is `xtask/src/idle.rs`. It reads `/proc` for the
resident memory of a process tree and the growth of each thread's
`voluntary_ctxt_switches`.

## The workspace actor

The server does not use it. `hamlet_actors` holds Hamlet's actors on the
actor host: the workspace, and the [realm](#the-realm-actor). The
workspace is the kind `workspace`, in `src/workspace.rs` and
`src/workspace/`: one actor per machine, keyed `local`, that runs the
agents' commands and file changes under the sandbox, within each agent's
reach (C18 to C21). `hamlet_messages` holds the calls between actors
and their answers, one module per actor that takes them: `workspace`
(with `workspace/reach.rs`, `workspace/files.rs` and
`workspace/refusal.rs`, the refusals by family), `realm` and `agent`.
Each crosses the host as CBOR. A command is answered `ended`,
`running`, `waits_for_chief` or `refused`, and a file change the same
without `running`; a refusal holds a reason and its words. `address.rs` holds the actors as parties,
with their kinds and keys, and `sender.rs` the check of a caller.
`fixtures/` holds the bytes of each message, and
`tests/fixtures/main.rs` rewrites a fixture whose bytes changed and
fails once, so the change is committed. `fixtures/stored/` holds each
case of each type a row keeps and is never rewritten; `lib.rs` says how a message
may change.

The realm, the actor `realm/local`, sets each agent's reach
(`SetReach`), answers calls that wait for a chief (`ChiefAnswer`) and
may cancel any agent's commands (`CancelAgent`). An agent, the actor
`agent/<n>` keyed by its `AgentNumber`, calls for its own tools:
`ListTools`, `Run`, `CommandStatus`, `Cancel`, `ReadFile`, `Search`,
`Find`, `WriteFile` and `EditFile`.
Each message is a `Message`, which names the party that takes it and
the kind that sends it. A handler first reads its sender from the
caller the host stamped, with `sender::sender`, and any other caller
fails with the error `message.wrong_caller`. An agent's call is for
the agent that made it. The workspace calls the realm with `Secrets`
and `NeedsChief`, and an agent with `Finished`. In this repository
the agent is the [agent actor](#the-agent-actor) or the demo's
stand-in, and the realm is the realm actor or one of the demo's
stand-ins. The host gives the workspace its
`Sandbox` as a value of `HostOptions`, and the workspace does not start
without one; `workspace::Options` there sets how long a call waits for
its command and how much saved output an agent keeps.

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
answers `Ended` with the command's end if it ended by then, else
`Running` with its id. Before it answers `Running`, its row becomes
`running_later`, as does a run a chief allowed: the workspace calls the
agent once with `Finished` when the command ends; a failed call is
logged and dropped. The actor stays awake while a command runs, and
sets the actor's run wake to the latest time limit of such commands,
so a host that stopped cleanly starts it again by then. A command whose
host stopped or was killed keeps its row running, and the next start
of the workspace marks it cut off (`run::CUT_OFF`). A `running_later`
one becomes `cut_off_later`, and the start calls its agent with
`Finished` and `Exit::CutOff`, again while the link to the agent's host
is down, for up to a minute (`RETELL_WHILE_LINK_DOWN`). Once the agent answers, the
row is `cut_off`; a failed call is logged, and the next start calls
again. An ended reach leaves its agent's rows untold.
`CommandStatus` answers by command id, for the agent's own commands. A
repeat of a call with the same id and request answers from its row, or
waits with the first call while the command runs. The same id with
another request fails with the error `message.id_reused`, once the
request's shape has passed: a request refused for its shape is
answered `refused` whatever its id.

`workspace/run/output.rs` keeps a command's output, the realm's secrets
taken out as it streams. Its end carries the tail, the last 2000 lines
or 50 KiB. Once the output passes the tail, all of it goes to a spill
file, `actors/workspace/local.output/<agent>/<command>.log`, up to
512 MiB (`SPILL_CAP`), which the end's `Cut` names and the agent may
read. An agent's folder holds at most 1 GiB (`Options::saved_output`):
before a command opens its file, the files of that agent's finished
commands go, least recently written first, until the rest is under the
limit. A running command's file is never removed.

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
otherwise it ends not run, as `FileChanged`. Its row then holds the
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
asked; else it does not run, as `ReachChanged`. A change also needs
the file to still hold what hashes to what the chief was shown, else
it does not run, as `FileChanged`. An allow's scope is in the reach
the realm sets before it answers, not in the answer. The agent hears how the call ended with
`Finished`. A wait stays through a restart.

`Cancel` ends the calling agent's running commands, and `CancelAgent`
the named agent's: one call's, one conversation's or all of them, as
`Exit::Cancelled`. Each answers once every command has stopped, and
ends no wait. A `SetReach` to `Reach::Ended` ends
the agent's waits as lapsed and tells the agent of each, cancels all its
commands, ends what they left running under Seatbelt, and removes its
saved output and a worker's scratch folder.

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
`waits.rs`, `cancel.rs`, `workers.rs`, `tools.rs` and `realm_folder.rs`. `hamlet_messages/tests/fixtures/main.rs` checks the messages.

## The agent actor

An agent, the actor `agent/<n>` keyed by its `AgentNumber`, runs in the
Node host, in `hosts/node/src/agent/`. It is pi-durable's actor wrapper,
ported to `hosts/node/src/pi/`, composed in `actor.ts` with actions of
its own: the realm's `submit`, `place`, `fill`, `copies`, `retire`,
`erase`, `read_inbox`, `take_inbox`, `drop_inbox` and `on_task`, the
workspace's `finished`, the profile's `moved` and `remove_path`, the
library's `shelves_changed`, and `pi.wake`, the alarm of its timers. Each action checks
its sender, reads its message with the lenient readers of `messages/`,
which type every message the agent takes or sends, one file for each
actor it hears from or calls, and answers `null`,
but `remove_path`, which answers `removed` or `later`, `retire`, its
branch's tip, `read_inbox`, a part of its inbox, and `on_task`, yes or
no.
An input is taken once under its id.

At each wake `actor.ts` builds the agent's parts from its peers: the
workspace, the realm and its own filesystem. `turn.ts` holds the rules
the parts share. `callOut` stamps every call out with its id
(`t<task id>` for a tool's call), its conversation and the turn's task.
It words a refusal or a failure for the model. A call that fails after it
may have changed something answers with today's `tool_interrupted` error,
under the name of the tool the model called.
pi-durable runs a tool cut off by a crash again at the next start, with
the same task id, so the call goes out again under the same id and the
workspace or the realm answers with what it kept.

- `spend.ts` is the model gate. Every provider request asks the realm's
  `may_spend` before it is sent, and is charged after it ends, under
  `m<uuid>`.
- `workspace-tools.ts` offers the project's files, `bash` and the tools a
  chief added by hand, each one call to the workspace. A command still
  running after the workspace's 5 s ends its tool with `running`, and its
  end arrives later as `finished`, a new input.
- `own-files.ts` serves the same file tools on the agent's filesystem,
  `actors/agent/<n>.fs/`, for a path at or below its root. `filesystem.ts`
  declares its interface, `AgentFilesystem`, and `native-filesystem.ts`
  opens the folder through the native module. An own `edit` is the one
  tool that is not run again. After a crash, the model is told that the
  edit was interrupted and may have partially run.
- `society-tools.ts` holds the society tools in today's names,
  descriptions, parameters and result words, each a row of `TOOLS` that
  sends `Tool<Act>{call, act}` to the realm and words its answer, which
  names the agents it acted on. `society.ts` offers each agent its rows,
  as its kind, its place and its conversation allow, and replies to a
  question. `submit_skill` offers the skill's files to the library first.
- `inputs.ts` words each input a turn is given as the model reads it: a
  chief's line, a letter, a question, an answer and a worker's answer,
  which goes to the conversation it names. `parcels.ts` fetches a
  letter's or a question's files from the realm in parts and saves them
  in the agent's `inbox/`, numbered as today.
- `branch.ts` keeps the agent's filesystem and its branch of the
  profile: a fill writes the files at the branch's tip, a save after a
  turn sends what changed since, and a move from the profile pulls
  what the branch holds now, never over a file the agent changed. A
  removal from the profile, as a purge sends, removes the path whether
  or not the agent changed it.
- `retire.ts` answers the realm's calls of an ended hire's agent:
  `retire` stops its turn and saves its files, and `erase` removes its
  filesystem and asks the host to destroy the agent.
- `inbox.ts` answers a merge's move of the absorbed hire's `inbox/`:
  `read_inbox` gives it in parts, `take_inbox` writes each file under
  `inbox/from-hire-<n>/` once its last piece arrives, with no turn, and
  `drop_inbox` removes what was taken.
- `tasks.ts` keeps each input given under one of the realm's tasks and
  when it ended, and answers `on_task`: whether an input of one of a
  chief's tasks is still to end or ended within the protected minutes,
  which the realm asks before a lead ends or merges the hire.
- `timers.ts` is `wake`, a pi-durable background task that admits one
  input at its time.

A chief's chat with an agent is a list of sessions, each a pi-durable
conversation. `sessions.ts` keeps them in the agent's `chat` table, with
the one that is current. `chat.ts` takes a client's chat calls, each from
a client only through `askedBy`. A sign-in is the Hand's alone, and any
other agent answers `login` with `not_the_hand`.

- `commands.ts` runs `/new`, `/resume`, `/retry`, `/stop`, a rewind and
  `/reflect`, with today's notices and refusals.
- `transcript.ts` answers `read_chat` and `read_history` and sends the
  `chat` events a client follows, numbered within each wake.
  `chat-items.ts` folds a session's entries into the items a client
  reads, as today folds them.
- `copies.ts` keeps the realm's copies of the charter, the model and the
  settings, and the character the profile's files carry. The realm's create input carries the copies it
  holds for the agent, and the create keeps them in the transaction that
  writes the agent's row, so a first turn runs on them. Each wake asks
  for newer ones with `read_copies` without waiting, and the realm sends
  a newer one with `copies`.
- `prompt.ts` builds the system prompt before every model call from the
  copies, memory, the list of skills, the library's shelves and the
  workspace's tool list, each cut to its limit. The place, realm and
  scholars sections are the realm's, read with one `read_place` per
  request; a request the realm does not answer fails.
- `compaction.ts` sizes compaction for the model that runs, and asks once
  more after an answer cut off with nothing in it. `reflect.ts` asks for a
  reflection every `reflect_every` chief turns and when a chief leaves a
  session.
- `failed.ts` writes a failed turn's notice and tells the realm
  `turn_failed`.
- `models.ts` gives each model call the realm's `credential` and the
  copy's price (`agentModels`). The host keeps a list of the secrets it
  holds, the realm's secret list among them, and `models.ts` scrubs them
  from what a client reads. `modelList` answers a client's
  `read_models`, and `checkModel` its `check`, which asks a model for a
  hello outside any conversation and charges nothing.
  `login.ts` runs a sign-in step by step and gives the credential to the
  realm's `keep_credential`.
- `claude.ts` adds the provider `claude`, which runs Anthropic's program
  through its Agent SDK in the realm folder's `claude/`.
  `claude-query.ts` and `claude-history.ts` load at its first call.

A worker or an errand runs its brief's one turn (`brief.ts`) and tells
the realm its end with `worker_done` or `errand_done`. An errand is
offered the built-in tools its brief names: a dream every one its
hire's lead has not turned off, and a librarian's call or a review none.
A worker is offered the built-in tools its brief names, or all of them
when it names none.

A tool's name, parameters and descriptions are today's server's, word
for word, except `wake`, because today's server has no timer an agent can
set. `wake` lets an agent come back to its work at a later time without
holding its turn open. The timer's input starts a new turn.

`hosts/node/test/` tests the agent on fake calls (`agent-testing.ts`) in
`agent.test.ts`, `spend.test.ts`, `tools.test.ts`, `society.test.ts`,
`timers.test.ts`, `inbox.test.ts` and `tasks.test.ts`, the chat in the `chat-*.test.ts` files, and
`messages.test.ts` checks each message the agent
writes or takes against `hamlet_messages`'s fixtures.
`test/native/agent-actor.test.ts` runs the agent on the native host.

`demo --realm-actor` runs the agent in its Node host against the real
realm, workspace and library, as [the actor runtime](#the-actor-runtime)
describes. The realm creates every agent with its create input: the
Hand and each hire by a hire's steps (`realm/hire.rs`), a worker by
`realm/workers.rs` and an errand by `realm/workers/errands.rs`. The
realm's first start begins the Hand's steps (`realm/chiefs.rs`). The
demo's `realm say` gives an agent a chief's line. The kill kit's rows
on `Demo::RealmActor` run there.

## The realm actor

The server does not use it yet. The realm actor, `realm/local` in
`hamlet_actors/src/realm.rs` and `src/realm/`, keeps the realm in its
own file, whose tables are in `realm/schema.sql`: the chief and the
hamlets, the tree of hires, workers and errands, laid and agreed
roads, letters and the parcels of files they carry, tasks, charges,
allowances, waits and objections, the documents and settings, the
steps of a hire, a worker, an errand, a review and a parcel, and the
record of what was decided. It also holds `credentials.json` in its
folder. `realm::factory` registers it, and `realm::Options`, a value
of `HostOptions`, sets how long a line or a letter to a hire still
hiring waits for its steps and how long a lead's review may take.

Each action is an `Act` in the module of what it changes, run by
`act::exec`. `load` reads rows, `decide` gives a `Decision` from what
was read and nothing else, and `answer` builds the answer once the
changes are committed. An act that only reads decides with
`act::Unchanged`. `exec` looks up the action's marker (`marker.rs`) by
its request id or tool call id, so a repeat gets the first answer;
opens one transaction; and commits the decided changes with the
marker. A society tool the hamlet's limits reserve for a chief is
kept in a `reserved` wait instead (`oversight/reserved.rs`), and runs
once, as its caller, when a chief allows it; a denied one counts
toward its task's refusals, as a tool's wait does. A `Decision::WaitFor`
waits, outside the transaction, for a hire's steps to end, and the
action runs again. `callers.rs` says who may call each action: a
chief's request, a society tool of an agent (refused while the agent
ended, and to a number that is no member, such as an errand's), a lead
(the chief, or an agent with a lead's powers), or the workspace. Any
other caller fails with the error `message.wrong_caller`, and an id
used again with another request with `message.id_reused`. `Reply`,
`Charge` and `NeedsChief` keep no marker: a repeat writes nothing, as
a question keeps its first answer and a charge or a wait is kept once
per call. Three handlers run outside `Act`: `KeepCredential` and the
settings' `EditSettings`, which write `credentials.json` before they
commit, and `FetchPart`, which reads a parcel's file from the realm's
store; `tests/realm/writers.rs` names them.

A change is a value of the `Change` enum of the module that owns its
tables, and only that module's `apply` writes them;
`tests/realm/writers.rs` fails otherwise. `change.rs` holds the
transaction and `Affected`, what a commit moved: the agents whose
place or reach changed, the steps begun, the parts of the copies an
agent reads, and the parts of the world a client sees. After the
commit, the steps begun start, and `calls.rs` pushes each moved place
(`place/push.rs`), reach (`reach.rs`, through `versions.rs`, which
keeps each version wanted and in force) and copy (`documents.rs`), and
makes the action's deliveries, such as `Submit` to an agent, a notice,
a worker's answer to its starter, or `ChiefAnswer` to the workspace,
before or after the answer. `peer.rs` is the one way to another actor.
`lanes.rs` keeps the order of the deliveries to one agent. A commit
lines up its deliveries while its transaction is open, and each waits
for the deliveries to the same agent lined up before it, so an agent
takes them in the order their acts committed. Deliveries to different
agents go at the same time, except that one delivery reaching several
agents, such as a task's held letters and answers (`Delivery::Held`),
holds each of their lanes until it is done. A failed call is logged,
and `Say` answers whether its line reached the agent; what was
committed stays.
Some deliveries are owed, because no repeat by their sender carries
them: a line a review kept (`Delivery::Kept`), a parcel's refusal
notice (`Delivery::ParcelRefused`), a wait's notice (`Delivery::Notice`,
which also runs a reserved act a chief allowed, under the request id its
first run asked with) and a line whose input is a notice, such as the
librarian's answer or verdict. After a failure in passing,
`calls.rs` makes an owed delivery again on the steps' schedule
(`steps::retried`: 1 s, then doubled up to 60 s) until it is taken,
its callee refuses it, its receiver has left or the realm stops, and
keeps the realm awake meanwhile.

A hire is one transaction, then six steps in `hire.rs`, each recorded
done by a transaction of its own (`steps.rs`). The first four call
another actor: the profile's actor, created from the realm's character
for a blank hire and started for a scholar's; its branch `hires/<n>`,
at the version the hire names when it names one; the agent made with
its place, its task and its copies; and the agent's filesystem filled,
which the agent pages from its profile. A version the profile lacks
fails the hire for good. A hire's end begins a row of its own
(`drop_parts.rs`) that drops its unfinished save and offer parts in the
profile and the library, and a `retire` row (`retire.rs`) in which its
agent saves its files to its branch. A merge (`merges.rs`), a chief's
`/merge` or a lead's `merge_hires`, ends the absorbed hire and begins
a `merge` row: the retire, the profile's merge of the absorbed hire's
branch into the kept one's, the kept hire's fill, and the move of the
absorbed hire's `inbox/` into the kept hire's `inbox/from-hire-<n>/`
through both agents, which gives no turn. What of the inbox an agent
refuses to move stays with the absorbed hire, and the merge stands; the
hire's erase is refused while the kept hire is not erased. A `/restore`
brings an ended hire back (`tree::coming_back`), and its hire's steps
go on if its end cut them off; of a hire merged away, it begins a
`split` row that takes the merge out again. An `/erase` (`erase.rs`)
begins an `erase` row, which drops the hire's branch once every merge,
split and retire of it has ended, and removes its agent.
`Reach` commits the hire's first reach as wanted, which the push after
that commit sends to the workspace, and `Live` calls no actor: the
transaction that records it makes the hire live and closes the row. A
worker (`workers.rs`), an errand (`workers/errands.rs`), a review
(`reviews.rs`) and a parcel (`parcels.rs`) have steps of their own. A
step that fails in passing is tried again on the steps' schedule
(`steps::retried`). A step its callee refuses fails for good, and
`end_failed` ends its row: a hire or a worker as failed, an errand as
one that gave no answer, a review as failed and a parcel as refused.
At the realm's start, each step row not closed goes on from its last
done step, and `End` closes the row.

The rest is by module: `chiefs.rs` founds a hamlet, with
`chiefs/hamlet.rs` its name, site, mode and folder, and its removal,
by a chief or the Hand's `remove_hamlet`, its return and its erasure, `chiefs/limits.rs`
and `chiefs/tier.rs`; `tree.rs` keeps the tree, with `Hire`, `End` and
`Seat` in `tree/lead.rs`, the tools a lead turns off in
`tree/tools.rs`, and nicknames and the models chosen for an agent or
a hamlet in `tree/choices.rs`; `workers.rs` starts a lead's workers, which answer
their starter once and end with it, and `workers/errands.rs` runs
errands, short-lived agents outside the tree, such as a review;
`roads.rs` says which agents have a road, `laid.rs` keeps the roads a
lead lays, and `society.rs` the roads two mayors agree; `say.rs` is a
chief's line, which `reviews.rs` holds for the lead's review when the
limits ask for one; `letters.rs` letters, questions and answers, and
`parcels.rs` the files a letter sends, copied into the realm's store
(`parcels/store.rs`) by hash and fetched by the receiver in parts; `tasks.rs` keeps what a
line or an agent's turns are charged to; `budget.rs` answers
`MaySpend` and keeps `Charge`, and `allowance.rs` the allowance a lead
or a chief gives a hire, with `ReadBudget`; `oversight.rs` keeps
grants and trust, and `oversight/asking.rs` an agent's questions and
objections to a chief, which hold its spending and letters on that
task until a chief rules; `waits.rs` keeps the waits for a chief of
every kind and the refusals counted on a task, and `waits/answer.rs`
a chief's answer to one;
`documents.rs` and `settings.rs` keep the charter, the catalogue and
the settings and push each agent its copies; `profiles.rs` keeps each
profile's name and label, makes and reads profiles for
`create_scholar` and `read_scholar`, keeps `edit_scholar`, `tag` and
`revert` for a chief, and answers a chief's own change from the
profile's answer, and `profiles/lifecycle.rs` a profile's fork,
archive, return and erasure, which erases its hires first, and its
place in the city; `dreams.rs` runs a chief's dream as a step row, one
at a time in the realm and none while its profile is purged: the
profile checks `main`, and a hire's branch, out under `dreams/<n>/`,
an errand dreams in main's checkout with the hire's network
(`Reach::Dream`), and the profile commits what it changed when it
ends; `prompt.rs` renders the place, realm and scholars sections of an
agent's prompt for `ReadPlace`, with the secrets taken out;
`librarian.rs` runs `ask_librarian` and `submit_skill` as
errands, reads an offer from the library before the librarian judges
it, and brings the verdict to the library; `secrets.rs` holds
`credentials.json`, written whole with mode 0600, and answers
`Secrets`, the list a scrub reads, and an agent's `Credential`;
`record.rs` keeps
the decisions, and `agent_name` names an agent as chiefs and agents
are shown it, as `hire#3 (Mira)`. `world.rs` keeps the world a client
reads with `ReadWorld` as a copy in memory, rebuilt after each commit that
changes it, and `versions.rs` numbers those commits in the file.

`tests/realm.rs` runs the realm on a host with stand-ins for the
workspace, the agents, the profiles and the library
(`tests/realm/stand_ins.rs`, `profile_stand_in.rs`). They keep what they were sent,
and a test can hold a call, fail it once or fail it every time for one
agent. A test calls the realm as a client, and as an agent or the
workspace through that stand-in's `forward`. The files in
`tests/realm/` group its tests by subject.

## The profile and library actors

The server does not use them either. `hamlet_actors/src/profile.rs`
and `src/profile/` are the kind `profile`, one actor per profile keyed
by its `ProfileId`: the only writer of the repository
`scholars/<n>.git` and of the store beside it. `main` is what the
profile knows, `hires/<agent>` each hire's branch, and annotated tags
its versions. `versions.rs` tags, reverts, edits, purges and erases,
and reads the files of a revision (`Read`); `hires.rs` makes a hire's
branch at the version it is hired at, pins it, and takes `main` into
it; `files.rs` gives an agent the files of its branch in parts
(`Files`), takes its changes back (`Save`) and drops an ended hire's
unfinished parts; `merges.rs` merges one hire's branch into
another's, from the same profile or from another profile's branch
read out in parts, takes a merge out again and drops a branch;
`dreams.rs` checks
`main`, and a hire's branch, out for a dream and commits what the
dream changed when it ends. `src/library.rs` and `src/library/` are
the kind `library`, keyed `local`: the only writer of `library.git`
and its checkout `library/`, which every agent reads. An agent offers
a skill in parts, the realm reads it (`ReadOffer`), the librarian's
verdict, sent by the realm, shelves it, and unshelving takes it off; each change moves the list's version and is told to the
agents.

Both take every call through `src/repo.rs`, the one way to a
repository and the store beside it: `admit` checks the caller, and an
action runs a closure through `Call::read`, `Call::once` (once per id,
alone, the store's marker written in the same commit), `Call::fresh`
(the id checked before the action calls other actors, as a purge
does) or `Call::refuse`. A unit a kill cut off is finished by its
repeat; a start finishes a cut-off purge, removes stale locks and
writes `library/` again. The parts of a save or an offer come in
order, and their rows outlive a sleep and a restart.
`repo/git.rs` reaches the repository through `hamlet_git`,
`repo/store.rs` the store, `repo/checkout.rs` a checkout on disk, and
`repo/parts.rs` the files sent in parts. `hamlet_git/src/purge.rs`
takes a path out of a repository's whole history.

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

`hamlet_server --actors` serves the same client protocol from the actors
instead (`front.rs`, Unix only). The `node-host` feature compiles the
`front` target, `tests/front/`, which starts that server with its Node host;
`cargo xtask node` runs it once the Node host's packages and native module
are in place.

| Module | Responsibility and interface |
|---|---|
| `front.rs`, `front/` | The door to the actors behind `--actors`: `host.rs` opens the folder with the realm, workspace, profile and library actors and the agent in a Node child, and a route table that lets a client reach only the realm and an agent; `front.rs` claims the folder, publishes `server.json` once the realm answers and starts the agents while it serves, checks each connection's token and every message with `clients/check.rs`, applies one connection's requests in the order sent, a stop ending the waits of those sent before it, and an answer naming its wait once the stops sent before it are answered, sends no `State` older than the last a connection was sent, and carries frames through `clients/connection.rs`; `route.rs` turns a client message into what it becomes, `call.rs` (`Calls`) is the one executor of the actors' calls, with their deadlines and error words, `hub.rs` holds an agent's `chat` subscription while a watch, an owed reply or a busy period needs it and settles a line's reply when its input ends, `frames.rs` maps chat events and answers to frames, `commands.rs` and `commands/parse.rs` run the chief's `/` commands from today's table, `words.rs` holds their words, and `view.rs` renders the `State` from the realm's world |
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
| An actor's address, the name rule, and who made a call | `hamlet_address/src/lib.rs` |
| Calls between actors | `hamlet_host/src/local/call.rs` |
| Which caller may call or make an actor of which kind | `hamlet_host/src/route.rs`; the check, `admit`, in `hamlet_host/src/local/call.rs` |
| Making an actor with an input | `hamlet_host/src/local/create.rs` |
| What an actor reaches of its host: its address, the realm folder, the program's values, and its calls, creates and get-or-starts as itself | `hamlet_host/src/local/access.rs` |
| Where each actor kind runs | `hamlet_host/src/directory.rs` |
| The link to the Node host: its child, handshake, frames, and calls each way | `hamlet_host/src/child.rs`, `link.rs`, `link/`; the stand-in Node host `hamlet_host/examples/link_child.rs` |
| The agent actor: its actions, its parts and the stamp of its calls out | `hosts/node/src/agent/actor.ts`, `turn.ts`; [the agent actor](#the-agent-actor) |
| A tool the agent offers the model | `hosts/node/src/agent/workspace-tools.ts`, `own-files.ts`, `society-tools.ts`, `society.ts`, `timers.ts`; all but `wake` in the words of today's server |
| An input as the model reads it, and a letter's files in the inbox | `hosts/node/src/agent/inputs.ts`, `parcels.ts` |
| A chat command, a client's chat call or a chat event | `hosts/node/src/agent/chat.ts`, `commands.ts`, `transcript.ts`, `chat-items.ts`, `sessions.ts` |
| What the agent's model reads, and the model it runs on | `hosts/node/src/agent/prompt.ts`, `copies.ts`, `models.ts`, `compaction.ts` |
| A sign-in, or the provider `claude` | `hosts/node/src/agent/login.ts`, `claude.ts` |
| A message the agent takes or sends | `hosts/node/src/agent/messages/`, one file per actor; `hosts/node/test/messages.test.ts` checks them against `hamlet_messages/fixtures/` |
| The Node host's entry and the demo's TypeScript kinds | `hosts/node/src/main.ts`, `entry.ts`, `demo.ts`, `actor/registry/link.ts`; the linked demo in `hamlet_host/examples/demo/link.rs` |
| The demo's real realm and agent: its kinds, route table, verbs and faux model | `hamlet_host/examples/demo/realm_actor.rs`, `agent_host.rs`; `hosts/node/src/agent/demo-host.ts` |
| A model request: its key, its price and its scrub; a client's model list and model check | `hosts/node/src/agent/models.ts` (`agentModels`, `modelList`, `checkModel`), `chat.ts` (`askedBy`, `read_models`, `check`) |
| What a TypeScript actor can do: call another actor with a deadline, its address and caller, `get`, `all` and `run` on its rows | `hosts/node/src/actor/actor/calls.ts`, `config.ts`; `registry/native.ts`, `runtime.ts`, `napi-runtime.ts`; `common/database/`; the native side in `hamlet_actor_node/src/address.rs`, `actor_context.rs` |
| The native module's TypeScript declarations | `hamlet_actor_node/index.d.ts` and `index.js`, written by `cargo xtask declarations`, never by hand |
| What the host reports on `Host::events` | `hamlet_host/src/event.rs` |
| A one-shot timer's tries, and the timers set aside after them | `hamlet_actor_core/src/actor/schedule.rs`; `Host::set_aside_timers` and `Host::rearm_timer` in `hamlet_host/src/local.rs` |
| A kill scenario | the actor's file in `hamlet_killkit/src/scenario/`, a new actor's file listed in `all()` of `src/scenario.rs`, and a fault point with `hamlet_host::fault::point` |
| A message between actors, or its fixture | `hamlet_messages/src/`, then `cargo test -p hamlet_messages` rewrites `fixtures/` |
| An agent's reach on the workspace, and the gate | `hamlet_actors/src/workspace/reach.rs` (`apply`, `gate`, `placement`); [the workspace actor](#the-workspace-actor) |
| A command's life: its row, `Ended` or `Running`, `Finished`, its status, a repeat, a cut-off command | `hamlet_actors/src/workspace.rs`, `workspace/run.rs`, `workspace/calls.rs` |
| A command's tail and spill file | `hamlet_actors/src/workspace/run/output.rs` |
| The workspace's reads, writes and edits | `hamlet_actors/src/workspace/files.rs`, `files/`; the replace by rename in `hamlet_sandbox/src/env.rs` and `hamlet_os/src/fs/walk.rs` |
| Calls that wait for a chief on the workspace | `hamlet_actors/src/workspace/pending.rs` |
| Cancelling an agent's commands, and an ended reach | `hamlet_actors/src/workspace.rs` (`cancel`, `end_agent`) |
| Workers' scratch folders on the workspace | `hamlet_actors/src/workspace/workers.rs` |
| Tools added by hand on the workspace | `hamlet_actors/src/workspace/tools.rs` |
| How the realm actor runs an action: the marker, the transaction, the calls after it | `hamlet_actors/src/realm/act.rs`, `change.rs`, `calls.rs`; [the realm actor](#the-realm-actor) |
| The order of the realm's deliveries to one agent | `hamlet_actors/src/realm/lanes.rs` |
| An owed delivery the realm makes again after a failure | `hamlet_actors/src/realm/calls.rs` (`Delivery::owed`, `again`), `steps.rs` (`retried`) |
| The realm actor's hires, ends and steps | `hamlet_actors/src/realm/tree/lead.rs`, `hire.rs`, `steps.rs` |
| The realm actor's roads, letters, budgets and waits | `hamlet_actors/src/realm/roads.rs`, `laid.rs`, `letters.rs`, `budget.rs`, `waits.rs` |
| The place, realm and scholars sections of an agent's prompt | `hamlet_actors/src/realm/prompt.rs`; the agent's side in `hosts/node/src/agent/prompt.ts` |
| A profile's versions, edits, purge and erase | `hamlet_actors/src/profile/versions.rs`; [the profile and library actors](#the-profile-and-library-actors) |
| A hire's branch, its pin and taking `main` in | `hamlet_actors/src/profile/hires.rs` |
| The files an agent is given, and its saves | `hamlet_actors/src/profile/files.rs`, `repo/parts.rs`; the agent's side in `hosts/node/src/agent/branch.ts` |
| The profile tools, a chief's profile changes, dreams and the librarian in the realm | `hamlet_actors/src/realm/profiles.rs`, `realm/oversight/reserved.rs`, `realm/dreams.rs`, `realm/librarian.rs` |
| A dream's checkouts and its end | `hamlet_actors/src/profile/dreams.rs`, `repo/checkout.rs` |
| Offering, shelving and unshelving a skill | `hamlet_actors/src/library.rs`, `library/offers.rs` |
| An actor that keeps a repository: admitting a call, once per id, settling after a kill | `hamlet_actors/src/repo.rs` |

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
