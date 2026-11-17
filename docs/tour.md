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

The server is a Bevy app with no frame
([ADR 0015](adr/0015-the-server-runs-as-a-bevy-app-with-no-frame.md)): its runner sleeps until
an input comes and updates the app once for it (`app/runner.rs`). A typed line
is read into a request of its kind and reaches the system for that kind
(`clients/handlers.rs`). A line that begins with `/` is a command, run by the
system it was added with (`clients/commands.rs`); other words become a kept input of
the agent's chat (`conversations/chats.rs`). Every change an input makes is in its one
transaction, which also keeps the jobs its changes need; `PostUpdate` keeps it
(`app/follow_up.rs`), follows the inputs that ended, runs the jobs and starts the
tool runs, and `Last` sends the runtimes what they are owed and the clients
the state, then their replies. A handler never
awaits: what awaits runs on a task whose result comes back as an input
(`app/tasks.rs`). Pi asks the server for its prompt, tool permissions and
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
which `deny` runs.

`vendor/fynix` is the submodule that supplies `bevy_fynix`, the panels'
view layer, with the rich field the chat is typed in; the workspace excludes
it, and it is not edited here.

| Crate or package | Owns |
|---|---|
| `hamlet_cli` | `hamlet`: process lifecycle, terminal chat, editors and login, without Bevy; and `hamlet_launch`, what an installed app opens (`src/launch.rs`) |
| `hamlet_server` | `hamlet_server`: the realm's society, its conversations, the gate, budgets, tools, clients, credentials and harness supervision |
| `hamlet_store` | The realm's database: `schema.sql` with Diesel's tables generated from it, opening and checking it, transactions, markers and jobs, and the typed ids |
| `hamlet_git` | Scholars' and the library's repositories through gix, with no git program: refs, commits from files, reads, diffs, merges, reverts, versions, history, purging, and the writes that check a lodge out |
| `hamlet_sandbox` | What confines a command: the bubblewrap and Seatbelt policies, launching, a command's leftovers, and the secrets taken out of what it gives back |
| `hamlet_os` | The system calls whose form differs by OS, with no C code below them: `fs`, directories held open and what is reached from them one name at a time, never through a link at that name, and the tools' walk, path text and error words; `process`, a command's process tree, started inside its limits and killed whole, and `process::pty`, a command on a pseudo-terminal |
| `hamlet_app` | `hamlet_app`, the window: its panels, menus, chat and pointer, and the eight examples that draw the world and the panels |
| `harnesses/pi/` | Node pi-durable/pi-ai runtime; conversations, provider calls, compaction and forks |
| `hamlet_protocol` | Both protocols' types: `client` and `runtime`; `Version`, the semver version each protocol carries; `RequestId`, which holds only a hyphenated UUID; `State::apply` for a client's changes; recorded frames, the schema pi reads and the client schema's fingerprint |
| `hamlet_acceptance` | Real-process protocol, CLI and OAuth scenarios on scratch realm folders, through the typed `Client`, the `hamlet` command and `hamlet_testing`'s server |
| `hamlet_client` | Connection, outbox, realm, waits and watched chats as Bevy entities, the `Index` that finds them by their protocol ids, and images and files read in parts off the frame |
| `hamlet_desktop` | File/link openers, editor files, dialogs (`choose.rs`), the clipboard's images and files, wake locks and window icon |
| `hamlet_import` | Conversations of other programs found on the chief's machine (Claude Code's, Codex's and pi's sessions) or in a ChatGPT or Claude export, grouped by project, each read as the `Imported` a client sends: the branch last shown, its first message and newest ones, tool calls as steps (C35) |
| `hamlet_update` | A release updating itself: how this copy was installed, the signed manifest, the download and putting it in place; its `check` feature, the window's alone, is what touches the network |
| `hamlet_assets` | Embedded assets and Bevy asset loading |
| `hamlet_realm` | The two islands, the city, the harbor, hamlets, houses, roads, robots, the Hand, the library, chief, sound and camera |
| `hamlet_ui` | The widget kit: theme, styles, controls and their effects, with no client state |
| `hamlet_vfx` | Visual effects |
| `hamlet_testing` | Fixtures shared by tests: scratch folders, protocol values, recorded sessions' server messages, faux scripts, the loopback OpenAI endpoint, and a server process on a scratch realm folder with a raw connection to it (`send`, `until`, `answers`) |
| `hamlet_workspace` | The Linux remote-workspace worker |
| `hamlet_workspace_wire` | The workspace protocol for both sides: one type per operation with its reply, error codes, limits, the walk order and the shared fixture |

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
chief's place, the library), and `Trips` the messages on the roads right
now, each carried as a letter.

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
the other. A document finds a written path's file through `PathLinks`,
takes the words that are an agent's address and draws each as a chip
through `AgentLinks`, and
draws a picture through `PictureView`, which the app's `links` screen,
`reference.rs` and `pictures` screen set. The panels' steps before fynix mounts are `PanelSystems` in
`hamlet_app/src/lib.rs` (`Follow`, `Bind`), in `PreUpdate`;
`panel_sets` orders them, and the apps in `testing.rs` add it, so a UI
test settles in one update. A panel registers its dock window in
`dock.rs` with `add_panel`; one that opens more than once, as the
Terminal panel does, with `add_numbered_panel`. Its others are
`<id>-2`, `<id>-3` and so on, known only while the dock or the shown
desk holds them, so the dock's add menu offers only the first. A
terminal dragged from a Terminal panel's list onto the dock opens in
one of them (`screens/terminal/carry.rs`).

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

## The server

Paths below are under `crates/hamlet_server/src/`. Modules use a file
and same-named directory rather than `mod.rs`. Most domain APIs are
internal; the crate exports only `serve`. Its unit tests sit beside their
modules, on an in-memory or scratch database and scratch repositories
(`testing.rs` holds what several share). The `faults` feature compiles the
points where a test can kill the server (`app/fault.rs`), and `harness` the tests
that start the pi harness; both are for tests only. The `crashes` target,
`tests/crashes/`, starts the server built with them, kills it at each point
and serves the realm again; `remote.rs` there serves a realm in a loopback
double of the remote workspace worker.

| Module | Responsibility and interface |
|---|---|
| `lib.rs` | The way in: `create_app`, which makes the app `main` runs from the `Options` (`app/options.rs`) read from the command line, inserts them as a resource and adds `bevy_log`'s `LogPlugin`, the order of the `FollowUpSystems` in `PostUpdate` and the `SendSystems` in `Last`, every area's plugin and the runner. The crate's first comment says in which order to read the rest |
| `realm.rs`, `realm/` | The realm on disk. `Realm::open` claims a folder, opens its `Store`, checks the `realm.id` pair, and makes a new realm with its first chief and its Hand. `claim.rs` is the claim that keeps other servers out of the folder; `layout.rs`, `Layout`, names every path of the realm's folder; `change.rs`, `Change`, a change in progress: its transaction, its time, its actor and what it touched; `jobs.rs`, `Job`, names each effect a change keeps for after its commit; `settings.rs` the realm's settings with the process's overrides; `credentials.rs` the atomic credentials file and secret removal; `workspace.rs` and `workspace/` the remote workspace: each agent's session there (`serving.rs`) and a lodge's copy kept in step with the server's (`lodges.rs`) |
| `app.rs`, `app/` | The server's app shell; `app::plugin` adds its plugins. `input.rs`: everything that wakes the server (a runtime's line, a client's message or connection, a tool run that ended, a task that finished, the jobs runner's pass, a change to the tools added by hand) sends its message through the one `InputSender`; a connection holds at most a few of its messages waiting, so a client that floods slows only itself. `add_handler` adds an area's system for one kind of input message, run only when one came, which drains every one of them from its `Messages`; `InputOrigin` names who is told when an input fails. `runner.rs`: the app's runner updates the app a first time, then `Server` takes each `Input`, or a timer that came due, writes its message and updates the app once, in one transaction, until the app exits; a signal is an input that asks it to stop. `failure.rs`: it logs what failed, and only a `Fatal` error stops it. `shutdown.rs` is how it stops: the `Serving` state goes from `Open` to `Closing` (no new client, every connection closed, every sign-in ended, a credential rotation begun given 15 s) to `Stopping` (tool runs ended, runtimes stopped), and then the app exits. `startup.rs` is the first update: `PreStartup` opens the sandbox, the realm, the credentials, the providers, the jobs runner, the harness, the watch on the tools added by hand, the clients and the listener, each a resource; `Startup` starts them; and `server.json` is published once the update is over; `follow_up.rs` those of `PostUpdate` and `Last`: the input kept or rolled back and its sender told, then followed, with the observers that follow an input that ended, and what it queued sent; `tasks.rs` runs what awaits on tasks; `timers.rs` the timers each area adds; `options.rs` what the server is started with; `clock.rs` the clock; `fault.rs` the fault points |
| `core.rs` | Each area's plugin inserts the resources it keeps. `Core` is the realm's live state as handlers reach it, a `SystemParam` of the world's resources and queries, some grouped as params of their own (`ModelTools`, `RemoteWorkspace`, `Uploads`) that narrower systems take alone, with `change` and `attempt` (the second rolling back a refusal, its result a `Done`) |
| `society.rs`, `society/` | The society's domain functions, each taking a `Change`: `agents` (what every agent has), `chiefs`, `hand`, `city` (scholars), `hamlets`, `hires`, `tree`, `workers`, `roads`, `merges`, `dreams`, `library`, `record` (decisions), `limits`, `addresses`, `names` and `authority`, the one place that says who may do what. `oversight.rs` and `oversight/gate.rs` are waits, their rulings and lapses, and the gate: reach, reserved acts, manual mode, trust and grants; `spend.rs` is budgets and allowances: attempts admitted against them, usage charged once, budget resets and reports. A refusal is a `Refusal`, in words for a chief or an agent |
| `knowledge.rs`, `knowledge/` | What agents know, on disk: `run.rs` and `worker.rs` run the jobs on a thread of their own with a connection of their own (plant, lodge, commit, take-in, bring-home, erase); `folder.rs` is a folder held open and reached without following links; `edits.rs` changes a scholar's `main` or a lodge's files outside the jobs: commits, a lodge's memory and purges; `papers.rs` reads a scholar's or a lodge's papers; `merges.rs`, `dreams.rs` and `library.rs` are the repositories' side of merging hires, dreams and the library; `promotion.rs` makes a scholar of a hire, from what it read before the change; `versions.rs` is a scholar's versions as a chief or an agent reads them |
| `runtimes.rs`, `runtimes/` | The harness runtime processes, their lines, and what each is sent. `runtimes.rs` starts the pi runtime, an entity whose lines are entities related to it until written or answered, takes it on or refuses it at its hello, and starts it again after a failure; `process.rs` is its process and its lines on standard input and output; `handlers.rs` runs the system for each runtime message, those of the lifecycle of runtimes and conversations in `handlers/lifecycle.rs`, of a running turn in `handlers/turns.rs`, and of credentials and providers in `handlers/providers.rs`; `reconciliation.rs` sends each ready runtime what its conversations are owed; `leases.rs` holds credential leases |
| `conversations.rs`, `conversations/` | Conversations, their inputs and turns, prompts, and what follows an input's end. `conversations.rs` keeps the conversations; `inputs.rs` the inputs they are given with the order their turns run in, and `items.rs` the entries their runtime kept; `turns.rs` answers what a runtime requests during a turn; `completion.rs` runs once after an input ends, and `failures.rs` says why a turn failed in plain words; `imports.rs` brings in another program's project as a hamlet with a hire for each conversation, and holds each conversation until its runtime makes it; `dreams.rs` is when agents dream: the dreams asked for, which wait their turn, the last dream of a hire ended with one, the hours, who is due, and `/dream`; `chats.rs`, `reflections.rs`, `messages.rs`, `errands.rs`, `briefs.rs`, `questions.rs`, `verdicts.rs` and `tasks.rs` are each agent's one conversation (stop and its hold, resume, retry, rewind, withdraw, compaction and a merge's input), reflection, notifications, requests and their responses, one-off errands, a worker's brief, `request_chief`, the check of a final answer, and tasks; `marks.rs` is what an input is, as its runtime keeps it; `models.rs` picks each agent's model; `activity.rs` folds what an agent is doing |
| `conversations/prompt.rs`, `conversations/prompt/place.rs` | `sections` builds the prompt from the charter, machine, project, character, memory, skills, place, realm and scholars |
| `calls.rs`, `calls/` | What a runtime asks while a turn runs: whether a tool call may run (`permissions.rs`), and the call carried out once under its key (`execution.rs`); a run in progress is an entity whose task starts once its input is kept and stops with it (`tool_runs.rs`). `specs.rs` is the tools the server offers every runtime besides its own; `calls.rs` answers the runtime and puts `request_chief`'s questions to the chief. `deciding.rs` makes the gate's decision of a call in steps; `looks.rs` reads what a call waits on off the loop; `held.rs` keeps the gate's decision; `answer.rs` applies a chief's answer; `objections.rs` and `reviews.rs` are objections and a lead's review; `place.rs` says where an agent's tools work and which it is offered; `society.rs` and `society/` carry out the society's tools, each reading its arguments into a type of `society/args.rs`, whose schemas the tools are offered with: `messages.rs` notifications, requests and workers, `library.rs` the librarian, `scholars.rs` the scholars, `hamlet_acts.rs` a hamlet's settings, `agent_acts.rs` what the Hand does to an agent, and `realm.rs` the realm's charter, settings, models and shelves |
| `clients/commands.rs`, `clients/commands/` | The chief's `/` commands: each area's commands added with the system that runs them (`AddCommands`), listed to clients as deeds, each a row of a table with the function that runs it; `society.rs`, `oversight.rs`, `shelves.rs`, `versions.rs` and `promotion.rs` hold the tables and functions of those on the society, waits and grants, skills, a scholar's history and a scholar made of a hire; `targets.rs` reads what a command's words name and checks the chief's right over it |
| `attachments.rs`, `attachments/` | What a chief's line carries besides its words (C33, S29). `images.rs` and `images/` are images: uploaded in parts, each upload an entity that goes with its connection (`uploads.rs`), checked and kept once by hash in the realm's `images/` folder (`store.rs`), fitted to a model's limits (`fitting.rs`), given to an agent in its own folder (`attaching.rs`), shown by a small copy, swept when nothing uses them (`garbage_collection.rs`), and read back by id. `files.rs` is the files a chief attaches, uploaded in parts and kept once by hash in the realm's `files/` folder; `inbox.rs` saves them, and what agents send with `notify` and `request` (H30), in the receiver's `inbox/` and names where; `captions.rs` is a chief's line with its attachments, each caption set in the words at its attachment's place |
| `tools.rs`, `tools/` | What agents' tools do: the built-in tools in `builtin.rs` and `builtin/` (`files`, `shell`, `search`, `find`, `diff`, `environment` which is each agent's reach under the sandbox), the custom tools a chief adds by hand in `custom.rs`, loaded again when their folder changes, the society's tools as a type and what is known of each in `catalogue.rs`, and an edit's shown change, a diff, in `edit_diff.rs` |
| `clients.rs`, `clients/` | Connections, each an entity whose client's watches and owed replies are entities related to it (`connection.rs`, `Connections`), strict parsing of client messages (`check.rs`), the system for each request (`handlers.rs`) and what they share (`requests.rs`), the state and its changes (`state.rs`), transcripts (`fold.rs`) and the chief's documents (`documents.rs`) |
| `terminals.rs`, `terminals/` | `Terminals`: shells on pseudo-terminals, for the connections that watch them, and each agent's read-only commands terminal, which the shell tool prints to through a `Feed`; closed when their agent ends or their hamlet goes. What they print comes back as one input per batch, and a watcher that fell behind is drawn again on a timer. `screen.rs`, each one's `vt100` screen, which answers its program's cursor and device queries, the bytes that redraw it, its last command and whether one is running; `shell.rs`, starting the chief's unsandboxed shell; `chief.rs`, the system for the chief's `terminal` requests; `persistence.rs`, `restore`, which opens again at start under `terminals/` what the last stop kept |
| `providers.rs`, `providers/` | The model catalogue, each listed model read into a `Listing` (`listing.rs`), and provider requests and sign-ins |

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
| `runtime.ts` | Handshake, conversations, input admission, writes, compaction and its thresholds, configure, cancellation, history and updates |
| `extension.ts` | The hamlet extension: tools, prompt section and allow/yield hooks, each asked until the server decides; waits end on the key the server gives |
| `recovery.ts` | Carrying on a cut-off answer: summarising and the one permitted retry |
| `items.ts` | Kept entries and answers as the protocol shows them, a compaction as its summary |
| `earlier.ts` | The entries a conversation brought in from another program begins with |
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
| Typed commands, command replies | `hamlet_server/src/clients/commands.rs` and `clients/commands/`; advertised deeds sent in `clients/requests.rs` |
| An agent's conversation: stop and its hold, resume, retry, rewind, withdraw, compaction | `hamlet_server/src/conversations/chats.rs` and `conversations/completion.rs`; the order of turns in `conversations/inputs.rs` and `runtimes/reconciliation.rs`; the requests in `clients/handlers.rs` |
| Bringing in other programs' projects | Finding and reading them in `hamlet_import`; the card in `hamlet_app/src/welcome/projects.rs` with `hamlet_client/src/imports.rs`; `hamlet_cli/src/import.rs`; `hamlet_server/src/conversations/imports.rs`; pi's `earlier.ts` |
| Transcript rows, queued lines and paging | `hamlet_server/src/clients/fold.rs`, `conversations/items.rs`; `hamlet_client/src/chats.rs` |
| Requests to the chief (`request_chief`) and their cards; what an agent waits on, over its chat's message bar | `hamlet_server/src/conversations/questions.rs`, `calls/answer.rs`, `hamlet_app/src/asking.rs`, `screens/petition.rs` (`in_chat`) |
| Approvals, grants and waits | `hamlet_server/src/society/oversight/gate.rs`, `society/oversight.rs`, `calls/held.rs`, `calls/answer.rs` |
| Selecting, quoting and replying to chat text | `hamlet_app/src/select.rs`, `screens/chat/quote.rs`, `screens/chat/reply.rs` |
| Copying an agent's or a hamlet's ID, and the chips an address in the chat is drawn as | `hamlet_app/src/reference.rs`; the address found in words in `hamlet_ui/src/markdown/links.rs` |
| An address's form, `hand`, `mayor@<hamlet>`, `hire#<id>`, `worker#<id>` or `scholar#<id>`, read and written | `hamlet_protocol/src/address.rs` |
| Who may do what | `hamlet_server/src/society/authority.rs` |
| Founding, hiring, ending, restoring, seating a mayor, the tree | `hamlet_server/src/society/hamlets.rs`, `hires.rs`, `tree.rs`; the tools in `calls/society.rs`, the commands in `clients/commands.rs` |
| Scholars and their versions | `hamlet_server/src/society/city.rs`, `clients/commands/versions.rs`, `clients/commands/promotion.rs`, `knowledge/`; the repositories in `hamlet_git` |
| Merging and splitting hires | `hamlet_server/src/society/merges.rs`, `knowledge/merges.rs` |
| Workers | `hamlet_server/src/society/workers.rs`, `conversations/briefs.rs` |
| Notifications, requests, responses and roads | `hamlet_server/src/conversations/messages.rs`, `calls/society/messages.rs`, `society/roads.rs` |
| Instructions and objections | `hamlet_server/src/calls/objections.rs`, `calls/reviews.rs` |
| The library and the librarian | `hamlet_server/src/knowledge/library.rs`, `calls/society/library.rs`, `society/library.rs`, `clients/commands/shelves.rs` |
| Dreams | `hamlet_server/src/conversations/dreams.rs`, `knowledge/dreams.rs`, `society/dreams.rs` |
| Budgets, caps and allowances | `hamlet_server/src/society/spend.rs`, `society/limits.rs`, `conversations/tasks.rs` |
| A prompt or chief document | `hamlet_server/src/conversations/prompt.rs`, `clients/documents.rs`; default charter and the Hand's character in `hamlet_server/assets/` |
| The database's tables | `hamlet_store/src/schema.sql`, then `cargo test -p hamlet_store` |
| A job | `hamlet_server/src/realm/jobs.rs`, `knowledge/run.rs` |
| A model connection, login or price | `hamlet_server/src/providers.rs`, `realm/credentials.rs`; provider implementation in pi |
| Folder, mode or tool reach | `hamlet_server/src/calls/place.rs`, `tools/builtin/environment.rs`, `hamlet_sandbox`; [`sandbox.md`](sandbox.md) |
| CLI process discovery | `hamlet_cli/src/process.rs`; [`cli.md`](cli.md) |
| Installers, archives and what they ship | `.github/scripts/package.sh`, `hamlet.iss`, `.github/workflows/release.yml`; [`releases.md`](releases.md) |
| Updates | `hamlet_update`, `hamlet_app/src/updates.rs` and its toast in `toasts.rs`, `hamlet_cli/src/launch.rs`, `.github/scripts/manifest.sh`; [`releases.md`](releases.md#updates) |
| Client protocol | `hamlet_protocol/src/client/`, then `cargo test -p hamlet_protocol` for `client_schema.fingerprint`; [`protocol.md`](protocol.md) |
| Runtime protocol | `hamlet_protocol/src/runtime/`, then `cargo test -p hamlet_protocol`; [`harness.md`](harness.md) |
| Received entities | `hamlet_client/src/connection.rs`, `realm.rs`, `waits.rs`, `chats.rs`, `index.rs` |
| The message field and its chips: typing, sending, the place of each attachment, the chips in a sent line | `hamlet_app/src/screens/chat.rs` (`send_typed`), `chat/field.rs`, `chat/attach.rs` (the chips and a sent line's tiles), `chat/bubble.rs` (a sent line); the field itself is fynix's `rich_field` |
| An image of a line shown over the window | `hamlet_app/src/screens/viewer.rs`; the image itself is read in `screens/pictures.rs` (`Source::Original`) |
| Images on a line: upload, fitting, what a model takes, the read tool's images | `hamlet_server/src/attachments/images.rs`, `providers.rs`, `tools/builtin/files.rs`; in the window `hamlet_client/src/images.rs`, `hamlet_desktop/src/clipboard.rs`, `hamlet_app/src/screens/chat/attach.rs` and, for a custom model's limits, `hamlet_app/src/models/home.rs`; in the CLI `hamlet_cli/src/chat.rs` and `terminal.rs` |
| Files and folders on a line, and sent with `notify` or `request`: upload, the inbox, the cap | `hamlet_server/src/attachments/files.rs`, `attachments/inbox.rs`, `attachments/captions.rs`, `attachments/images/attaching.rs`, `tools/builtin/parcel.rs`, `calls/society/messages.rs`, `society/oversight/gate.rs` (`Call::Parcel`); in the window `hamlet_client/src/files.rs`, `hamlet_desktop/src/choose.rs`, `hamlet_app/src/screens/chat/attach.rs`; in the CLI `hamlet_cli/src/client.rs`, `chat.rs` and `mentions.rs` (the `@path` tokens) |
| Terminals: the chief's shells, the agents' commands logs, the panel | `hamlet_server/src/terminals.rs`, `terminals/`; `hamlet_os/src/process/pty.rs`; `hamlet_protocol/src/screen.rs` (resizing a screen, both ends); `hamlet_client/src/terminals.rs`; `hamlet_app/src/screens/terminal.rs`, `screens/terminal/`; [`sandbox.md`](sandbox.md#terminals) |
| A panel or right-click menu | `hamlet_app/src/screens/`, `world_menu/items.rs` |
| The Hub: its rail, a thread, its details sheet, scholar and hamlet pages | `hamlet_app/src/screens/hub.rs`, `hub/` (`rail.rs`, `thread.rs`, `details.rs`, `scholar.rs`) |
| The tag of an agent's city scholar, and the mark of what a hire learned that its scholar lacks | `hamlet_app/src/scholar_tag.rs`; `learned` on the client protocol's `Agent`, kept in `hamlet_server/src/conversations/dreams.rs` (`Learned`), found again after jobs by `knowledge/dreams.rs` (`has_learned`) |
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
