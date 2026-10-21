# A tour of the code

Where things are and where to change them. Start with
[`README.md`](../README.md) for running Hamlet and [`AGENTS.md`](../AGENTS.md)
for the checks and conventions.

## How a change travels

The Rust server owns the realm and permissions. Its Node pi harness owns
conversations and model calls. The window and CLI are its clients, over the client protocol;
neither opens the server's database nor runs an agent.

```text
Window / terminal -- client protocol --> hamlet-server -- runtime protocol --> pi
                                               |                               |
                                         realm.sqlite                    harness store
                                        houses, sandbox                  conversations
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
| `hamlet_protocol` | The client protocol's types and recorded frames |
| `hamlet_acceptance` | Real-process protocol and CLI scenarios on scratch homes |
| `hamlet_client` | Connection, outbox, realm, waits and watched chats as Bevy entities |
| `hamlet_desktop` | File/link openers, editor files, dialogs, wake locks and window icon |
| `hamlet_assets` | Embedded assets and Bevy asset loading |
| `hamlet_realm` | Land, hamlets, houses, roads, robots, chief, sound and camera |
| `hamlet_ui` | Panels, menus, chat and pointer |
| `hamlet_vfx` | Visual effects |
| `hamlet_testing` | Offline fixture values and scratch directories |
| `hamlet_workspace` | Linux remote-workspace manager/worker reached through the server's workspace-v1 client |
| `hamlet_workspace_wire` | Workspace protocol v1 for both sides: one type per operation with its reply, error codes, limits, the walk order and the shared fixture |

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
the public test boundary is the client protocol or the process command line.

| Module | Responsibility and interface |
|---|---|
| `home.rs`, `claim.rs` | `Home::open` claims a path, opens its `Store` and checks the `realm.id` pair. `Layout` names storage, credentials and harness directories |
| `store.rs`, `schema.sql` | The schema and the `realm.sqlite` connection, with WAL, FULL sync and foreign keys; a store of another version is refused |
| `operation.rs` | `OperationId` identifies runtime plus call key. Owns every marker: `Store::run` commits an operation's result marker with its record effects; `Store::once` and `once` mark any outcome with its request digest, alone or in the caller's transaction, and refuse a changed request under a used key |
| `service.rs` | `Service` owns the claimed home, credentials, houses, models, chats and uploads; `handle` routes chief requests |
| `runtime.rs` | `serve` probes the sandbox, starts the harness, publishes `server.json`, authenticates connections and closes in order |
| `protocol.rs` | `Protocol::parse` validates client input against the client protocol's schema; reply and fault rendering |
| `record.rs`, `record/` | Agent identity, membership, names, roads, office, models, constraints and typed refusals. `RecordOperation` is a transactional change, and every change to a hamlet's record is one of its `Change`s |
| `documents.rs`, `settings.rs`, `limits.rs` | Chief edits and reads. `Service` reads each document once into a typed `Work`, `Settings` (stored settings plus process overrides) or `Limits`. A hamlet's limits live only in its record, with the schema's defaults filled in; `Limits::model_of` picks each agent's model |
| `chats.rs`, `chats/` | Chief and peer watches, durable inputs, follow-up work, reflection, stop, retry, resume and rewind. `Chats::next` yields each tool call or preparation as it ends, a panicked one as a failed outcome |
| `chats/calls.rs`, `chats/calls/` | The one tool gate: `decide.rs` admits a call by its request digest and keeps the gate's decision, `ask.rs` raises a question, `answer.rs` validates a chief's answer and makes all its writes in one transaction, `tasks.rs` runs the calls. Marked outcomes survive replay; changed requests are refused |
| `gate.rs`, `oversight.rs`, `oversight/held.rs` | Approval rules on typed limits, typed dangers, wait kinds and rulings. `held.rs` alone stores a held call and its wait, with typed transitions; `oversight::stop` and `rewind` are the policies that end waits |
| `harness.rs` | Start/watch runtime processes, bind conversations and dispatch each link message. Tool calls go to the chats' gate |
| `harness/session.rs`, `harness/sync.rs` | The link's ids and requests in flight; what each conversation is owed (creates, cancels, tools, inputs) and what its runtime reports back |
| `harness/answers.rs`, `harness/credentials.rs` | Admit and charge model attempts with the typed model selection; credential leases for provider work |
| `harness/process.rs`, `harness/bindings.rs` | The pi launch profile and processes; runtimes, bindings and inputs with their `InputStatus` in `realm.sqlite` |
| `harness/mark.rs` | `InputMark`, the typed kind and data of each input (chief line, retry, reflection, notice, mail, errand), converted to a link `Mark` at the edge |
| `chats/driver.rs`, `bin/hamlet-harness-driver.rs` | With the `harness` feature: the test driver's reach into the chats' gate, waits and stop, for the real-process suite in `tests/harness` |
| `tools.rs`, `tools/`, `hand.rs` | `tools::Session` runs every tool call on the backend `Houses::tools` chose: built-in tools over `environment::Reach` (this machine under the sandbox, or a remote workspace), and hand-added manifests, which only run here |
| `sandbox.rs`, `sandbox/` | Bubblewrap/Seatbelt policies, subprocess limits and termination |
| `papers.rs` | `Houses` picks the local or remote backend once at startup. Callers get its houses (`house`, `reader`, `tools`), its `Capability` refusals and whether it `confirms` each change; no caller branches on the backend |
| `papers/files.rs`, `papers/tree.rs`, `papers/remote.rs` | `Files`, one async house tree for both backends: local work runs off the runtime through descriptor-relative no-follow calls, remote work as typed requests |
| `papers/copy.rs`, `papers/moving.rs` | `Snapshot`: the one verified tree write with claimed-path rollback, used by gifts and house moves; copies to the next-start backend |
| `workspace.rs` | `Transport` to the manager with typed `Failure` codes; `Session` of one agent's requests under a `Policy`, sending the library by digest, cancelling on drop and naming uncertain outcomes |
| `gifts.rs`, `gifts/` | Connection-owned upload staging and manifest limits; skill and file gifts resolved, snapshotted, delivered and recorded apart from the service loop |
| `prompt.rs`, `prompt/` | `sections` composes charter, machine, project, character, notes, shelf, hamlet and realm |
| `starters.rs` | Shipped starter content from `crates/hamlet_server/assets/starters/` |
| `models.rs`, `credentials.rs`, `scrub.rs` | Catalogue and provider requests, the model each attempt runs (`Choices`), atomic credential files, sign-ins and secret removal before publication or harness persistence |
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
JSON lines on stdin/stdout; diagnostics go to stderr. How runtimes,
bindings and the link work is in [`harness.md`](harness.md).

| File under `harnesses/pi/src/` | Owns |
|---|---|
| `link.ts` | TypeBox messages; `npm run schema` exports `link.schema.json` |
| `channel.ts` | Correlated link requests and replies |
| `runtime.ts` | Handshake, conversations, input admission, configure, cancellation, history and updates |
| `extension.ts` | The hamlet extension: tools, prompt section and allow/yield hooks, each asked until the server decides; waits end on the key the server gives |
| `items.ts` | Kept entries and answers as the link shows them |
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
| Agent creation, model choice, departure and merge | `hamlet_server/src/society/agents.rs`, `leaving.rs`, `merging.rs`, whose `merging/lane.rs` runs every merge and split; rules in `record/` |
| Letters, questions, votes and library | `hamlet_server/src/society/letters.rs`, `votes.rs`, `library.rs` |
| Instructions and objections | `hamlet_server/src/society/objections.rs`, `reviews.rs` |
| Founding and cross-hamlet agreements | `hamlet_server/src/realm/deeds.rs`, `dealings.rs` |
| A prompt or chief document | `hamlet_server/src/prompt/`, `documents.rs`; default charter in `hamlet_server/assets/charter.md` |
| A model connection, login or price | `hamlet_server/src/models.rs`, `credentials.rs`; provider implementation in pi |
| Folder, mode or tool reach | `hamlet_server/src/documents.rs`, `tools.rs`, `sandbox/`; [`sandbox.md`](sandbox.md) |
| CLI process discovery | `hamlet_cli/src/process.rs`; [`cli.md`](cli.md) |
| Client protocol | `hamlet_protocol/src/messages.rs`; [`protocol.md`](protocol.md) |
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
