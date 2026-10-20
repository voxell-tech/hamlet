# A tour of the code

Where things are and where to change them. Start with
[`README.md`](../README.md) for running Hamlet and [`AGENTS.md`](../AGENTS.md)
for the checks and conventions.

## How a change travels

The Rust server owns the realm and permissions. Its Node pi harness owns
conversations and model calls. The window and CLI are protocol v0 clients;
neither opens the server's database nor runs an agent.

```text
Window / terminal -- protocol v0 --> hamlet-server -- private link --> pi
                                          |                          |
                                   realm.sqlite                 harness store
                                   houses, sandbox              conversations
```

A typed line reaches `chats::converse` through `runtime::serve`. Commands
change server-owned records or documents; ordinary words become a durable
harness input. Pi asks the server for its prompt, tool permissions and
model budget. The server checks calls again when executing them, including
replayed calls. Kept harness updates feed watched transcripts and the
server's state. `hamlet_client` turns `state`, `change`, `transcript` and
`turn` into Bevy entities; world and panels read them.

## The crates and packages

Rust crates live under `crates/`. The root package `hamlet` supplies
`hamlet-app` and the visual examples.

| Crate or package | Owns |
|---|---|
| `hamlet_cli` | `hamlet`: process lifecycle, terminal chat, editors and login, without Bevy |
| `hamlet_server` | `hamlet-server`: realm, permissions, tools, records, credentials and harness supervision |
| `harnesses/pi/` | Node pi-durable/pi-ai runtime; conversations, provider calls, compaction and forks |
| `hamlet_protocol/schema/` | TypeBox source and offline schema checks, a separate Node package |
| `hamlet_protocol_gen` | Generates Rust types from `crates/hamlet_protocol/schema.json` and `harnesses/pi/link.schema.json` |
| `hamlet_protocol` | Generated v0 types, schema drift checks and recorded frames |
| `hamlet_acceptance` | Real-process protocol and CLI scenarios on scratch homes |
| `hamlet_client` | Connection, outbox, realm, waits and watched chats as Bevy entities |
| `hamlet_desktop` | File/link openers, editor files, dialogs, wake locks and window icon |
| `hamlet_assets` | Embedded assets and Bevy asset loading |
| `hamlet_realm` | Land, hamlets, houses, roads, robots, chief, sound and camera |
| `hamlet_ui` | Panels, menus, chat and pointer |
| `hamlet_vfx` | Visual effects |
| `hamlet_testing` | Offline fixture values and scratch directories |
| `hamlet_workspace` | Linux remote-workspace manager/worker reached through the server's workspace-v1 client |

`RealmPlugin` in `hamlet_realm/src/lib.rs` adds the scene modules, then the
generator, the chief's interactions and the bridge from client entities.
Examples that only draw a scene add its module plugins themselves; the
town generator also adds `generator::plugin` and an `Origin` for its seed.

`hamlet_ui/src/screens/` groups the panels drawn from the client's state.
`widgets/` groups their shared controls: `feel/`, `icons.rs`, `tabs.rs`,
`platform.rs` and `tuning.rs`. The other panels live beside those folders;
`theme.rs` and `looks.rs` define their appearance.

## The server

Paths below are under `crates/hamlet_server/src/`. Modules use a file and
same-named directory rather than `mod.rs`. Most domain APIs are internal;
the public test boundary is protocol v0 or the process command line.

| Module | Responsibility and interface |
|---|---|
| `home.rs`, `claim.rs` | `Home::open` claims a path, opens its `Store` and checks the `realm.id` pair. `Layout` names storage, credentials and harness directories |
| `store.rs`, `schema.sql` | Ordered schema migrations and the `realm.sqlite` connection, with WAL, FULL sync and foreign keys |
| `operation.rs` | `OperationId` identifies runtime plus call key. Owns every marker: `Store::run` commits an operation's result marker with its record effects; `Store::once` and `once` mark any outcome with its request digest, alone or in the caller's transaction, and refuse a changed request under a used key |
| `service.rs` | `Service` owns the claimed home, credentials, houses, models, chats and uploads; `handle` routes chief requests |
| `runtime.rs` | `serve` probes the sandbox, starts the harness, publishes `server.json`, authenticates connections and closes in order |
| `protocol.rs` | `Protocol::parse` validates client input against v0; reply and fault rendering |
| `record.rs`, `record/` | Agent identity, membership, names, roads, office, models, constraints and typed refusals. `RecordOperation` is a transactional change, and every change to a hamlet's record is one of its `Change`s |
| `documents.rs`, `settings.rs`, `limits.rs` | Chief edits and reads, stored settings plus process overrides, JSON limit validation |
| `chats.rs`, `chats/` | Chief and peer watches, durable inputs, follow-up work, reflection, stop, retry, resume and rewind |
| `chats/calls.rs`, `gate.rs`, `oversight.rs` | Request digest/binding checks, approval rules, waits, standing grants and execution. Marked outcomes survive replay; changed requests are refused |
| `harness.rs`, `harness/` | Start/watch runtime processes, bind conversations, admit inputs, carry link messages, admit and charge model attempts, acknowledge updates and retain owed cancellation. Tool calls go to the chats' gate |
| `chats/driver.rs`, `bin/hamlet-harness-driver.rs` | With the `harness` feature: the test driver's reach into the chats' gate, waits and stop, for the real-process suite in `tests/harness` |
| `tools.rs`, `tools/`, `hand.rs` | Built-in tools, their confined file/shell execution and hand-added manifests |
| `sandbox.rs`, `sandbox/` | Bubblewrap/Seatbelt policies, subprocess limits and termination |
| `papers.rs`, `papers/tree.rs` | Descriptor-relative house access, notes, bounded no-follow tree operations and recoverable snapshots |
| `workspace.rs`, `papers/files.rs`, `papers/moving.rs` | Authenticated remote execution, local/remote house files and background copies to the next-start backend |
| `gifts.rs`, `gifts/` | Connection-owned upload staging, manifest limits, confined delivery and skill gifts |
| `prompt.rs`, `prompt/` | `sections` composes charter, machine, project, character, notes, shelf, hamlet and realm |
| `starters.rs` | Shipped starter content from `crates/hamlet_server/assets/starters/` |
| `models.rs`, `credentials.rs`, `scrub.rs` | Catalogue and provider requests, atomic credential files, sign-ins and secret removal before publication or harness persistence |
| `spend.rs` | Budget reservations and usage charges deduplicated by model attempt, plus agent/task/hamlet accounting |
| `society.rs`, `society/` | `SocietyTool` names each society tool, its role and schema; letters/questions, instructions/objections, votes, library and recoverable merge/departure work. The 50 ms sweeps read only due votes and reviews; finished ones are kept under `votes-done` and `review-done` |
| `realm.rs`, `realm/` | Living/closed places, cross-hamlet addresses, founding, agreements and pending house moves |
| `state.rs` | Composes protocol state from the owning records and live activity |

`realm.sqlite` holds server-owned records, chief and society documents,
bindings, inputs, waits, charges and operation markers. It does not hold
pi's authoritative transcript. Each runtime opens only its own store under
`harnesses/<instance>/`; the server consumes committed updates through the
link. House files stay files. See [`home.md`](home.md).

## The pi harness

`harnesses/pi/src/main.ts` is the Node entry. The server starts it with
`--runtime`, `--store` and `--link stdio`. The private link is versioned
JSON lines on stdin/stdout; diagnostics go to stderr.

| File under `harnesses/pi/src/` | Owns |
|---|---|
| `link.ts` | TypeBox messages; `npm run schema` exports `link.schema.json` |
| `channel.ts` | Correlated link requests and replies |
| `runtime.ts` | Handshake, conversations, input admission, tools, prompt/configure hooks, cancellation, history and updates |
| `models.ts` | Provider calls admitted and charged by the server; asks it to scrub snapshots and final responses |
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
| Questions and their answer cards | `hamlet_protocol/src/asking.rs`, `hamlet_server/src/chats/calls.rs`, `hamlet_ui/src/asking.rs` |
| Selecting, quoting and replying to chat text | `hamlet_ui/src/select.rs`, `screens/chat/quote.rs`, `screens/chat/reply.rs` |
| Agent creation, model choice, departure and merge | `hamlet_server/src/society/agents.rs`, `leaving.rs`, `merging.rs`; rules in `record/` |
| Letters, questions, votes and library | `hamlet_server/src/society/letters.rs`, `votes.rs`, `library.rs` |
| Instructions and objections | `hamlet_server/src/society/objections.rs`, `reviews.rs` |
| Founding and cross-hamlet agreements | `hamlet_server/src/realm/deeds.rs`, `dealings.rs` |
| A prompt or chief document | `hamlet_server/src/prompt/`, `documents.rs`; default charter in `hamlet_server/assets/charter.md` |
| A model connection, login or price | `hamlet_server/src/models.rs`, `credentials.rs`; provider implementation in pi |
| Folder, mode or tool reach | `hamlet_server/src/documents.rs`, `tools.rs`, `sandbox/`; [`sandbox.md`](sandbox.md) |
| CLI process discovery | `hamlet_cli/src/process.rs`; [`cli.md`](cli.md) |
| Protocol or generated types | `hamlet_protocol/schema/`, then `npm run schema` and `cargo run -p hamlet_protocol_gen`; [`protocol.md`](protocol.md) |
| Received entities | `hamlet_client/src/connection.rs`, `realm.rs`, `waits.rs`, `chats.rs` |
| A panel or right-click menu | `hamlet_ui/src/screens/`, `world_menu/items.rs` |
| Panels' colors, controls and icons | `assets/theme.toml`; `hamlet_ui/src/theme.rs`, `looks.rs`, `widgets/feel/`, `widgets/icons.rs` |
| World colors, size or timing | `assets/palette.toml`, `world.toml`, `vfx.toml`, `ui.toml` |
| Ground, houses and roads | `hamlet_realm/src/generator/`; `LandData::surface` is the drawn surface |
| Robot motion | `hamlet_realm/src/keeper/acts/`; [`robots.md`](robots.md) |
| Wizard, Hearth, camera or sound | `hamlet_realm/src/chief/`, `pick/camera.rs`, `sound/` |
| Window state or editor saves | `hamlet_ui/src/remember.rs`, `window.rs`, `saving.rs`; `hamlet_desktop/src/editor.rs` |

`hamlet_realm/src/hamlet.rs` bridges received entities to the generator's
plots and roads. `Citizen` is an agent number in its hamlet, not a name.
`chief.rs` names the wizard in the world and the chief's panel in the UI;
`Chief` in `hamlet_client` is the server's chief identity. The window keeps
only its own view files under `HAMLET_VIEW`.

## Tuning and previewing the window

Assets are embedded. The `hot-reload` feature also watches their source:

```sh
cargo build --bin hamlet-app --features hot-reload
HAMLET_HOME=/tmp/hamlet-try HAMLET_APP="$PWD/target/debug/hamlet-app" cargo run -p hamlet_cli -- dev
```

The generator section of `world.toml` and existing mesh colors do not
change an already-built world. The charter is a server document, edited
through the protocol rather than asset hot reload. An unreadable asset
reports an error and keeps the last good values.

| Command | Output |
|---|---|
| `cargo run --example ui_gallery -- --scene <name> --out <dir>` | Panel scenes listed in `examples/ui_gallery/main.rs` |
| `cargo run --release --example far_lands -- --out <dir>` | Land and town images; `--bench` measures it |
| `cargo run --example chief_sheet -- --out <dir>` | Wizard, Wickstep and library |
| `cargo run --example cursor_sheet` | Cursor sheet |
| `cargo run --release --example sound_sheet` | Generated sound WAVs |

`style_test` and `town_gen` open windows. For live work use
`HAMLET_HOME=/tmp/hamlet-try make dev`; it builds both Rust processes and
sets `HAMLET_VIEW=<home>/window`. Closing the window leaves the server;
Ctrl-C in the dev terminal stops it. Use Node from `harnesses/pi/.nvmrc`.
