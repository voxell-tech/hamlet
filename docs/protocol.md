# Protocol v0

The window, `hamlet chat` and the CLI use one authenticated WebSocket to
Hamlet's server. There is no HTTP API. The wire carries JSON messages.

The contract is [`server/src/protocol.ts`](../server/src/protocol.ts),
exported as [`server/protocol.schema.json`](../server/protocol.schema.json).
The names below, such as `Hello` and `ClientMessage`, are its schema names.
`?` means optional; `null` is an explicit value. Objects reject extra fields.

## Transport and handshake

The server listens on `127.0.0.1` only, over plain `ws://`. Local clients
read the URL from `server.json` in the home and `connection.token` from
`credentials.json` there. The home is `HAMLET_HOME`, else `~/.hamlet`.

For a server on another machine, forward its loopback port through SSH.
Set `HAMLET_SERVER` to the tunnel's local `ws://127.0.0.1:<port>` URL and
`HAMLET_TOKEN` to that remote home's token. `HAMLET_SERVER` requires
`HAMLET_TOKEN`; it bypasses local discovery. The server itself still binds
to loopback. Keep the token private; plain WebSocket supplies no encryption.

Send `Hello` first, within 10 seconds:

```json
{"type":"hello","v":0,"token":"<home token>"}
```

A successful hello receives a full `state` before live changes. `Hello.seen`
accepts `{epoch, revision}`, but the server always sends the full state.

| Failure | Answer and connection |
|---|---|
| No hello in time, or another message first | `fault`: `A connection begins with hello and the home's token.`; close **1008** |
| Wrong token | `fault`: `That is not this server's token.`; close **1008** |
| Malformed hello, including the wrong `v` | Validation `fault`; close **1008** |
| A second hello | `fault`: `This connection said hello already.`; stays open |
| Unexpected connection failure | `fault`; close **1011** |

A client-to-server WebSocket message holds at most **1,048,576 bytes**
(1 MiB). An oversized message gets a best-effort `fault` and close **1009**.
The server applies no matching outbound limit to state or transcript pages.
File gifts have a separate
**32 MiB** limit and travel in chunks (`Give`, `GivePart`). A server shutdown
closes clients with **1001**. These rules are in `connection.ts`'s `serve`.

## Shared shapes

| Schema | Shape and meaning |
|---|---|
| `Address` | `{hamlet: string, agent: string}`. An agent's name, with `agent: "mayor"` for the office's current holder. |
| `AgentRef` | `{hamlet: string, number: integer}`. Stable identity, independent of name or office. |
| `ChatRef` | `{with: Address, past?: integer, from?: AgentRef, kind?: "letters" \| "questions"}`. |
| `RequestId` | A UUID string. |
| `Seen` | `{epoch: integer, revision: integer}`. |
| `Choice` | `allow`, `always`, `deny`, `go`, `withdraw`, `reply`, `dismiss`. Use the choices advertised by the wait. |
| `Endpoint` | `{name: string, baseUrl: string}`. |
| `Price` | `{input: number, output: number}`: dollars per million tokens. |
| `Amount` | `{tokens: integer, micros: integer, ms: integer}`: tokens, millionths of a dollar, milliseconds. |

`ChatRef.with` selects the agent the chief talks to. Without `past`, it
selects the current chief conversation; `past` selects a stable conversation
number. A current watch addressed to `mayor` follows the office when its
holder changes. `from` selects a peer conversation with that stable agent;
`kind` selects its letters or questions lane, defaulting to letters.
`from` and `past` together are refused.

Every client message except `hello` and `close` carries `id: RequestId`.
The answer's `to` matches it. Do not confuse that with `Say.to`, which is an
`Address`. An opening `transcript` has `to`; live updates need no request ID.
A `reply` settles the request, not necessarily all work in a conversation.

The durable conversation inputs have separate request IDs. `chiefInput`
uses `chief:<say id>` to admit a chief's line once in that conversation.
`retryInput` uses `retry:<UUID>`; this UUID is independent of the wire
`retry` request's `id`. `reflectionInput` uses
`reflect:<conversation number>:<ordinal>`. Clients send UUIDs, not these
internal IDs. These constructors are in `server/src/chats/conversations.ts`.

`State.epoch` identifies the running state watcher, using the server clock
at its start. Revision starts at zero and increases when the realm state
changes. `Changes` carries the epoch and new revision. A full `state`
replaces the client's realm snapshot; `change` applies its listed updates.
These counters describe realm state, not transcript operations. Reconnect
with hello and reopen watches; `seen` does not request a delta replay.

## Client → server

These are all members of `ClientMessage`. Every row has `type` as shown.
Unless marked otherwise, add `id: RequestId` to the listed fields.
Expected request failures normally answer with `reply`; malformed messages
and watch failures can answer with `fault` (see [Errors](#errors)).

| Type (schema) | Fields | Action and answer |
|---|---|---|
| `hello` (`Hello`) | `v: 0`, `token: string`, `seen?: Seen`; no `id` | Authenticate; full `state`. |
| `say` (`Say`) | `to: Address`, `text: string` | Send a chief line or typed command. A command gets `reply`; an admitted agent line gets its receipt when that submission settles. Live work arrives through watches and realm changes. |
| `give` (`Give`) | `to: Address`, `files: {path: string, size: integer}[]` | Begin a file gift; `reply.upload` identifies the upload for chunks. Paths are relative; sizes are byte counts. |
| `give` (`Give`) | `to: Address`, `skill: {starter: string}` | Give an installed starter skill; `reply`. |
| `give` (`Give`) | `to: Address`, `skill: {from: Address, name: string}` | Copy another agent's named skill; `reply`. |
| `give-part` (`GivePart`) | `upload: string`, `path: string`, `offset: integer`, `data: string` | Send base64 bytes at a byte offset in a declared file. `reply` acknowledges the chunk or completed gift. |
| `answer` (`Answer`) | `wait: string`, `choice: Choice`, `text?: string` | Answer a wait; `reply`. |
| `stop` (`Stop`) | `agent?: Address` | Stop that agent's work, or all work if omitted; `reply`. |
| `rewind` (`Rewind`) | `chat: ChatRef`, `item: string` | Take back a chief input and what followed. `reply` with kind `rewound` returns its words; watchers get a reset and replacement transcript. Refused while work is running or queued. |
| `new` (`New`) | `to: Address` | Start a new numbered chief conversation; the previous queue can finish. `reply`. |
| `resume` (`Resume`) | `to: Address`, `number: integer` | Make that saved chief conversation current; `reply`. |
| `retry` (`Retry`) | `to: Address` | Ask the agent to carry on; `reply`. |
| `edit` (`Edit`) | `doc` and fields below | Edit a chief-owned document or model setting; `reply`. |
| `open` (`Open`) | `chat: ChatRef`, `before?: string` | Watch and fetch a page; `transcript` with `to`, or `fault`. |
| `close` (`Close`) | `chat: ChatRef`; no `id` | End that connection's watch; no answer. |
| `login` (`Login`) | One step below | Start, answer or cancel a connection-owned sign-in. |
| `check` (`Check`) | `what: "model"`, `model?: string` | Check that model, or the selected model; `reply`. |
| `check` (`Check`) | `what: "connection"` | Check the configured model connection; `reply`. Connection state also changes through `change`. |
| `read` (`Read`) | `what` and fields below | Fetch one document or list; `read` with `to`, or failure `reply`. |
| `move-houses` (`MoveHouses`) | none | Copy every agent's house from the workspace the server works in to the one chosen in Settings, replacing the houses there; `reply` once done. Progress also changes through `change`. |

`/new` and `/resume <number>` also exist as typed commands sent through
`say`. They perform the same conversation changes as the `new` and `resume`
messages. Both forms are part of the protocol.

### Edit documents

Each row is an `Edit` form with `type: "edit"` and `id`.

| `doc` | Fields | Meaning |
|---|---|---|
| `charter` | `text: string \| null` | Replace the charter; `null` restores the built-in charter. |
| `character` | `agent: Address`, `text: string` | Replace the agent's character. |
| `notes` | `agent: Address`, `text: string` | Replace the agent's notes. |
| `limits` | `hamlet: string`, `reason?: string`, `set: {section: string, key: string, value: unknown}` | Change one limit. The server checks the value. |
| `limits` | `hamlet: string`, `reason?: string`, `whole: object` | Replace the limits document, with defaults for omitted entries. |
| `settings` | `set: {model?: string, reflectEvery?: integer, allowNet?: boolean, endpoints?: Endpoint[], workspace?: {url: string, token: string} \| null}` | Change only the supplied settings. `workspace` chooses a remote workspace, or the local one for `null`; the server keeps it with its credentials and works in it from its next start. |
| `catalogue` | `model: string`, `set: {allowed?: boolean, price?: Price \| null, note?: string}` | Change a catalogue entry; `price: null` clears its price override. |

### Read documents

Each row is a `Read` form with `type: "read"` and `id`.

| `what` | Fields | `ReadResult` payload |
|---|---|---|
| `prompt` | `agent: Address` | `prompt: string`: the composed prompt. |
| `history` | `agent: Address` | `history: Past[]`: numbered chief conversations, with current flag, title, author, times, message and token counts. |
| `models` | `refresh?: boolean` | `models: Listed[]`: name, price, price source, context size, allowed flag and note. `refresh` refreshes discovery. |
| `papers` | `agent: Address`, `full?: boolean` | `papers: Papers`: notes, character and skill names/descriptions. By default, notes and character are trimmed and cut to 6000 Unicode code points each; `full: true` returns them uncut. |
| `charter` | None | `charter: {text: string, own: boolean}`: text and whether the chief supplied it. |

### Login steps

Each row is a `Login` form with `type: "login"` and `id`.

| Fields | Action and answer |
|---|---|
| `start: {provider: string, method: "key" \| "subscription"}` | Start sign-in. Server `login` messages carry the session ID and progress. |
| `reply: {login: string, value: string}` | Supply a key or answer for that session; `reply`. More server `login` steps can follow. |
| `cancel: string` | Cancel that session ID; `reply`. |

Sessions belong to the initiating connection. Disconnecting cancels them.

## Server → client

These are all members of `ServerMessage`. Each row has `type` as shown.

| Type (schema) | Fields | Meaning | Who receives it |
|---|---|---|---|
| `state` (`State`) | `epoch`, `revision`, `chief: string`, `sandbox: string`, `hamlets: Hamlet[]`, `agents: Agent[]`, `waits: Wait[]`, `grants: Grant[]`, `settings: SettingsView`, `providers: ProviderView[]`; optional `houses: string`, `library: LibrarySkill[]`, `closed: ClosedHamlet[]`, `deeds: string[]`, `starters: {name: string, description: string}[]` | Full realm snapshot. | Every client that said hello; a new client gets its own initial snapshot. |
| `change` (`Changes`) | `epoch`, `revision`, `changes: Update[]` | Realm updates since the previous revision. | Every client that said hello. |
| `transcript` (`Transcript`) | `to?: RequestId`, `chat: ChatRef`, `number?: integer`, `items: Item[]`, `earlier: string \| null`, `last?: {what: string, at: integer} \| null`, `working: boolean`, `pending: integer`, `usage: UsageView` | A page, or replacement page after a reset. `number` is the chief conversation's stable number. | Only clients watching that chat; a requested page goes to its requester. |
| `turn` (`Turn`) | `chat: ChatRef`, `number?: integer`, `ops: Op[]` | Live transcript operations, in order. | Only clients watching that chat. |
| `reply` (`Reply`) | `to: RequestId`, `kind: "notice" \| "error" \| "rewound"`, `text: string`, `upload?: string` | Request receipt, refusal or words recovered by rewind. | Only the connection that asked. |
| `read` (`ReadResult`) | `to: RequestId` and one of `prompt`, `history`, `models`, `papers`, `charter` as above | Requested document or list. | Only the connection that asked. |
| `login` (`LoginProgress`) | `login: string`, `to?: RequestId`, `step: LoginStep` | Sign-in session progress. | Only the connection that started the session. |
| `fault` (`Fault`) | `text: string`, `to?: RequestId` | Protocol, watch or connection failure. | Only the affected connection. |

`Update` replaces the named view or removes it. Its `type` is `hamlet`
(`hamlet`), `agent` (`agent`), `agent-gone` (`ref`), `wait` (`wait`),
`wait-closed` (`id`, `how`), `grants` (`grants`), `settings` (`settings`) or
`providers` (`providers`). Library and closed-hamlet list changes, and hamlet
removal, send a whole `state` instead.

`Agent` is a summary: identity, name, purpose, model, mayor flag, busy and
fault flags, activity, short saying and spend. Optional fields include
skills, departure, consultation and peer conversation summaries. It contains
no transcript. `Hamlet`, `Wait`, `Grant`, `SettingsView` and `ProviderView`
are the complete views defined in `protocol.ts`. `SettingsView`'s optional
`workspace` is `{url, running, move?}`: the remote workspace chosen and the
one the server works in until it restarts, each `null` for the local one. It
never holds the token. `move` is the latest move of the houses to the chosen
workspace since the server started: `state` (`moving`, `moved` or `failed`),
houses `done` of `of`, and its outcome as `text`.

`Item` has `id: string`, `at: integer`, `kind`, `text: string`, and optional
`chief`, `author`, `rewindable` and `tool`. Its kind is `user`, `assistant`,
`reasoning`, `tool`, `notice` or `error`. Peer inputs use `author`, not
`chief`. Item IDs identify an entry and content block (`<entryId>:<block>`);
use them as opaque cursors. Tool details are `name`, `summary`, nullable
`exit`, `ms`, `failure` and `change`. A tool argument summary holds at most
200 Unicode code points. A `Change` preview holds at most 200 lines, each
at most 240 characters, plus total and added/removed counts.

`Op` is discriminated by `op`, not `type`. `applyOp` is the shared reducer.

| `op` | Fields | Meaning |
|---|---|---|
| `add` | `item: Item`, `replaces?: string` | Add an item, replacing the named provisional item if supplied. |
| `append` | `item: string`, `text: string` | Append text to that item. |
| `close` | `item: string`, `exit: integer`, `ms: integer \| null`, `failure: string \| null`, `change?: Change \| null` | Close a tool item. |
| `status` | `working: boolean`, `pending: integer`, `last: {what: string, at: integer} \| null` | Replace work and queue status. |
| `usage` | `usage: UsageView` | Replace usage totals. |
| `reset` | None | Clear the transcript; a new `transcript` follows. |

`UsageView` carries `calls`, `unreported`, `input`, `cached`, `output`,
`reasoning`, `ms`, `micros`, `lastInput` and a display `summary`.
`LoginStep` is one of `{url, instructions?}`, `{code}`,
`{prompt, secret: boolean}`, `{progress}`, `{done}` or `{failed}`; all
unnamed value types here are strings. `LoginProgress.to` can correlate
steps with the starting request; a later client login reply has its own ID.

## Conversations

State carries agents' summaries, not transcripts. `open {chat}` watches
one conversation and returns the newest page first. A connection can keep
several watches. It receives live `turn` operations only for those chats.
`close {chat}` ends that connection's watch; disconnect ends all its watches.

A page holds at most **40 items**, not 40 turns or durable entries.
`transcript.ts`'s `transcriptOf` takes the last 40 folded items before the
cursor, or before the end when `before` is omitted. Pages are fetched newest
first, but **items inside each page stay in entry and content-block order,
oldest to newest**, not reverse timestamp order. The backing read batches
500 durable entries at a time; that is not the wire page size.

`earlier` is the first returned item's ID if older items remain, else
`null`. Send `open {chat, before: earlier}` to fetch the preceding page,
excluding the cursor item. An unknown cursor is a fault. Paging an existing
watch leaves its live subscription running, even if the page request fails.

Peer conversations (`from` and `kind`) are read-only. `say`, `stop`,
`rewind`, `new`, `resume` and `retry` cannot edit a peer conversation.
`rewind` explicitly refuses a peer or past `ChatRef` through
`requireWritable`. The other five messages take an agent `Address`, not a
peer `ChatRef`, and act on the chief conversation or agent. Adding peer chat
fields to them is malformed. Opening a peer watch does not retarget writes.

Without a watch, conversation activity appears only through realm `change`
messages: activity, saying, spend and busy state. A full `state` also carries
these summaries. Fetch history with `read`, then open the numbered chat to
read its items. Past chief conversations are read-only until resumed.

## Errors

Errors come two ways: `reply` with kind `error`, and `fault`. Both carry
readable `text`; neither carries a machine error code.

| Envelope | Use |
|---|---|
| `reply`, kind `error`, with `to` | An ordinary requested operation is refused: an edit, read, answer, gift, check or conversation action. Some typed command refusals are rendered as kind `notice` instead (`replyFor`). |
| `fault` with `to` | A malformed request with a recoverable UUID, an open/page refusal, a watch failure still associated with its opening request, or an unexpected request-handler failure. |
| `fault` without `to` | A handshake or connection failure, input with no valid request ID, or a watch failure after its opening request has been answered. |

A fault does not itself imply disconnect. Malformed input after hello,
second hello and request-handler failures leave the connection open.
Handshake refusals, message size violations and unexpected connection
failures close it as described above. `parseClient`, `replyFor`, `faultFor`
and `connection.ts`'s `serve` own these paths. Watch failures use `faultFor`,
even when their cause is an ordinary access refusal.

## Changing the protocol

1. Edit `server/src/protocol.ts`, the authority for both directions.
2. In `server/`, run `npm run schema` to export `protocol.schema.json`.
3. At the root, run `cargo run -p hamlet_protocol_gen` to generate
   `crates/hamlet_protocol/src/v0.rs`. Do not edit generated files by hand.
4. Re-record `server/test/frames/` when the wire sessions change. The offline
   command is in [`AGENTS.md`](../AGENTS.md).
5. Update this page. `server/test/protocol-doc.test.ts` checks that every
   client and server message type in the exported schema is named here.
