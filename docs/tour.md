# A tour of the code

Where things are. Start with [`README.md`](../README.md) for running
Hamlet and [`AGENTS.md`](../AGENTS.md) for the checks and conventions.
Each module's own doc comments say what its files do.

## How a change travels

The Rust server owns the realm and permissions. Its Node pi harness owns
conversations and model calls. The window and CLI are its clients, over
the client protocol; neither opens the server's database nor runs an agent.

```text
Window / terminal -- client protocol --> hamlet_server -- runtime protocol --> pi
                                               |                               |
                                  realm.sqlite, profiles/*.git           harness store
                                  worktrees, sandbox                     conversations
```

The server is a Bevy app with no frame
([ADR 0015](adr/0015-the-server-runs-as-a-bevy-app-with-no-frame.md)): its
runner sleeps until an input comes and updates the app once for it
(`app/runner.rs`). A typed line becomes a request handled by the system
for its kind (`clients/handlers.rs`); a line that begins with `/` is a
command (`clients/commands.rs`); other words become a kept input of the
agent's chat (`conversations/chats.rs`). Every change an input makes is
in its one transaction, which also keeps the jobs the change needs.
`PostUpdate` keeps it (`app/follow_up.rs`), and `Last` sends the runtimes
what they are owed and the clients the state. A handler never awaits:
what awaits runs on a task whose result comes back as an input
(`app/tasks.rs`). Pi asks the server for its prompt, tool permissions and
model budget; the server checks calls again when executing them,
replayed calls included. `hamlet_client` turns `state`, `change`,
`transcript` and `turn` into Bevy entities; the world and panels read
them.

## The crates and packages

Rust crates live under `crates/`. The root `Cargo.toml` is only the
workspace, so `cargo build`, `cargo test` and `cargo clippy` there cover
every crate. `xtask/` is a workspace of its own, run as `cargo xtask`
(see AGENTS.md). `vendor/fynix` is the submodule that supplies
`bevy_fynix`, the panels' view layer; it is not edited here.

| Crate or package | Owns |
|---|---|
| `hamlet_cli` | `hamlet`: process lifecycle, terminal chat, editors and login, without Bevy; and `hamlet_launch` (`src/launch.rs`) |
| `hamlet_server` | `hamlet_server`: the realm's society, conversations, the gate, budgets, tools, clients, credentials and harness supervision |
| `hamlet_store` | The realm's database: `schema.sql`, opening and checking it, transactions, markers and jobs, the typed ids |
| `hamlet_git` | Profiles' and the library's repositories through gix, with no git program |
| `hamlet_sandbox` | What confines a command: the bubblewrap and Seatbelt policies and secret scrubbing |
| `hamlet_os` | System calls whose form differs by OS, with no C code: `fs`, `process`, `process::pty` |
| `hamlet_app` | `hamlet_app`, the window: panels, menus, chat and pointer, and the examples |
| `harnesses/pi/` | Node pi-durable/pi-ai runtime: conversations, provider calls, compaction and forks |
| `hamlet_protocol` | Both protocols' types, `client` and `runtime`; `State::apply`; recorded frames, the schema pi reads and the client schema's fingerprint |
| `hamlet_acceptance` | Real-process protocol, CLI and OAuth scenarios on scratch realms |
| `hamlet_client` | Connection, outbox, realm, waits and chats as Bevy entities, and the `Index` that finds them by protocol id |
| `hamlet_desktop` | File and link openers, editor files, dialogs, clipboard, wake locks and window icon |
| `hamlet_import` | Other programs' conversations found on the chief's machine or in an export, read as the `Imported` a client sends (C35) |
| `hamlet_update` | A release updating itself; its `check` feature, the window's alone, touches the network |
| `hamlet_assets` | Embedded assets and Bevy asset loading |
| `hamlet_realm` | The islands, the city, the harbor, hamlets, houses, roads, robots, the Hand, the library, chief, sound and camera |
| `hamlet_ui` | The widget kit: theme, styles, controls and effects, with no client state |
| `hamlet_vfx` | Visual effects |
| `hamlet_testing` | Fixtures shared by tests, including a server process on a scratch realm |
| `hamlet_workspace` | The Linux remote-workspace worker |
| `hamlet_workspace_wire` | The workspace protocol for both sides |

The workspace's `bevy` entry turns on no feature: `hamlet_app` turns on
the product's set, and every other crate names only what it needs.
`hamlet_client`, `hamlet_assets` and `hamlet_desktop` use Bevy's parts
rather than `bevy`, so `cargo test -p hamlet_assets` builds no renderer.

`hamlet_client/src/connection.rs` applies the server's frames in
`PreUpdate` (`ClientSystems::Receive`) and sends the outbox in `Last`
(`ClientSystems::Send`). `RealmPlugin` in `hamlet_realm/src/lib.rs` adds
the scene, the generator, the chief's interactions and the bridge from
client entities. The realm's steps before the world are `RealmSystems`
in `hamlet_realm/src/stage.rs`, and [`bevy.md`](bevy.md#order) says what
goes in each. The panels' steps before fynix mounts are `PanelSystems`
in `hamlet_app/src/lib.rs`. Panels live in `hamlet_app/src/screens/` and
register their dock window in `dock.rs`; they are built from
`hamlet_ui`, which reads no client state.

The Web panels are only in a macOS window built with `hamlet_app`'s
`web` feature: `dev --web` builds it with `hamlet_web_helper` beside
it, the process the engine starts for each page. Building it needs
cmake and ninja on the path. The engine, CEF, is read from `CEF_PATH`,
else from `~/Library/Caches/Hamlet/cef/<version>`, which a Web panel
fetches into when it is not there; the window starts the engine at
launch, so a fetched engine is used from the next one. Each page is
an entity of its own that `screens/web/page.rs` hangs off its panel's
node while the panel is the tab in front, since the dock builds its
nodes again as it changes.

## The store and the repositories

`hamlet_store` is the only crate that knows the database's layout.
`schema.sql` is the one schema; `schema.rs` is generated from it by
`cargo test -p hamlet_store`
([ADR 0010](adr/0010-the-server-queries-its-store-through-diesel.md)).
`Store::immediate` begins a transaction; `marker` keeps the outcome of a
request under its identity, and `job` keeps an effect to run after the
commit
([ADR 0012](adr/0012-the-realm-is-a-database-of-tables-changed-in-transactions-with-a-jobs-outbox.md)).

`hamlet_git` is the only crate that touches a profile's repository
([ADR 0011](adr/0011-profiles-are-git-repositories-agents-are-branches-worktrees-are-checkouts.md)).
It reads and writes no folder of a worktree: the server lists a
worktree's files with their blob ids and `plan` says which writes make
the worktree hold a commit.

`realm.sqlite` holds server-owned records, not pi's authoritative
transcript. Each runtime opens only its own store under
`harnesses/<instance>/`. A profile's knowledge, the Hand's included, is
in `profiles/<id>.git`, and a worktree is a folder of the realm. See
[`realm.md`](realm.md).

## The server

Paths are under `crates/hamlet_server/src/`. Modules use a file and
same-named directory rather than `mod.rs`, and the crate exports only
`serve`. The crate's first comment in `lib.rs` says in which order to
read the rest. The `faults` feature compiles the points where a test can
kill the server (`app/fault.rs`), and `harness` the tests that start pi;
the `crashes` target (`tests/crashes/`) kills the server at each point
and serves the realm again.

| Module | Owns |
|---|---|
| `lib.rs`, `app/` | `create_app`, the runner, inputs, startup and shutdown, follow-up, tasks and timers |
| `realm/` | The realm on disk: opening and claiming its folder, `Layout`, `Change`, jobs, settings, credentials and the remote workspace |
| `core.rs` | `Core`, the realm's live state as handlers reach it |
| `society/` | The society's domain functions, each taking a `Change`; `authority.rs` is the one place that says who may do what; `oversight/` the waits and the gate; `spend.rs` budgets |
| `knowledge/` | What agents know, on disk: the jobs on their own thread, edits, papers, merges, dreams, library, promotion and versions |
| `runtimes/` | The harness runtime processes, their lines, and what each is sent |
| `conversations/` | Conversations, inputs and turns, prompts, chats, dreams, imports and what follows an input's end |
| `calls/` | What a runtime asks during a turn: permissions, execution once per key, tool runs, and the society's tools |
| `clients/` | Connections, request handlers, the state and its changes, transcripts, documents and the chief's `/` commands |
| `attachments/` | Images and files on a chief's line, and the inbox (C33, S29, H30) |
| `tools/` | What agents' tools do: built-in, custom and the society's catalogue |
| `web_ide.rs` | One OpenVSCode Server per hamlet folder for the Files panel, started by `web-ide` under the sandbox |
| `terminals/` | Shells on pseudo-terminals and each agent's commands terminal |
| `providers/` | The model catalogue, provider requests and sign-ins |

## The pi harness

`harnesses/pi/src/main.ts` is the Node entry. The server starts it with
`--runtime`, `--store` and `--transport stdio`. The runtime protocol is
versioned JSON lines on stdin and stdout; diagnostics go to stderr. How
runtimes, conversations and the protocol work is in
[`harness.md`](harness.md). `runtime.ts` holds the conversations,
`extension.ts` the hamlet tools and hooks, `models.ts` the provider calls
the server admits and charges, and `claude*.ts` the `claude` provider
([ADR 0014](adr/0014-claude-subscriptions-run-in-anthropics-own-program-under-pi.md)).

Credentials belong to the server; the harness receives only what an
authorized provider attempt needs, in memory. A librarian's call and a
review have no tools, and a dream has the built-in ones.

## Where to change behavior

Search the module names above first. The ones that are not obvious:

| Change | Owner |
|---|---|
| What the chat field lists after `@`, `/`, `$` and `#` | `hamlet_protocol/src/signs.rs`; `hamlet_app/src/screens/chat/suggestions.rs` |
| An address's form | `hamlet_protocol/src/address.rs` |
| Who may do what | `hamlet_server/src/society/authority.rs` |
| The database's tables | `hamlet_store/src/schema.sql`, then `cargo test -p hamlet_store` |
| Folder, mode or tool reach | `hamlet_server/src/calls/place.rs`, `tools/builtin/environment.rs`, `hamlet_sandbox`; [`sandbox.md`](sandbox.md) |
| Terminals | `hamlet_server/src/terminals/`; [`sandbox.md`](sandbox.md#terminals) |
| CLI process discovery | `hamlet_cli/src/process.rs`; [`cli.md`](cli.md) |
| Installers and archives | `.github/scripts/package.sh`, `.github/workflows/release.yml`; [`releases.md`](releases.md) |
| Updates | `hamlet_update`, `hamlet_app/src/updates.rs`; [`releases.md`](releases.md#updates) |
| Client protocol | `hamlet_protocol/src/client/`, then `cargo test -p hamlet_protocol`; [`protocol.md`](protocol.md) |
| Runtime protocol | `hamlet_protocol/src/runtime/`, then `cargo test -p hamlet_protocol`; [`harness.md`](harness.md) |
| The Hubs and desks | `hamlet_app/src/screens/hub.rs`, `desk.rs` |
| Web panels | `hamlet_app/src/screens/web.rs`, `screens/web/`, `web_helper.rs`; [ADR 0016](adr/0016-web-panels-draw-chromium-through-cef-in-the-scene.md) |
| Agents' web tools | `hamlet_server/src/web.rs`, `hamlet_protocol/src/client/web.rs`, `hamlet_app/src/screens/web/agents.rs`, `bridge.rs`; [ADR 0017](adr/0017-agents-use-the-web-through-their-own-web-tab.md) |
| The two islands | `hamlet_realm/src/generator/land.rs`; [`islands.md`](islands.md) |
| Robot motion | `hamlet_realm/src/keeper/acts/`; [`robots.md`](robots.md) |
| Panels' colors, sizes and icons | `assets/themes/`, `hamlet_ui/src/theme.rs`, `styles.rs` |
| World colors, size or timing | `assets/palette.toml`, `world.toml`, `vfx.toml`, `ui.toml` |

## Tuning and previewing the window

Assets are embedded. The `hot-reload` feature also watches their source;
`dev --hot` builds the window with it and points it at the checkout:

```sh
HAMLET_HOME=/tmp/hamlet-try cargo run -p hamlet_cli -- dev --hot
```

The generator section of `world.toml` and existing mesh colors do not
change an already-built world. An unreadable asset reports an error and
keeps the last good values.

The examples in `crates/hamlet_app/examples/` (`ui_gallery`, `far_lands`,
`sea_bench`, `chief_sheet`, `cursor_sheet`, `sound_sheet`) write images,
sheets or timings with `--out <dir>`; `style_test` and `town_gen` open
windows. For live work use `HAMLET_HOME=/tmp/hamlet-try cargo xtask dev`;
it sets `HAMLET_VIEW=<realm folder>/window`.
