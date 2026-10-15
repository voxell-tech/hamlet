# Proposal: the agent server

Rebuild Hamlet's agent runtime as a server in TypeScript on pi-durable, with
the window as its client. The concepts and rules stay, as
[`docs/spec/`](../spec/README.md) states them; how agents are built changes.
The decisions are [ADR 0001](../adr/0001-agents-run-on-a-server-built-on-pi.md)
to [ADR 0007](../adr/0007-agents-push-through-a-git-proxy-in-the-server.md).

The work happens on its own branch, and the window keeps working on `main`
until the branch replaces the Rust runtime whole.

## The shape

```
Window (Rust, Bevy)                        Server (TypeScript, Node)
  world, panels, sound                       pi-durable Harness over the home's SQLite
  client core: the same components,  <ws>    Hamlet's extensions: prompt, tools, gate,
  filled from what the server sends            budget, society, votes, realm, library
                                             ExecutionEnv --> sandbox (bubblewrap, Seatbelt)
                                             git proxy --> git host, with the chief's token
                                             credentials file (keys, logins, tokens)
```

## The server

A `server/` folder at the repository's root, its own package. Its parts:

| Part | What it is |
|---|---|
| Home | One pi-durable storage per home (`openNodeSqliteStorage`), plus the files tools use: houses, the library's skills, tools added by hand, each hamlet's clone (ADR 0005) |
| Harness | `Harness.open` over the home, with pi-ai's models and the registry of Hamlet's extensions. `resume()` at start carries on interrupted work (S3) |
| Extensions | One per concept, below |
| Environment | Hamlet's `ExecutionEnv`, built per tool call from the conversation and its hamlet: runs commands and file operations under the ported sandbox policy (ADR 0003) |
| Git proxy | Smart HTTP for the hamlet's one repository: checks the hamlet's credential, refuses deletions and tags, adds the chief's token (ADR 0007) |
| Credentials | One file in the home, mode 0600, read into pi-ai's credential store; the git tokens beside them (S7) |
| Connection | One WebSocket, with a token (ADR 0004) |
| CLI | `hamlet`, below (S13) |

pi-durable is pinned to 1.2.0 and upgraded on purpose: its API is marked
experimental.

### The home on disk

The server opens a fresh home; it does not import a Rust home's files.
`HAMLET_HOME` names it, otherwise `~/.hamlet`. Development uses a scratch
home. One process holds the home's lock under the Rust app's socket name.

| Path under the home | Holds |
|---|---|
| `home.sqlite` | pi-durable's storage: conversations, characters, the hamlet's record, oversight, spend, limits, settings and the catalogue |
| `credentials.json` | The installation ID, connection token, model credentials and git tokens |
| `models.json` | pi-ai's stored model lists |
| `server.json` | The running server's URL and process ID, with no token |
| `houses/<hamlet>/<number>/` | The agent's writable files: `NOTES.md`, `skills/` and `inbox/` |
| `library/` | Shared skills, starting with the starters, readable by agents |
| `tools/` | Tools added by hand, readable by agents |

The first hamlet is `home`. Its founder is agent number 0, named `founder`,
with house `houses/home/0/`; `mayor` addresses the office it holds. Character
and conversations are in storage, not writable files in the house.

`credentials.json` has mode 0600 and is replaced whole through a temporary
file and a rename. It holds `installationId`, `connection: {token}`,
`models: {provider: credential}` and `git: {name: token}`. pi-ai receives no
ambient credentials; `HAMLET_API_KEY` is held in memory only. ChatGPT uses
its own `chatgpt` provider and credential, beside an `openai` key, with
Hamlet's installation ID and name. ChatGPT and OpenRouter are the offered
sign-ins; other providers use keys.

### The `hamlet` command

The server's package supplies `hamlet` through `server/bin/hamlet`. With
the Node version in `server/.nvmrc`, run `npm ci` and `npm link` in
`server/` to put it on PATH. `npm run hamlet -- <command>` runs the same
executable from that folder. An old Cargo-installed `hamlet` can shadow it.
After `cargo build --bin hamlet-app` at the repository root:

```sh
HAMLET_HOME=/tmp/hamlet-try hamlet dev
```

| Command | Does |
|---|---|
| `hamlet server [--port <n>] [--faux <script.json>]` | Starts a server on a home; a faux script answers offline for tests and demos |
| `hamlet chat [--to <agent>]` | Chats with the mayor or the named agent and answers what waits, in the terminal |
| `hamlet dev [--port <n>] [--faux <script.json>] [--home <dir>]` | Serves a scratch home on a free port unless given one, and opens its window |
| `hamlet window` | Opens a window connected to a running server |
| `hamlet edit <charter\|character\|notes\|limits\|settings>` | Opens the chief's document in `$EDITOR` and saves through the server; limits and settings are JSON |
| `hamlet login <provider> [--key]` | Gives a key or follows an offered sign-in |
| `hamlet models [try [model]\|refresh]` | Lists models, tries a one-word hello, or refreshes the list |

The server listens on `127.0.0.1` at port 7313 unless given another port.
Local clients find its URL through `server.json` and its token through
`credentials.json`; elsewhere they use `HAMLET_SERVER` and `HAMLET_TOKEN`,
through an SSH tunnel. The server runs for one chief, named by
`HAMLET_USER`, else `USER`, else `user`. Closing a client stops no work;
end of input waits for that client's submissions and triggers no reflection.

The step-2 commands are `/new`, `/resume [n]`, `/retry`, `/reflect`,
`/stop [agent|all]`, `/queue`, `/allow`, `/always`, `/deny`, `/dismiss`,
`/grants`, `/revoke`, `/budget`, `/reset-budget`, `/folder`, `/mode` and
`/give <agent> <skill> [from <agent>]`. `/undo` says there is nothing to undo. The server
parses commands; the chat client handles only `/agent` and `/hamlet` as
address changes. Society and realm commands come with their later steps.

The folder is a path on the server's machine, with earlier folders kept;
the clone given by URL comes in step 6. Manual mode holds folder edits and
commands, not edits of the agent's own house. Tools added by hand reach the
paths their manifests grant, not the shell's paths. There is no
`--insecure` mode yet.

Agents may write git's files in the chief's own folder, including nested
repositories' hooks and config. The chief accepts that these may later run
under the chief's own git. In step 6 the folder becomes a clone Hamlet
keeps (S9): the chief never runs git there, and Hamlet runs its own git with
hooks off, so these files need no guard.

`hamlet dev` chooses its home from `--home`, then `HAMLET_HOME`, then a
per-user temporary folder kept between runs. It refuses `~/.hamlet` and
sets `HAMLET_VIEW=<home>/window`. Closing its window keeps the server
running; Ctrl-C stops both. `hamlet window` finds the app by `HAMLET_APP`,
then PATH, then the repository's `target/release` and `target/debug`.
It passes the server address and token through the environment, never argv.
Players still open an app called Hamlet; only its executable is named
`hamlet-app`. Release archives contain the window, not the server.

The window has no home lock and runs no agent. It finds a local server
through the home's `server.json` and connection token, or uses
`HAMLET_SERVER` and `HAMLET_TOKEN`. Its own size, layout, chief's look and
places live on its machine under `HAMLET_VIEW`, else the OS's config folder
for Hamlet. Local links and pictures resolve there too: existing absolute
paths or paths under the received work folder, never the server's houses.
Charter and character edits use local temporary files, with each save sent
through `edit`.

### Hamlet on pi-durable

| Hamlet (spec) | pi-durable |
|---|---|
| A conversation (C22) | A conversation; the chief's, the letters' and the questions' each one |
| The prompt's parts (C11) | Sections of Hamlet's extensions, rendered before each request; the budget section reads words fixed at the turn's start |
| Tools by role (H35), a ballot's one tool | `configure()` of the conversation's tools; applies from the next request |
| Read, write, edit, bash, search, find (C16) | pi-durable's `read`, `write` and `edit` on Hamlet's environment; Hamlet's `bash` wraps pi's with the network flag, timeout and scrubbed output; `search` and `find` are Hamlet's own |
| Tools added by hand (C17) | Tools registered by a loader extension from the home's tools folder |
| The gate (C7, H11) | A `beforeTool` hook in the oversight extension |
| The hamlet's record, the realm, oversight, the library's record, limits, settings | Session-scoped documents; a tool commits a change with a marker of its result, so a repeated call returns that result (ADR 0005) |
| A task and its caps (H12) | A document of the task's counts, read by the gate and the budget check |
| Spend (H8) | pi-durable's kept responses and aggregate summary usage, charged in integer micro-dollars to Hamlet's ledger by hamlet and local day, agent and task; roads join it in step 4 |
| Letters (H30) | A submission to the receiver's conversation with the sender; its answer is submitted back to the sender's conversation |
| Questions (H30) | A tool that submits to the receiver's conversation and waits for it, as pi-durable's subagent example does, so it survives a restart |
| Votes (H13) | A task that owns the mayor's turn and each ballot as child tasks |
| Reflection (C5) | A queued server input with its own task and a reserved ordinal, kept across a restart; zero allows only reflection a chief asks for; with a nonzero count, an orderly close waits up to 120 s for owed reflections able to run |
| Summarising (C28) | pi-durable's compaction, set to C28's thresholds |
| Taking a conversation back (C27) | A fork from the entry before the message, with the conversation pointed at the fork (ADR 0002) |
| What the window watches (S5) | `watchEvents()` per open conversation, and the documents' changes for the realm's state |

### What leaves Rust

`hamlet_core`'s runtime, `hamlet_rules`, `hamlet_model`, `hamlet_home` and
`hamlet_sandbox` are being rebuilt on the server, the sandbox policy and
rules ported with their tests. They remain compiled and tested as porting
references until step 7; no window crate depends on them, guarded by
`tests/window_is_a_client.rs`.

The window keeps `hamlet_world`, `hamlet_ui`, `hamlet_desktop`, `hamlet_vfx`
and `hamlet_assets`. `hamlet_client` fills the components and events the
world and panels read: `Settlement`, `Citizen`, `Mayor`, `Busy`, `Activity`,
`Transcript`, `Status`, `Usage`, `Waiting`, `RaisedBy`, and `UserMessage`
and `SessionOutput` as messages to and from the server. Road, departure,
letter and vote components are retained without producers until their
server steps.

## The protocol, version 0

Defined once in TypeBox in `server/src/protocol.ts`.
`server/protocol.schema.json` is the full definition, exported by
`npm run schema` and checked by a test. `cargo run -p hamlet_protocol_gen`
generates `crates/hamlet_protocol/src/v0.rs` from it, checked in and guarded
by a schema-drift test. The terminal clients use the same protocol as the
window. Faux sessions recorded in `server/test/frames/*.jsonl` are checked
against the schema by the server tests and replayed by the Rust client
tests. Re-record them in `server/` with
`HAMLET_RECORD_FRAMES=1 npx vitest --run test/frames.test.ts`.

| Direction | Message | Carries |
|---|---|---|
| Client -> server | `hello` | Version 0, the token, and optionally the last epoch and revision seen |
| | `say` | A typed line to an agent: words or a command (H24) |
| | `give` | A file manifest to an agent, or one small skill gift (H25) |
| | `give-part` | Base64 bytes at the next offset of a file in an accepted upload |
| | `answer` | A wait's ID, a choice and optional words (H11, H10) |
| | `stop`, `rewind`, `new`, `resume`, `retry` | C24 to C27; resume uses a stable conversation number |
| | `edit` | Charter, character, notes, limits, settings or a catalogue model's allowance, price and note |
| | `open`, `close` | Start and stop watching a conversation; open can request a page before an item |
| | `login` | Start a key or subscription login, reply to a step, or cancel it |
| | `read` | Prompt, history, models, papers (notes, character and skills), or charter |
| | `check` | Try a model's one-word hello, or check the default model's connection |
| Server -> client | `state` | The whole realm's state, with an epoch and revision |
| | `change` | Whole entities replaced or removed, at a later revision |
| | `turn` | Item-level operations on an open conversation (C30) |
| | `transcript` | A page of a conversation, its earlier-page cursor, work status and usage |
| | `reply` | Notice, error or rewound words, addressed to a request |
| | `login` | A URL, code, prompt, progress, completion or failure for a login |
| | `read` | The requested prompt, history, models, papers or charter |
| | `fault` | Something that failed, in words for a chief, optionally addressed to a request |

### IDs and conversations

Every request except `hello` and `close` carries a client-made UUID as
`id`; a reply echoes it as `to`. A typed line is submitted with request ID
`chief:<id>`, so sending it again to the same conversation admits it once.
A retry is `retry:<uuid>`. A reflection is `reflect:<number>:<ordinal>`,
where the number is the conversation's stable number and the ordinal grows
across the chief's chat with that agent. Its reservation is kept before
submission, so a restart between the two submits the same ID.

One chief's chat with one agent is shared by every client. Its conversations
are numbered from 1, with a current conversation and a history. `/new` moves
the current pointer at once; the earlier conversation finishes its queue
and, unless reflection is set to zero, reflects on owed turns. `/resume`
alone lists history, the non-current conversations, and `/resume <n>` returns
by number. A rewind forks the current conversation but keeps its
number. An address names a hamlet and agent, with `mayor` for the office;
a chat reference adds optional `past` for a numbered conversation.

### State and transcript

State carries the chief and sandbox, hamlets with their folder, earlier
folders, mode, limits and budget, agents with their number, model, activity,
words, spend and busy or faulted state, open waits, grants, settings and
providers. Settings show endpoints without keys, whether setup is needed,
and the last connection check. Providers show their available login methods.
A server start gives a new epoch; revisions grow within it. A connecting
client receives the current state. Changes replace or remove whole entities,
and carry ready-made words beside numbers so the client holds no rules.
State also carries optional `deeds`, the supported chief-command names,
and `starters`, the shipped skill descriptions; each agent's optional
`skills` lists its house's shelf. The window offers command deeds only
when advertised, and sends file and skill gifts over the protocol.

`hamlet_client` reconciles state and changes into entities, keeping the
entities of hamlets and agents still present. It sends `hello` with its
last epoch and revision when reconnecting, retains unsent requests in its
Outbox and opens its watched chats again. A lost connection retries; a
refused token stops retries and shows the server's words.

A transcript item has ID `<entryId>:<block>`, a time and a kind: chief input,
assistant text, reasoning, tool, notice or error. Tool items carry the name,
argument summary, change, exit, time and failure. Turn operations add or
replace an item, append text, close a tool, change status or usage, or reset
the transcript before sending it again. Transcripts and turns carry an
optional stable conversation `number`, so a client can keep watching an
older conversation after `/new`. `open` accepts `before` for paging and
`transcript.earlier` gives the next cursor. An older-page request leaves an
existing chat watch running, even if that page cannot be read. Transcripts
also carry optional `last` activity (`what` and `at`), as status ops do, so a
late-opened chat shows the same last tool call, reply or error as a live one.

An empty notice `reply` to a `say` acknowledges that its submission settled,
not that all work in the conversation ended. A `rewound` reply gives the
removed chief words back to the input field. A request's reply is not ordered
against a chat's `turn` and `transcript` messages. A reset and its replacement
transcript are sent together, with no message between them. Reflections and
retries show as notices, as does a compaction.

### Gifts and message sizes

Every client message, including `hello` before authentication, is capped at
1 MiB of wire bytes. A larger message closes the connection with code 1009.

A file gift begins with `give {id, to, files: [{path, size}]}`. A folder is
its files, with relative paths under the inbox. The server refuses more than
32 MiB in total, more than 4096 files (including empty ones), unsafe paths,
paths repeated after NFC normalization and case-insensitive Unicode folding,
file/folder conflicts, and an absent agent, with an error `reply` addressed to
that `id`. Each connection may hold at most three unfinished gifts. Across
all connections, each receiving agent may have at most 32 MiB reserved by
accepted gifts, including ones being delivered. A refusal creates no stage.
The window holds none of these rules. An accepted manifest receives a notice
`reply` with an optional `upload` id.

The same connection sends `give-part {id, upload, path, offset, data}` under
the message cap, where `data` is base64 and `offset` is that file's next byte.
Each part gets a reply to its own id. The last part's notice names the inbox
paths and the server notifies the agent. An all-empty gift completes with
the manifest's reply and needs no parts. Bytes stay in private staging outside
every house until all files are complete. Once the last bytes have been
written and the files closed, the upload is detached from its connection,
its idle timer is cleared, and delivery begins. A disconnect before that
point discards the whole upload, waiting for any in-flight staging or write
to end before removing its stage. A disconnect after that point changes
nothing: delivery and the agent's notification finish, even though the
client may not receive its receipt. No discard races a staging step or copy.

Delivery runs through the receiving agent's `SandboxEnv.exec`, with only
its house writable and its stage readable. A folder target is claimed with
`mkdir --` before `cp -RP` copies its contents; a file target is claimed with
a no-clobber open. An existing target ends delivery without changing it.
The sandbox checks each write's resolved path, so a replaced inbox link
cannot direct a write outside the house. A missing inbox is recreated.
A failed copy removes only the destinations it claimed under the same policy;
staged bytes are removed after the copy ends. Failures give words without
server paths. An unfinished upload idle for 60 seconds is discarded as a
disconnected one is. A new server discards staging left by its predecessor.
Upload ids cannot be used by another connection.

A skill stays one message: `give {id, to, skill: {starter}}` or
`give {id, to, skill: {from: Address, name}}`. It shares the typed `/give`
deed and records the resolved giver's name, with its hamlet when different.
A skill from another agent is copied in two sandboxed steps. The first can
read the giver's whole house, granted by its server-owned path, and write
only a private staging folder. It copies without following links, then the
server checks the staged tree for links, special files, more than 4096 entries
and more than 32 MiB. The receiving agent's sandbox delivers the checked stage
as it delivers a file gift. Shipped starters are checked and delivered from
their server-owned folder. A missing skills folder is recreated. A 33 MiB
sandbox file limit bounds a growing source while allowing Linux `cp` to
probe EOF of an exactly 32 MiB file; the accepted gift still takes at most
32 MiB.

State lists starters with shipped descriptions and each agent's skill folder
names only. State, papers and prompts share a regular-file, no-link listing,
cached for at most one second and invalidated by gifts, commits and a turn's
end. Houses are never watched recursively. House skill papers are never
opened to build state. Prompts and papers list a house's shelf and read all its descriptions and
usage in one request to a read-only sandbox over the whole house. The reader
caches papers by device, inode, modification time, change time and size,
checking those inside the sandbox on each request. It uses a no-follow,
non-blocking open, accepting only regular files and reading at most 64 KiB
each. A swapped shelf or parent folder cannot disclose outside names or
papers. The prompt's Run command starts with `cd '<house>' &&`.

Every file operation of `SandboxEnv`, including readers, watchers, metadata
and temporary files, runs in a persistent sandboxed Node helper. The first
file operation starts it; `cleanup()` ends it. Only Node, the helper and its
runtime files are added to the policy's readable paths, and no ambient
environment is inherited. The kernel checks the path actually opened.
Environments with the same home, working directory and policy share a helper.
A dead helper is replaced on demand. Each request has at most 30 seconds,
or the policy's shorter wall limit; an overrun kills the helper and fails
its in-flight requests, without retrying a possibly completed write. A helper
idle for 30 seconds exits, and readers left open then report that they are
closed. An active watch keeps its helper alive until closed. File size and
core limits still hold, but a command's CPU and process limits do not apply
to the persistent helper. Malformed frames fail their addressed request or
watch. House skill shelves have separate read-only helpers.

An approval wait requires its `danger`; fault, question and objection waits
carry `danger: null`.

### Documents, models and logins

`read` returns the prompt as it would be sent now, numbered history, the
model catalogue, papers or the charter with whether it is the chief's own.
A papers read cuts notes and character to 6000 characters unless `full` is
true, which keeps the text whole for editing. A models read may refresh
the list. Catalogue entries give price and its source, context size,
allowance and note.

`edit` accepts the charter or its default, an agent's notes or character,
a limits key or a whole limits document, settings fields, or catalogue
fields. A refused edit changes nothing and replies with an error.
`check` replies in words and changes the connection status in settings;
a model check waits at most 90 s. Login replies belong to the connection
that started the login; disconnecting it cancels the flow. Credentials
stay on the server.

Tool results, protocol messages and the oversight record are scrubbed of
known credentials. An assistant's own messages keep their text, reasoning
and tool-call arguments unscrubbed in pi's storage; C32 is not yet met.

## Order of work

1. **The spike. Done.** The eight offline proofs below establish pi-durable's
   request hooks, durable waits and questions, sandbox interface, git bridge
   and push checks, and safe record changes with result markers.
2. **The mayor alone. Done.** One home with one hamlet and its mayor: chat,
   tools in the sandbox, notes and skills, queued reflection, manual and
   auto with the gate, spend and limits, models and logins. The npm terminal
   commands drive protocol v0, and interrupted work resumes on reopening.
3. **The window's client core. Done.** The world and panels read
   `hamlet_client`'s entities from protocol v0. Chats, approvals, history,
   papers, settings, models, logins, edits and gifts go through the server.
   The binary and packaged executable are `hamlet-app`; `--headless`,
   `--repl`, `--insecure` and `--replay` leave it. `hamlet dev` starts the
   pair safely; the window keeps its own view files, and closing it stops
   no agent.
4. **The society**: agents, roads, letters and questions, tasks, leaving and
   coming back, votes, the library, objections and questions to the chief.
5. **The realm**: several hamlets, roads and moves between them, founding
   and removing.
6. **The clone and the git proxy.**
7. **Deleting the Rust runtime crates**, once the server does everything they
   did.

Step 3 leaves these to steps 4 and 5, rather than keeping rules or home
reads in the window:

- **Step 4:** agents beyond the mayor, roads, letters and questions, tasks
  across agents, removal, merge and restore, votes and their record, the
  shared library shelf and its record, objections and questions to the
  chief. The corresponding command deeds stay hidden unless the server
  advertises them. File and skill gifts already work through `give`.
- **Step 5:** several hamlets, founding, naming, moving, closing and
  restoring them, closed-hamlet listings, and dealings between hamlets.
  The overview shows the hamlets and budget lines received now; it reads
  no realm or vote records from disk.

## The spike

The eight proofs are tests against pi-durable 1.2.0 with a fake provider,
offline. They pass in `server/test/`.

| # | To prove | Fails if |
|---|---|---|
| 1 | A section and `configure()` changed between two requests of one run reach the second request: the prompt, the tools and the model | A change lands only on the next run |
| 2 | A budget check before a request can end the run cleanly, with the turn shown as stopped (H8) | The run cannot be ended there, or leaves a broken transcript |
| 3 | A `beforeTool` hook can wait minutes for a chief, and a restart while it waits brings the wait back, not an `interrupted` result | The wait cannot survive a restart in any form pi-durable allows |
| 4 | A question between agents (H30) survives a restart of the server on either side | The asker or the asked loses its place |
| 5 | Hamlet's `ExecutionEnv` under bubblewrap and Seatbelt passes pi-durable's `registerEnvConformance` | It cannot meet the interface |
| 6 | A command in the sandbox with no network reaches the server's git proxy: a bridge under bubblewrap, one local port under Seatbelt | No way in without opening the network |
| 7 | The git proxy reads a push's ref updates before the pack and refuses a deletion and a tag | It has to buffer whole packs, or cannot see the refs |
| 8 | A Hamlet document change commits with a marker of the call's result; a call cut off before pi keeps the result returns the marked result when it runs again (ADR 0005) | The change can happen twice, or the call loses its result |

## Risks

- pi-durable is experimental, and 1.2.0 already broke its tool API. Pinned,
  and reached only through Hamlet's extensions.
- `node:sqlite` is marked experimental in Node 24.
- OpenAI's login listens on port 1455, which the Codex CLI uses too; a clash
  fails the login with a clear error.
- Windows has no sandbox, and the server offers no `--insecure` mode.
- C31 is not yet met: first-token timing is not recorded for every call;
  raw provider usage and each summarising call's timing, billed charge and
  end reason are not recorded. Summaries' model time is not yet charged.
- C32 is not yet met for assistant messages kept by pi, including their
  text, reasoning and tool-call arguments.

## Left for later

Not part of this proposal: where the server is deployed and where tools run
then; a GitHub App for tokens; several chiefs on one home; several folders
per hamlet; syncing a folder from the chief's machine; a window that starts
its own server. Whether to import Rust homes is decided before step 7.
