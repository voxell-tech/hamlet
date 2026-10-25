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
                                         realm.sqlite                    harness store
                                        houses, sandbox                  conversations
```

A typed line reaches `chats::converse` through `requests.rs`, whose one
match names the owner that answers each client message. Commands
change server-owned records or documents; ordinary words become a durable
harness input. Pi asks the server for its prompt, tool permissions and
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
is eight steps, `format`, `clippy`, `deny`, `test`, `acceptance`,
`harness`, `doc` and `node`, each a task of its own;
`.github/workflows/rust.yml` runs them by name, and spells out only the
Windows window's clippy. `deny.toml` at the root configures cargo deny,
which `deny` runs.

`vendor/fynix` is the submodule that supplies `bevy_fynix`, the panels'
view layer; the workspace excludes it, and it is not edited here.

| Crate or package | Owns |
|---|---|
| `hamlet_cli` | `hamlet`: process lifecycle, terminal chat, editors and login, without Bevy |
| `hamlet_server` | `hamlet_server`: realm, permissions, tools, records, credentials and harness supervision |
| `hamlet_os` | The system calls whose form differs by OS, with no C code below them: `fs`, directories held open and what is reached from them one name at a time, never through a link at that name, and the tools' walk, path text and error words; `process`, a command's process tree, started inside its limits and killed whole |
| `hamlet_app` | `hamlet_app`, the window, and the seven examples that draw the world and the panels |
| `harnesses/pi/` | Node pi-durable/pi-ai runtime; conversations, provider calls, compaction and forks |
| `hamlet_protocol` | Both protocols' types: `client` and `runtime`; `RequestId`, which holds only a hyphenated UUID; `State::apply` for a client's changes; recorded frames, the schema pi reads and the client schema's fingerprint |
| `hamlet_acceptance` | Real-process protocol, CLI and OAuth scenarios on scratch homes, through the typed `Client`, the `hamlet` command and `hamlet_testing`'s server |
| `hamlet_client` | Connection, outbox, realm, waits and watched chats as Bevy entities, the `Index` that finds them by their protocol ids, and gifts read in parts off the frame |
| `hamlet_desktop` | File/link openers, editor files, dialogs, wake locks and window icon |
| `hamlet_assets` | Embedded assets and Bevy asset loading |
| `hamlet_realm` | Land, hamlets, houses, roads, robots, chief, sound and camera |
| `hamlet_ui` | Panels, menus, chat and pointer |
| `hamlet_vfx` | Visual effects |
| `hamlet_testing` | Fixtures shared by tests: scratch folders, protocol values, faux scripts, the loopback OpenAI endpoint, and a server process on a scratch home with a raw connection to it (`send`, `until`, `answers`) |
| `hamlet_workspace` | The Linux remote-workspace worker the server's workspace-v1 client connects to |
| `hamlet_workspace_wire` | Workspace protocol v1 for both sides: one type per operation with its reply, error codes, limits, the walk order and the shared fixture |

The workspace's `bevy` entry turns on no feature: `hamlet_app` turns on
the product's set, and every other crate names only the ones its own
code needs. `hamlet_client`, `hamlet_assets` and `hamlet_desktop` use
Bevy's parts (`bevy_app`, `bevy_ecs`, `bevy_asset`, ...) rather than
`bevy`, so `cargo test -p hamlet_assets` builds no renderer.

`hamlet_client/src/connection.rs` applies the server's frames in
`PreUpdate` (`ClientSystems::Receive`), so the realm and the panels see a
frame in the update it arrives in, and sends the outbox in `Last`
(`ClientSystems::Send`). The connect task calls the `Wake` resource after
each frame it hands over, and a gift's read after each folder walked or
part read; `hamlet_ui/src/window.rs` inserts one that wakes winit's
reactive loop, so a frame is drawn at once rather than at the loop's
next timeout. `connection::Contact` is the window's standing
with its server: connecting, connected, lost or refused. `index.rs` finds
a hamlet, an agent, a wait or a chat by its protocol id, kept by the
components' hooks, and `gifts.rs` walks and reads a gift's files on
Tokio's blocking threads.

`RealmPlugin` in `hamlet_realm/src/lib.rs` adds the scene, then the
generator, the chief's interactions and the bridge from client entities.
`scene::plugin` adds the scene's nine plugins in order; examples that
only draw a scene add it themselves, and the town generator also adds
`generator::plugin` and an `Origin` for its seed. The realm's steps
before the world are `RealmSystems` in `hamlet_realm/src/stage.rs`
(`Steer`, `Aim`, `Act`, `Bridge`, then `WorldSystems`), and
[`bevy.md`](bevy.md#order) says what goes in each. In the generator,
`society::Ground` is what can be picked up and set down (a town, the
chief's place, the library), and `Trips` the letters on the roads right
now.

`hamlet_ui/src/screens/` groups the panels drawn from the client's state.
`widgets/` groups their shared controls: `feel/`, `icons.rs`, `tabs.rs`,
`platform.rs` and `tuning.rs`. The other panels live beside those folders;
`theme.rs` and `looks.rs` define their appearance. The panels' steps
before fynix mounts are `PanelSystems` in `hamlet_ui/src/lib.rs`
(`Follow`, `Bind`), in `PreUpdate`; `panel_sets` orders them, and the
apps in `testing.rs` add it, so a UI test settles in one update.

## The server

Paths below are under `crates/hamlet_server/src/`. Modules use a file
and same-named directory rather than `mod.rs`. Most domain APIs are
internal; the crate exports only `serve`, the types its process tests
seed a home with, and, with the `harness` feature, the test driver's
body and what its tests read a conversation's charges with (`BindingId`,
`usage`). Its unit tests sit beside their modules; `tests/server.rs`
runs the executable where it refuses to start, which needs no Node, and
the `harness` feature adds `tests/crashes` (a served home: crashes,
fault points, signals, remote workspaces and the society) and
`tests/harness` (the pi harness).

| Module | Responsibility and interface |
|---|---|
| `home.rs`, `claim.rs` | `Home::open` claims a path, opens its `Store` and checks the `realm.id` pair. `connection()` reads its database without the store's transactions. `Layout` names storage, credentials and harness directories |
| `store.rs` | The schema, composed from `spend.sql` and the `schema.sql` in `record/`, `harness/`, `chats/` and `society/`, and the `realm.sqlite` connection, with WAL, FULL sync and foreign keys; a store of another version is refused; `changes` counts the rows changed since it opened |
| `operation.rs` | `OperationId` identifies runtime plus call key. Owns every marker: `Store::run` commits an operation's result marker with its record effects; `Store::once` and `once` mark any outcome with its request digest, alone or in the caller's transaction, and refuse a changed request under a used key; its tests, crashes at the commit included, are in `operation/tests.rs` |
| `service.rs` | `Service` owns the claimed home, credentials, houses, models, chats and uploads; `db()` is the home's open database, and a write that needs a transaction takes `home.store()`; `edit_document` and `read` answer the chief's documents |
| `serve.rs` | `serve` from start to stop: claims the home, probes the sandbox, starts the harness and model providers, publishes `server.json`, and closes in order |
| `connection.rs` | The WebSocket connections: accepting, each one's read and write task, its queue of waiting messages, and the close codes |
| `requests.rs` | The hello, then one match over `ClientMessage` naming the owner that answers each request; a handler that errs or panics ends only its request, with a `fault` |
| `runtime.rs` | `Serving`, the serve loop's state: its events, settle, refresh, and recovering from a failed event. A 50 ms tick only checks memory and the skills on disk unless the state may have changed, as `Since` defines: an event, a tick that found work, a clock turn (a road overgrowing, a budget's share gone moving), a house move's progress or an overdue provider answer. Every two seconds the tools folder is read again |
| `diagnostic.rs` | `tell`, the server's one diagnostic: a line on stderr under `hamlet_server:`, written as a JSON string with the credentials' secrets removed (`Secrets::Fixed` for a fixed text) |
| `protocol.rs` | `Protocol::parse` validates client input against the client protocol's schema and correlates a fault to the message's `id` when it is a `RequestId`; `Refusal`, an expected outcome with its words; reply and fault rendering; its tests are in `protocol/tests.rs` |
| `record.rs`, `record/` | Agent identity, membership, names, roads, office, models, limits and typed refusals. `RecordOperation` is a transactional change, and every change to a hamlet's record is one of its `Change`s; its tests are in `record/tests.rs` |
| `documents.rs`, `settings.rs`, `limits.rs` | Chief edits and reads. `Service` reads each document once into a typed `Work`, `Settings` (stored settings plus process overrides) or `Limits`. A hamlet's limits live only in its record, with the schema's defaults filled in; `Limits::model_of` picks each agent's model |
| `chats.rs`, `chats/` | Chief and peer watches, durable inputs, follow-up work, reflection, stop, retry, resume and rewind. `Chats::next` yields each tool call or preparation as it ends, a panicked one as a failed outcome |
| `chats/calls.rs`, `chats/calls/` | The one tool gate: `decide.rs` admits a call by its request digest and keeps the gate's decision, `ask.rs` raises a question, `answer.rs` validates a chief's answer and makes all its writes in one transaction, `tasks.rs` runs the calls. Marked outcomes survive replay; changed requests are refused |
| `gate.rs`, `oversight.rs`, `oversight/held.rs` | Approval rules on typed limits, typed dangers, wait kinds and rulings. `held.rs` alone stores a held call and its wait, with typed transitions; `oversight::stop` and `rewind` are the policies that end waits |
| `harness.rs` | Start/watch runtime processes, bind conversations and send each runtime protocol message to its own method. Tool calls go to the chats' gate |
| `harness/session.rs`, `harness/sync.rs` | `RuntimeSession`, the runtime protocol's ids and requests in flight; what each conversation is owed (creates, cancels, tools, inputs) and what its runtime reports back |
| `harness/answers.rs`, `harness/credentials.rs` | Admit and charge model attempts with the typed model selection; credential leases for provider work |
| `harness/calls.rs` | The calls a runtime asks of the host (allow, operation, prompt, yield), each turned into a `Notice::Call` that `Harnesses::answer` answers |
| `harness/tools.rs` | The tools a runtime offers its conversations, run in this server as operations |
| `harness/process.rs`, `harness/bindings.rs` | The pi launch profile and processes; runtimes, bindings and inputs with their `InputStatus` in `realm.sqlite` |
| `harness/mark.rs` | `InputMark`, the typed kind and data of each input (chief line, retry, reflection, notice, letter, errand), converted to a protocol `Mark` at the edge |
| `chats/driver.rs`, `harness_driver.rs`, `bin/hamlet_harness_driver.rs` | With the `harness` feature: the test driver's reach into the chats' gate, waits and stop; the driver's body; and the binary, only `main`, for the real-process suite in `tests/harness` |
| `tools.rs`, `tools/`, `hand.rs` | `ToolSession` runs every tool call on the workspace `Houses::tools` chose: built-in tools over `environment::Reach` (this machine under the sandbox, or a remote workspace), and hand-added manifests, which only run here |
| `tools/shell.rs` | The shell tools: `Dialect` names each shell language once, `Shells` are the ones a workspace offers, and `shell::run` is the one run of a shell call over `Reach`, here or remote |
| `sandbox.rs`, `sandbox/` | Bubblewrap/Seatbelt policies, subprocess limits and termination |
| `papers.rs` | `Houses` picks the local or remote `Workspace` once at startup. Callers get its houses (`house`, `reader`, `tools`), its `Capability` refusals and whether it `can_confirm` each change; no caller branches on the workspace |
| `papers/files.rs`, `papers/tree.rs`, `papers/remote.rs` | `Files`, one async house tree for both workspaces: local work runs off the runtime through descriptor-relative no-follow calls, remote work as typed requests |
| `papers/copy.rs`, `papers/moving.rs` | `Snapshot`: the one verified tree write with claimed-path rollback, used by gifts and house moves; copies to the next-start workspace |
| `workspace.rs` | `Transport` to the worker with typed `Failure` codes; `WorkspaceSession` of one agent's requests under a `Policy`, sending the library by digest, cancelling on drop and naming uncertain outcomes |
| `gifts.rs`, `gifts/` | Connection-owned upload staging and manifest limits; skill and file gifts resolved, snapshotted, delivered and recorded apart from the service loop |
| `prompt.rs`, `prompt/` | `sections` composes charter, machine, project, character, notes, shelf, hamlet and realm |
| `starters.rs` | Shipped starter content from `crates/hamlet_server/assets/starters/` |
| `models.rs`, `credentials.rs`, `scrub.rs` | Catalogue and provider requests, the model each attempt runs (`Choices`), atomic credential files, sign-ins and secret removal before publication or harness persistence, and before a diagnostic is logged |
| `spend.rs` | Budget reservations and usage charges deduplicated by model attempt, plus agent/task/hamlet accounting |
| `society.rs`, `society/` | `SocietyTool` names each society tool, its role and schema, and `SocietyOutcome` is what one returns, which `once` keeps at its marker; letters/questions, instructions/objections, votes, library and recoverable merge/departure work. The sweeps run on the ticks that rebuild the state, and read only due votes and reviews; finished ones are kept under `votes-done` and `review-done` |
| `realm.rs`, `realm/` | Living/closed places, cross-hamlet addresses, founding, agreements and pending house moves |
| `testing.rs` | For unit tests: `config`, a harness configuration that starts nothing, and `set_limits`, which writes a hamlet's limits |
| `state.rs` | Composes protocol state from the owning records and live activity, with the instant one of its views next changes with the clock alone; `skills_moved` tells whether skills on disk differ from the state's |

`realm.sqlite` holds server-owned records, chief and society documents,
bindings, inputs, waits, charges and operation markers. It does not hold
pi's authoritative transcript. Each runtime opens only its own store under
`harnesses/<instance>/`; the server consumes committed updates through the
runtime protocol. House files stay files. See [`home.md`](home.md).

## The pi harness

`harnesses/pi/src/main.ts` is the Node entry. The server starts it with
`--runtime`, `--store` and `--transport stdio`. The runtime protocol is
versioned JSON lines on stdin/stdout; diagnostics go to stderr. How
runtimes, bindings and the runtime protocol work is in
[`harness.md`](harness.md).

| File under `harnesses/pi/src/` | Owns |
|---|---|
| `protocol.ts` | The runtime protocol's types and checks, from the generated `protocol-schema.ts` |
| `channel.ts` | Correlated protocol requests and replies |
| `runtime.ts` | Handshake, conversations, input admission, configure, cancellation, history and updates |
| `extension.ts` | The hamlet extension: tools, prompt section and allow/yield hooks, each asked until the server decides; waits end on the key the server gives |
| `items.ts` | Kept entries and answers as the protocol shows them |
| `models.ts` | Provider calls admitted and charged by the server, made with the pi model built from its selection; asks it to scrub snapshots and final responses |
| `faux.ts` | Offline scripts, with separate agent and errand queues |
| `faults.ts` | Exact process-crash pauses used by tests |

Credentials belong to `Service`; the harness receives only what an
authorized provider attempt needs, in memory. Its scrub requests go back
to the server. Role changes use configure/configured before the next
request. Ballots have only `cast_ballot`; other one-off errands have no
ordinary tools.

## Where to change behavior

| Change | Owner |
|---|---|
| Typed commands, command replies | `hamlet_server/src/chats/say.rs`, `society.rs`, `realm/deeds.rs`; advertised deeds in `state.rs` |
| Conversation stop/retry/resume/rewind | `hamlet_server/src/chats/deeds.rs` and `chats/follow.rs` |
| Transcript rows, paging and peer watches | `hamlet_server/src/chats/fold.rs`, `chats/watch.rs`; `hamlet_client/src/chats.rs` |
| Questions and their answer cards | `hamlet_server/src/chats/asking.rs`, `chats/calls/ask.rs`, `chats/calls/answer.rs`, `hamlet_ui/src/asking.rs` |
| Approvals, grants and waits | `hamlet_server/src/gate.rs`, `chats/calls/decide.rs`, `oversight.rs`, `oversight/held.rs` |
| Selecting, quoting and replying to chat text | `hamlet_ui/src/select.rs`, `screens/chat/quote.rs`, `screens/chat/reply.rs` |
| Agent creation, model choice, departure and merge | `hamlet_server/src/society/agents.rs`, `leaving.rs`, `merging.rs`, whose `merging/lane.rs` runs every merge and split, one at a time; a chief's merge that fails lets the lane and the chief's connection go; rules in `record/` |
| Letters, questions, votes and library | `hamlet_server/src/society/letters.rs`, `votes.rs`, `library.rs` |
| Instructions and objections | `hamlet_server/src/society/objections.rs`, `reviews.rs` |
| Founding and cross-hamlet agreements | `hamlet_server/src/realm/deeds.rs`, `dealings.rs` |
| A prompt or chief document | `hamlet_server/src/prompt/`, `documents.rs`; default charter in `hamlet_server/assets/charter.md` |
| A model connection, login or price | `hamlet_server/src/models.rs`, `credentials.rs`; provider implementation in pi |
| Folder, mode or tool reach | `hamlet_server/src/documents.rs`, `tools.rs`, `sandbox/`; [`sandbox.md`](sandbox.md) |
| CLI process discovery | `hamlet_cli/src/process.rs`; [`cli.md`](cli.md) |
| Client protocol | `hamlet_protocol/src/client/`, then `V` and `cargo test -p hamlet_protocol` for `client_schema.fingerprint`; [`protocol.md`](protocol.md) |
| Runtime protocol | `hamlet_protocol/src/runtime/`, then `cargo test -p hamlet_protocol`; [`harness.md`](harness.md) |
| Received entities | `hamlet_client/src/connection.rs`, `realm.rs`, `waits.rs`, `chats.rs`, `index.rs` |
| Gifts of skills and files, their upload in parts | `hamlet_client/src/gifts.rs`; dropped files reach it through `hamlet_realm/src/orders.rs` |
| A panel or right-click menu | `hamlet_ui/src/screens/`, `world_menu/items.rs` |
| Panels' colors, controls and icons | `assets/theme.toml`; `hamlet_ui/src/theme.rs`, `looks.rs`, `widgets/feel/`, `widgets/icons.rs` |
| World colors, size or timing | `assets/palette.toml`, `world.toml`, `vfx.toml`, `ui.toml` |
| Ground, houses and roads | `hamlet_realm/src/generator/`; `LandData::surface` is the drawn surface |
| Robot motion | `hamlet_realm/src/keeper/acts/`; [`robots.md`](robots.md) |
| Wizard, Hearth, camera or sound | `hamlet_realm/src/chief/`, `pick/camera.rs`, `sound/` |
| Helpers several modules' tests share | `testing.rs` in `hamlet_server`, `hamlet_ui` and `hamlet_realm`; fixtures crates share in `hamlet_testing` |
| Window state or editor saves | `hamlet_ui/src/remember.rs`, `window.rs`, `saving.rs`; `hamlet_desktop/src/editor.rs` |

`hamlet_realm/src/hamlet.rs` bridges received entities to the generator's
plots and roads. `AgentNumber` is an agent's number in its hamlet, not a
name; `HamletId` is a hamlet's protocol id.
`chief.rs` names the wizard in the world and the chief's panel in the UI;
`Chief` in `hamlet_client` is the server's chief identity. The window keeps
only its own view files under `HAMLET_VIEW`.

## Tuning and previewing the window

Assets are embedded. The `hot-reload` feature also watches their source:

```sh
cargo build --bin hamlet_app --features hot-reload
HAMLET_HOME=/tmp/hamlet-try HAMLET_APP="$PWD/target/debug/hamlet_app" cargo run -p hamlet_cli -- dev
```

The generator section of `world.toml` and existing mesh colors do not
change an already-built world. The charter is a server document, edited
through the protocol rather than asset hot reload. An unreadable asset
reports an error and keeps the last good values.

| Command | Output |
|---|---|
| `cargo run --example ui_gallery -- --scene <name> --out <dir>` | Panel scenes listed in `crates/hamlet_app/examples/ui_gallery/main.rs` |
| `cargo run --release --example far_lands -- --out <dir>` | Land and town images; `--bench` measures it |
| `cargo run --example chief_sheet -- --out <dir>` | Wizard, Wickstep and library |
| `cargo run --example cursor_sheet` | Cursor sheet |
| `cargo run --release --example sound_sheet` | Generated sound WAVs |

`style_test` and `town_gen` open windows. For live work use
`HAMLET_HOME=/tmp/hamlet-try cargo xtask dev`; it builds both Rust processes and
sets `HAMLET_VIEW=<home>/window`. Closing the window leaves the server;
Ctrl-C in the dev terminal stops it. Use Node from `harnesses/pi/.nvmrc`.
