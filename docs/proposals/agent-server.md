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
  client core: the same components,  ◄ws►    Hamlet's extensions: prompt, tools, gate,
  filled from what the server sends            budget, society, votes, realm, library
                                             ExecutionEnv ──► sandbox (bubblewrap, Seatbelt)
                                             git proxy ──► git host, with the chief's token
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

pi-durable's version is pinned and upgraded on purpose: its API is marked
experimental.

### The `hamlet` command

The server's package owns the `hamlet` command, and the Rust binary is
renamed `hamlet-app`. Every job of the command besides opening the window
belongs to the server: today's `--headless` becomes the server and `--repl`
becomes `hamlet chat`.

| Command | Does |
|---|---|
| `hamlet server` | Starts a server on a home |
| `hamlet dev` | Starts a server on a scratch home and opens the window connected to it |
| `hamlet chat` | Chats with agents and answers what waits, in the terminal |
| `hamlet window` | Opens the window (`hamlet-app`) pointed at a server |

Players still open an app called Hamlet; only the executable inside it is
named `hamlet-app`.

### Hamlet on pi-durable

| Hamlet (spec) | pi-durable |
|---|---|
| A conversation (C22) | A conversation; the chief's, the letters' and the questions' each one |
| The prompt's parts (C11) | Sections of Hamlet's extensions, rendered before each request; the budget section reads words fixed at the turn's start |
| Tools by role (H35), a ballot's one tool | `configure()` of the conversation's tools; applies from the next request |
| Read, search, find, edit, shell (C16) | pi-durable's `read`, `write`, `edit` and `bash` on Hamlet's environment, plus Hamlet's own `search` and `find`, which pi-durable lacks |
| Tools added by hand (C17) | Tools registered by a loader extension from the home's tools folder |
| The gate (C7, H11) | A `beforeTool` hook in the oversight extension |
| The hamlet's record, the realm, oversight, the library's record, limits, settings | Session-scoped documents, committed with the tool result that changed them |
| A task and its caps (H12) | A document of the task's counts, read by the gate and the budget check |
| Spend (H8) | pi-durable's usage documents, plus Hamlet's ledger document for roads and the period |
| Letters (H30) | A submission to the receiver's conversation with the sender; its answer is submitted back to the sender's conversation |
| Questions (H30) | A tool that submits to the receiver's conversation and waits for it, as pi-durable's subagent example does, so it survives a restart |
| Votes (H13) | A task that owns the mayor's turn and each ballot as child tasks |
| Reflection (C5) | An input submitted by the server, with an entry of Hamlet's kind |
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

Defined once in TypeBox in `server/`, exported as JSON Schema, with the Rust
types generated from it.

| Direction | Message | Carries |
|---|---|---|
| Window → server | `hello` | The token, and the last state the window saw |
| | `say` | A typed line to a conversation: words or a command (H24) |
| | `answer` | An answer to something that waits (H11, H10) |
| | `stop`, `rewind`, `new`, `resume`, `retry` | C24 to C27 |
| | `edit` | A chief's document: charter, character, notes, limits, settings |
| | `open`, `close` | Start and stop watching a conversation |
| | `login` | Start a subscription login, and its steps' replies |
| Server → window | `state` | The whole realm's state |
| | `change` | One change to it |
| | `turn` | A live event of an open conversation (C30) |
| | `transcript` | An opened conversation as it stands |
| | `login` | A login's next step: a URL to open, a code to show |
| | `fault` | Something that failed, in words for a chief |

## Order of work

1. **The spike**, below. If it fails, this proposal is revisited before
   anything else is built.
2. **The mayor alone**: one hamlet, its mayor, chat, the tools in the
   sandbox, notes and skills, reflection, manual and auto with the gate,
   spend and limits, models and logins. `hamlet chat` drives it.
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

What has to be true before the rest is built. Each is a small test against
pi-durable 1.2.0 with a fake provider, offline.

| # | To prove | Fails if |
|---|---|---|
| 1 | A section and `configure()` changed between two requests of one run reach the second request: the prompt, the tools and the model | A change lands only on the next run |
| 2 | A budget check before a request can end the run cleanly, with the turn shown as stopped (H8) | The run cannot be ended there, or leaves a broken transcript |
| 3 | A `beforeTool` hook can wait minutes for a chief, and a restart while it waits brings the wait back, not an `interrupted` result | The wait cannot survive a restart in any form pi-durable allows |
| 4 | A question between agents (H30) survives a restart of the server on either side | The asker or the asked loses its place |
| 5 | Hamlet's `ExecutionEnv` under bubblewrap and Seatbelt passes pi-durable's `registerEnvConformance` | It cannot meet the interface |
| 6 | A command in the sandbox with no network reaches the server's git proxy: a bridge under bubblewrap, one local port under Seatbelt | No way in without opening the network |
| 7 | The git proxy reads a push's ref updates before the pack and refuses a deletion and a tag | It has to buffer whole packs, or cannot see the refs |
| 8 | A tool result and a Hamlet document change commit together, and a crash between them leaves neither | They commit apart |

## Risks

- pi-durable is experimental, and 1.2.0 already broke its tool API. Pinned,
  and reached only through Hamlet's extensions.
- `node:sqlite` is marked experimental in Node 24.
- OpenAI's login listens on port 1455, which the Codex CLI uses too; a clash
  fails the login with a clear error.
- Windows has no sandbox, as today.

## Left for later

Not part of this proposal: where the server is deployed and where tools run
then; a GitHub App for tokens; several chiefs on one home; several folders
per hamlet; syncing a folder from the chief's machine; a window that starts
its own server.
