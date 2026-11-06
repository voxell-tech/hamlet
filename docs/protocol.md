# Client protocol

The window, `hamlet chat` and the CLI use one authenticated WebSocket to
Hamlet's server. There is no HTTP API. The wire carries JSON messages.

The contract is the Rust types in
[`hamlet_protocol::client`](../crates/hamlet_protocol/src/client.rs), one
module per area: `state`, `chat`, `waits`, `gifts`, `documents`, `limits`
and `settings`.
The names below, such as `Hello` and `ClientMessage`, are their type names.
The handshake's `v` is the protocol's version, `V`; it moves whenever the
schema does: a message's shape or a type's name.
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
{"type":"hello","v":2,"token":"<home token>"}
```

A successful hello receives a full `state` before live changes. The
server answers only a hello whose `v` is its own, so a client that
receives that `state` knows the server speaks its version. The window
says so in its connection words.

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
closes clients with **1001**. These rules are in
[`crates/hamlet_server/src/connection.rs`](../crates/hamlet_server/src/connection.rs).

## Shared shapes

| Schema | Shape and meaning |
|---|---|
| `Address` | `{hamlet: string, agent: string}`. An agent's name, with `agent: "mayor"` for the office's current holder. |
| `AgentRef` | `{hamlet: string, number: integer}`. Stable identity, independent of name or office. |
| `ChatRef` | `{with: Address, past?: integer, from?: AgentRef, kind?: "letters" \| "questions"}`. |
| `RequestId` | A UUID in its 36-character hyphenated form, in either case, such as `0f8fad5b-d9cb-469f-a165-70867728950e`. The braced, URN and 32-digit forms are refused. |
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

The durable conversation inputs have separate request IDs. A chief's line
uses `chief:<say id>` to admit it once in that conversation.
A retry uses `retry:<UUID>`; this UUID is independent of the wire
`retry` request's `id`. A reflection uses
`reflect:<conversation number>:<ordinal>`. Clients send UUIDs, not these
internal IDs. The server constructs them in `chats/say.rs`,
`chats/deeds.rs` and `chats/follow.rs`.

`State.epoch` identifies the running state watcher, using the server clock
at its start. Revision starts at zero and increases when the realm state
changes. `Changes` carries the epoch and new revision. A full `state`
replaces the client's realm snapshot; `change` applies its listed updates.
A client that keeps a `State` applies each with
`hamlet_protocol::client::State::apply`, as the CLI does.
These counters describe realm state, not transcript operations. Reconnect
with hello, which brings a full `state`, and reopen watches.

## Client → server

These are all members of `ClientMessage`. Every row has `type` as shown.
Unless marked otherwise, add `id: RequestId` to the listed fields.
Expected request failures normally answer with `reply`; malformed messages
and watch failures can answer with `fault` (see [Errors](#errors)).

| Type (schema) | Fields | Action and answer |
|---|---|---|
| `hello` (`Hello`) | `v: 2`, `token: string`; no `id` | Authenticate; full `state`. |
| `say` (`Say`) | `to: Address`, `text: string` | Send a chief line or typed command. A command gets `reply`; an admitted agent line gets its receipt when that submission settles. Live work arrives through watches and realm changes. |
| `give` (`Give`) | `to: Address`, `files: {path: string, size: integer}[]` | Begin a file gift; `reply.upload` identifies the upload for chunks. Paths are relative; sizes are byte counts. |
| `give` (`Give`) | `to: Address`, `skill: {starter: string}` | Give an installed starter skill; `reply`. |
| `give` (`Give`) | `to: Address`, `skill: {from: Address, name: string}` | Copy another agent's named skill; `reply`. |
| `give-part` (`GivePart`) | `upload: string`, `path: string`, `offset: integer`, `data: string` | Send base64 bytes at a byte offset in a declared file. `reply` acknowledges the chunk or completed gift. |
| `answer` (`Answer`) | `wait: string`, `choice: Choice`, `text?: string`, `picks?: Pick[]` | Answer a wait; `reply`. |
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

### Questions and answers

A question wait's `detail` carries `questions: Questions`: a `message` and a
list of one to ten questions. Each question has `question`, `description`,
up to six `options` (`title` and `description`), and `text_input`. A
question with no options always accepts custom words. The questions are kept
across server restarts and appear in both full state and wait updates.

Send `choice: "reply"` and `picks` in question order. Each pick is either
`{kind: "offer", index: 0}` (zero-based option index) or
`{kind: "words", text: "custom answer"}`. Every question needs one valid
answer. Blank words, unavailable options and custom words when
`text_input` is false are refused without closing the wait. Picks cannot
accompany `text` or any choice other than `reply`. A plain `text` reply
answers a single question that accepts words. `dismiss` needs neither.

The committed result of `ask_chief` produces a transcript item with kind
`answered`, containing the chief's reply. Structured replies quote each
question above its answer. Live operations and reopened history use the same item
ID and text; a dismissed question produces no answered item. Known
credentials are removed before questions or answers are stored or sent.

### Edit documents

Each row is an `Edit` form with `type: "edit"` and `id`.

| `doc` | Fields | Meaning |
|---|---|---|
| `charter` | `text: string \| null` | Replace the charter; `null` restores the built-in charter. |
| `character` | `agent: Address`, `text: string` | Replace the agent's character. |
| `notes` | `agent: Address`, `text: string` | Replace the agent's notes. |
| `limits` | `hamlet: string`, `reason?: string`, `set: {section: string, key: string, value: unknown}` | Change one limit. The server checks the value. |
| `limits` | `hamlet: string`, `reason?: string`, `whole: object` | Replace the limits document, with defaults for omitted entries. |
| `settings` | `set: {model?: string, reflectEvery?: integer, allowNet?: boolean, effort?: "off" \| "minimal" \| "low" \| "medium" \| "high" \| null, endpoints?: Endpoint[], workspace?: {url: string, token: string} \| null}` | Change only the supplied settings. `effort` is how hard models think by default, the model's own for `null`; the `efforts` limit sets it for one agent. `workspace` chooses a remote workspace, or the local one for `null`; the server keeps it with its credentials and works in it from its next start. |
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
| `state` (`State`) | `epoch`, `revision`, `chief: string`, `confinement: Confinement`, `hamlets: Hamlet[]`, `agents: Agent[]`, `waits: Wait[]`, `grants: Grant[]`, `settings: SettingsView`, `providers: ProviderView[]`, `houses: string`, `library: LibrarySkill[]`, `closed: ClosedHamlet[]`, `deeds: {name: string, usage: string, description: string}[]`, `starters: {name: string, description: string}[]` | Full realm snapshot. | Every client that said hello; a new client gets its own initial snapshot. |
| `change` (`Changes`) | `epoch`, `revision`, `changes: Update[]` | Realm updates since the previous revision. | Every client that said hello. |
| `transcript` (`Transcript`) | `to?: RequestId`, `chat: ChatRef`, `number?: integer`, `items: Item[]`, `earlier: string \| null`, `last: {what: string, at: integer} \| null`, `working: boolean`, `pending: integer`, `usage: UsageView` | A page, or replacement page after a reset. `number` is the chief conversation's stable number. | Only clients watching that chat; a requested page goes to its requester. |
| `turn` (`Turn`) | `chat: ChatRef`, `number?: integer`, `ops: TranscriptOperation[]` | Live transcript operations, in order. | Only clients watching that chat. |
| `reply` (`Reply`) | `to: RequestId`, `kind: "notice" \| "error" \| "rewound"`, `text: string`, `upload?: string` | Request receipt, refusal or words recovered by rewind. | Only the connection that asked. |
| `read` (`ReadResult`) | `to: RequestId` and one of `prompt`, `history`, `models`, `papers`, `charter` as above | Requested document or list. | Only the connection that asked. |
| `login` (`LoginProgress`) | `login: string`, `to?: RequestId`, `step: LoginStep` | Sign-in session progress. | Only the connection that started the session. |
| `fault` (`Fault`) | `text: string`, `to?: RequestId` | Protocol, watch or connection failure. | Only the affected connection. |

`Update` replaces the named view or removes it. Its `type` is `hamlet`
(`hamlet`), `agent` (`agent`), `agent-gone` (`ref`), `wait` (`wait`),
`wait-closed` (`id`, `how`), `grants` (`grants`), `settings` (`settings`) or
`providers` (`providers`). Library and closed-hamlet list changes, and hamlet
removal, send a whole `state` instead.

`Confinement` says what keeps the agents' commands from the rest of
the machine. It is discriminated by `kind`: `sandbox` with the sandbox's
`name`, `insecure` when they run without one, with the server account's
full reach and no enforced network switch, or `remote` when the workspace
is another machine.

`Agent` is a summary: identity, name, purpose, model, mayor flag, busy and
fault flags, activity, short saying, spend, skills, consultation and peer
conversation summaries, and `left?` once it has left. It contains no
transcript. `Hamlet`, `Wait`, `Grant`, `SettingsView` and `ProviderView`
are the complete views defined in `hamlet_protocol`. `SettingsView.workspace`
is `{url, running, move?}`: the remote workspace chosen and the
one the server works in until it restarts, each `null` for the local one. It
never holds the token. `move` is the latest move of the houses to the chosen
workspace since the server started: `state` (`moving`, `moved` or `failed`),
houses `done` of `of`, and its outcome as `text`.

`Item` has `id: string`, `at: integer`, `kind`, `text: string`, and optional
`chief`, `author`, `rewindable` and `tool`. Its kind is `user`, `assistant`,
`reasoning`, `tool`, `notice`, `error` or `answered`, the chief's answers
to questions. Peer inputs use `author`, not `chief`. Item IDs identify an
entry and content block (`<entryId>:<block>`); use them as opaque cursors. Tool details are `name`, `summary`, nullable
`exit`, `ms`, `failure` and `change`. A tool argument summary holds at most
200 Unicode code points. A `Change` preview holds at most 200 lines, each
at most 240 characters, plus total and added/removed counts.

`TranscriptOperation` is discriminated by `op`, not `type`. Clients apply
operations with `hamlet_protocol::client::transcript::apply`.

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
`chats/watch.rs` takes the last 40 folded items before the
cursor, or before the end when `before` is omitted. Pages are fetched newest
first, but **items inside each page stay in entry and content-block order,
oldest to newest**, not reverse timestamp order. Harness history batches
are separate from the wire page size.

`earlier` is the first returned item's ID if older items remain, else
`null`. Send `open {chat, before: earlier}` to fetch the preceding page,
excluding the cursor item. An unknown cursor is a fault. Paging an existing
watch leaves its live subscription running, even if the page request fails.

Peer conversations (`from` and `kind`) are read-only. `say`, `stop`,
`rewind`, `new`, `resume` and `retry` cannot edit a peer conversation.
`rewind` explicitly refuses a peer or past `ChatRef` through
`chats/deeds.rs`. The other five messages take an agent `Address`, not a
peer `ChatRef`, and act on the chief conversation or agent. Adding peer chat
fields to them is malformed. Opening a peer watch does not retarget writes.

Without a watch, conversation activity appears only through realm `change`
messages: activity, saying, spend and busy state. A full `state` also carries
these summaries. Fetch history with `read`, then open the numbered chat to
read its items. Past chief conversations are read-only until resumed.

## Ordering and notices

An empty notice `reply` to a `say` acknowledges that its submission
settled, not that all work in the conversation ended. A `rewound` reply
gives the removed chief words back to the input field. A request's reply is
not ordered against a chat's `turn` and `transcript` messages. A reset and
its replacement transcript are sent together, with no message between them.
Reflections and retries show as notices, as does a compaction.

A `Wait` has the fields every wait has (`id`, `by`, `agent`, `about`,
`subject`, `change`, `answers`, `since`, `asked`, `chat`) and a `detail`
discriminated by `kind`: `approval` with its `danger`, the agent's wish as
`says` and how long `always` lasts (`session`, `grant`, or `null` when it is
not offered); `fault`; `question` with its `questions`; or
`objection`. `subject` is the line of `about` a one-line view quotes. A
refused `edit` changes nothing and replies with an error. `check` replies in words and changes the connection status in
settings; a model check waits at most 90 s.

## The society in state

Every state and change carries the society and realm in full:

- `Agent.consulting` and `conversations` describe library work and incoming
  peer conversations. `Agent.left` describes a departure, and only an agent
  that left has one.
- `Hamlet.roads` and `votes` carry resolved road ends and traffic, and
  ready-made vote reports. `Hamlet.at` is the town's position, or `null`
  for a town never set down, which stands on the first free site.
- `State.library` and `closed` carry the shared shelf and removed hamlets.
- `Limits.votes` is per hamlet. `Limits.hamlets` is governed by `home`.
- `State.houses` names the houses folder on the server's machine so local
  links and pictures can exclude it, including through symlinks.

Archived agents remain in state with `left` until erased. Office road ends
are resolved to the current mayor. Peer turns contribute to activity and
spend even when no chief watches their conversation.

`hamlet_client` reconciles state and changes into entities, keeping the
entities of hamlets and agents still present. When reconnecting it sends
`hello` again, retains unsent requests in its
Outbox and opens its watched chats again. A lost connection retries; a
refused token stops retries and shows the server's words.

## Errors

Errors come two ways: `reply` with kind `error`, and `fault`. Both carry
readable `text`; neither carries a machine error code.

| Envelope | Use |
|---|---|
| `reply`, kind `error`, with `to` | An ordinary requested operation is refused: an edit, read, answer, gift, check or conversation action. Some typed command refusals are rendered as kind `notice` instead (`chats/say.rs`). |
| `fault` with `to` | A malformed request with a recoverable UUID, an open/page refusal, a watch failure still associated with its opening request, or an unexpected request-handler failure, a panic included. |
| `fault` without `to` | A handshake or connection failure, input with no valid request ID, or a watch failure after its opening request has been answered. |

A fault does not itself imply disconnect. Malformed input after hello,
second hello and request-handler failures leave the connection open.
Handshake refusals, message size violations and unexpected connection
failures close it as described above. The server's `protocol.rs`,
`connection.rs` and `requests.rs` own these paths. Watch failures use
the `fault` envelope, even when their cause is an ordinary access
refusal.

## Changing the protocol

1. Edit the types in `crates/hamlet_protocol/src/client/`, the authority for both
   directions. The crate has no server-domain dependencies. A rule a
   type can hold goes in the type, as `RequestId` holds its UUID form,
   so Rust cannot build a message the schema refuses. A limit serde
   cannot check, such as a list's length, goes in a `#[schemars(...)]`
   attribute: the server checks every client message against the JSON
   Schema derived from these types.
2. Move `V` in `crates/hamlet_protocol/src/client.rs` when a message's
   shape or a type's name changes.
   `cargo test -p hamlet_protocol` keeps a sha256 of the client schema
   with `V` in `crates/hamlet_protocol/client_schema.fingerprint`. It
   fails while the shapes differ from the recorded ones and `V` has not
   moved. Once `V` moves, it rewrites the file and fails once: commit
   the file. Doc comments are not part of the fingerprint.
3. Re-record `crates/hamlet_protocol/frames/` when the wire sessions
   change. The offline Rust recorder's command is in the
   [acceptance README](../crates/hamlet_acceptance/README.md#recording-protocol-frames).
   Ordinary tests only read these fixtures.
4. Update this page. `cargo test -p hamlet_protocol` checks that every
   client and server message type is named here.
