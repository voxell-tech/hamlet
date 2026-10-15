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

The server's package runs the commands through npm, from `server/`, using
the Node version in `.nvmrc`: `npm run hamlet -- <command>`. For example:

```sh
HAMLET_HOME=/tmp/hamlet-try npm run hamlet -- server
```

| Command | Does |
|---|---|
| `hamlet server [--port <n>] [--faux <script.json>]` | Starts a server on a home; a faux script answers offline for tests and demos |
| `hamlet chat [--to <agent>]` | Chats with the mayor or the named agent and answers what waits, in the terminal |
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
`/give` for starters. `/undo` says there is nothing to undo. The server
parses commands; the chat client handles only `/agent` and `/hamlet` as
address changes. Society and realm commands come with their later steps.

The folder is a path on the server's machine, with earlier folders kept;
the clone given by URL comes in step 6. Manual mode holds folder edits and
commands, not edits of the agent's own house. Tools added by hand reach the
paths their manifests grant, not the shell's paths. There is no
`--insecure` mode yet.

Step 3 gives the package the `hamlet` executable, renames the Rust binary
`hamlet-app`, and adds `hamlet dev` to start a scratch server and its window
and `hamlet window` to open a window pointed at a server. Players still open
an app called Hamlet; only its executable is named `hamlet-app`.

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
| Spend (H8) | pi-durable's kept responses and aggregate summary usage, charged to Hamlet's ledger by hamlet and local day, agent and task; roads join it in step 4 |
| Letters (H30) | A submission to the receiver's conversation with the sender; its answer is submitted back to the sender's conversation |
| Questions (H30) | A tool that submits to the receiver's conversation and waits for it, as pi-durable's subagent example does, so it survives a restart |
| Votes (H13) | A task that owns the mayor's turn and each ballot as child tasks |
| Reflection (C5) | A queued server input with its own task and a reserved ordinal, kept across a restart; an orderly close waits up to 120 s for owed reflections able to run |
| Summarising (C28) | pi-durable's compaction, set to C28's thresholds |
| Taking a conversation back (C27) | A fork from the entry before the message, with the conversation pointed at the fork (ADR 0002) |
| What the window watches (S5) | `watchEvents()` per open conversation, and the documents' changes for the realm's state |

### What leaves Rust

`hamlet_core`'s runtime, `hamlet_rules`, `hamlet_model`, `hamlet_home` and
`hamlet_sandbox` are rebuilt on the server, the sandbox policy and the
rules ported with their tests. The window keeps `hamlet_world`, `hamlet_ui`,
`hamlet_vfx` and `hamlet_assets`, and gains a client core that fills the
components and events the world and panels read today: `Settlement`,
`Citizen`, `Mayor`, `Busy`, `Activity`, `Road`, `Transcript`, `Status`,
`Usage`, `Waiting`, `RaisedBy`, and `UserMessage` and `SessionOutput` as
messages to and from the server.

## The protocol, version 0

Defined once in TypeBox in `server/src/protocol.ts`.
`server/protocol.schema.json` is the full definition, exported by
`npm run schema` and checked by a test. The Rust types are generated from
it in step 3. The terminal clients use the same protocol as the window.

| Direction | Message | Carries |
|---|---|---|
| Client -> server | `hello` | Version 0, the token, and optionally the last epoch and revision seen |
| | `say` | A typed line to an agent: words or a command (H24) |
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
and reflects on owed turns. `/resume` alone lists history and `/resume <n>`
returns by number. A rewind forks the current conversation but keeps its
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

A transcript item has ID `<entryId>:<block>`, a time and a kind: chief input,
assistant text, reasoning, tool, notice or error. Tool items carry the name,
argument summary, change, exit, time and failure. Turn operations add or
replace an item, append text, close a tool, change status or usage, or reset
the transcript before sending it again. Transcripts and turns carry an
optional stable conversation `number`, so a client can keep watching an
older conversation after `/new`. `open` accepts `before` for paging and
`transcript.earlier` gives the next cursor.

An empty notice `reply` to a `say` acknowledges that its submission settled,
not that all work in the conversation ended. A `rewound` reply gives the
removed chief words back to the input field. Reflections and retries show
as notices, as does a compaction.

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
known credentials. Assistant text can reach pi's storage before it can be
scrubbed; C32 is not yet met.

## Order of work

1. **The spike. Done.** The eight offline proofs below establish pi-durable's
   request hooks, durable waits and questions, sandbox interface, git bridge
   and push checks, and safe record changes with result markers.
2. **The mayor alone. Done.** One home with one hamlet and its mayor: chat,
   tools in the sandbox, notes and skills, queued reflection, manual and
   auto with the gate, spend and limits, models and logins. The npm terminal
   commands drive protocol v0, and interrupted work resumes on reopening.
3. **The window's client core**, so the world and panels run on the server.
   The Rust binary is renamed `hamlet-app` here, with the packaging script
   and `AGENTS.md`, and `--headless` and `--repl` leave it.
4. **The society**: agents, roads, letters and questions, tasks, leaving and
   coming back, votes, the library, objections and questions to the chief.
5. **The realm**: several hamlets, roads and moves between them, founding
   and removing.
6. **The clone and the git proxy.**
7. **Deleting the Rust runtime crates**, once the server does everything they
   did.

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
- C31's first-token timing is conditional; raw provider usage and each
  summarising call's timing, billed charge and end reason are not recorded.
- C32 is not yet met for assistant text kept by pi.

## Left for later

Not part of this proposal: where the server is deployed and where tools run
then; a GitHub App for tokens; several chiefs on one home; several folders
per hamlet; syncing a folder from the chief's machine; a window that starts
its own server. Whether to import Rust homes is decided before step 7.
