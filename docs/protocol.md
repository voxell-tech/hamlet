# Client protocol

The window, `hamlet chat` and the CLI use one authenticated WebSocket to
Hamlet's server. There is no HTTP API. The wire carries JSON messages.

The contract is the Rust types in
[`hamlet_protocol::client`](../crates/hamlet_protocol/src/client.rs), one
module per area: `state`, `chat`, `waits`, `images`, `files`, `documents`,
`limits` and `settings`.
The names below, such as `Hello` and `ClientMessage`, are their type names.
The handshake's `v` is the protocol's version, `VERSION`, in semver. A
server takes a client whose version is compatible with its own by
semver's caret. It stays at 0.1.0 until Hamlet's first release.
`?` means optional; `null` is an explicit value. Objects reject extra fields.

## Transport and handshake

The server listens on `127.0.0.1` only, over plain `ws://`. Local clients
read the URL from `server.json` in the realm's folder and
`connection.token` from `credentials.json` there. The folder is
`HAMLET_HOME`, else `~/.hamlet`.

For a server on another machine, forward its loopback port through SSH.
Set `HAMLET_SERVER` to the tunnel's local `ws://127.0.0.1:<port>` URL and
`HAMLET_TOKEN` to that server's connection token. `HAMLET_SERVER` requires
`HAMLET_TOKEN`; it bypasses local discovery. The server itself still binds
to loopback. Keep the token private; plain WebSocket supplies no encryption.

Send `Hello` first, within 10 seconds:

```json
{"type":"hello","v":"0.1.0","token":"<connection token>"}
```

A successful hello receives a full `state` before live changes. The
server answers only a hello whose `v` is compatible with its own, so a
client that receives that `state` knows the server speaks its version.
The window says so in its connection words.

| Failure | Answer and connection |
|---|---|
| No hello in time | `fault`: `No hello came in time.`; close **1008** |
| Another message first, or a malformed one | `fault`: `The first message is a hello.`; close **1008** |
| Wrong token | `fault`: `That token is not this server's.`; close **4001** |
| A `v` not compatible with the server's | `fault` naming both versions; close **4002** |
| A second hello | `fault`: `This connection said hello already.`; stays open |
| Unexpected connection failure | `fault`; close **1011** |

A client-to-server WebSocket message holds at most **1,048,576 bytes**
(1 MiB). An oversized message gets a best-effort `fault` and close **1009**.
The server applies no matching outbound limit to state or transcript pages.
Images and files travel in chunks (`PutImage`, `PutImagePart`, `PutFile`,
`PutFilePart`). A server shutdown closes clients with **1001**. These rules
are in
[`crates/hamlet_server/src/clients/connection.rs`](../crates/hamlet_server/src/clients/connection.rs).

## Shared shapes

| Schema | Shape and meaning |
|---|---|
| `AgentId` | An integer naming one agent of the realm (the Hand, a hire or a worker), stable for as long as the agent exists. The server resolves typed addresses (`hand`, `mayor@<hamlet>`, `hire#<id>`, `worker#<id>`) in chief commands; messages carry ids. |
| `ScholarId` | An integer naming one scholar of the city. Commands address a scholar as `scholar#<id>`; its name is a label that may change and repeat. |
| `Keeper` | `{scholar: ScholarId}` or `{agent: AgentId}`: whose knowledge a document is of, a scholar's `main` or an agent's lodge. |
| `ChatRef` | `{with: AgentId, past?: integer, from?: AgentId, kind?: "letters" \| "questions"}`. |
| `RequestId` | A UUID in its 36-character hyphenated form, in either case, such as `0f8fad5b-d9cb-469f-a165-70867728950e`. The braced, URN and 32-digit forms are refused. |
| `ImageId` | The SHA-256 of an image's bytes as 64 lowercase hexadecimal digits. The same bytes have the same id however often they are sent. |
| `FileId` | The SHA-256 of a file's bytes as 64 lowercase hexadecimal digits. |
| `AttachedImage` | `{id: ImageId, at?: integer}`: an image a line carries and where it sits in the line's text. |
| `AttachedFile` | `{id: FileId, name: string, size: integer, at?: integer}`: a file a line carries and the name the agent finds it under, its path in an attached folder with `/` between the parts (no empty part, no `.` or `..`), at most 1024 characters, and where it sits in the line's text. |

| `Choice` | `allow`, `always`, `deny`, `go`, `withdraw`, `reply`, `dismiss`, `welcome`. Use the choices advertised by the wait. |
| `Endpoint` | `{name: string, baseUrl: string}`. |
| `Price` | `{input: number, output: number}`: dollars per million tokens. |
| `Amount` | `{tokens: integer, micros: integer, ms: integer}`: tokens, millionths of a dollar, milliseconds. |

`at` is a byte offset into the line's `text`, on a character boundary and at
most the text's length in bytes; a line with any other `at` is refused. An
attachment without `at` goes after the text. The files of one folder share
the folder's `at`.

`ChatRef.with` selects the agent the chief talks to. Without `past`, it
selects the current chief conversation; `past` selects a stable conversation
number. `from` selects a peer conversation with that stable agent;
`kind` selects its letters or questions lane, defaulting to letters.
`from` and `past` together are refused.

Every client message except `hello` and `close` carries `id: RequestId`.
The answer's `to` matches it. Do not confuse that with `Say.to`, which is an
`AgentId`. An opening `transcript` has `to`; live updates need no request ID.
A `reply` settles the request, not necessarily all work in a conversation.

The durable conversation inputs have separate request IDs. A chief's line
uses `chief:<say id>` to admit it once in that conversation.
A retry uses `retry:<UUID>`; this UUID is independent of the wire
`retry` request's `id`. A reflection uses
`reflection:<conversation>:<number>`. Clients send UUIDs, not these
internal IDs. The server constructs them in `talk/chats.rs` and
`talk/reflections.rs`.

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
| `hello` (`Hello`) | `v: Version`, `token: string`; no `id` | Authenticate; full `state`. |
| `say` (`Say`) | `to: AgentId`, `text: string`, `images?: AttachedImage[]`, `files?: AttachedFile[]` | Send a chief line or typed command. A command gets `reply`; an admitted agent line gets its receipt when that submission settles. Live work arrives through watches and realm changes. `images` are images sent before with `put-image`, at most 20 and at most what the agent's model takes; a line with images is refused when that model takes none. `files` are files sent before with `put-file`, of any kind, each named for where the agent finds it; the server copies them into the agent's lodge under `inbox/` and the line's caption says where each was saved. They are not given to the model as content. Each caption is put in the text at its attachment's `at`, those at one `at` in the order sent, images first, and the model is given the images in the order of their captions; the captions of attachments without `at` follow the text. `text` in the transcript is the words alone, and the transcript item carries each `at`. At most 10,000 files, 256 MiB each and 1 GiB together; the client asks its chief before sending more than 32 MiB or 1000 files. A command takes neither. |
| `put-image` (`PutImage`) | `size: integer` | Begin sending one image of that many bytes, at most 20 MiB, a PNG, JPEG, WebP or GIF; `reply.upload` identifies the upload for its parts. |
| `put-image-part` (`PutImagePart`) | `upload: string`, `offset: integer`, `data: string` | Send base64 bytes at a byte offset. `reply` acknowledges the part; the one that completes the image is answered with `reply.image`, the image's id. |
| `put-file` (`PutFile`) | `size: integer` | Begin sending one file of that many bytes, at most 256 MiB; `reply.upload` identifies the upload for its parts, or an empty file is answered at once with `reply.file`. |
| `put-file-part` (`PutFilePart`) | `upload: string`, `offset: integer`, `data: string` | Send base64 bytes at a byte offset. `reply` acknowledges the part; the one that completes the file is answered with `reply.file`, the file's id. |
| `answer` (`Answer`) | `wait: string`, `choice: Choice`, `text?: string`, `picks?: Pick[]` | Answer a wait; `reply`. |
| `stop` (`Stop`) | `agent?: AgentId` | Stop that agent's work, or all work if omitted; `reply`. |
| `rewind` (`Rewind`) | `chat: ChatRef`, `item: string` | Take back a chief input and what followed. `reply` with kind `rewound` returns its words; watchers get a reset and replacement transcript. Refused while work is running or queued. |
| `new` (`New`) | `to: AgentId` | Start a new numbered chief conversation; the previous queue can finish. `reply`. |
| `resume` (`Resume`) | `to: AgentId`, `number: integer` | Make that saved chief conversation current; `reply`. |
| `retry` (`Retry`) | `to: AgentId` | Ask the agent to carry on; `reply`. |
| `edit` (`Edit`) | `doc` and fields below | Edit a chief-owned document or model setting; `reply`. |
| `open` (`Open`) | `chat: ChatRef`, `before?: string` | Watch and fetch a page; `transcript` with `to`, or `fault`. |
| `close` (`Close`) | `chat: ChatRef`; no `id` | End that connection's watch; no answer. |
| `login` (`Login`) | One step below | Start, answer or cancel a connection-owned sign-in. |
| `check` (`Check`) | `what: "model"`, `model?: string` | Check that model, or the selected model; `reply`. |
| `check` (`Check`) | `what: "connection"` | Check the configured model connection; `reply`. Connection state also changes through `change`. |
| `read` (`Read`) | `what` and fields below | Fetch one document or list; `read` with `to`, or failure `reply`. |
| `move-lodges` (`MoveLodges`) | none | Copy every hire's lodge from the workspace the server works in to the one chosen in Settings, replacing the lodges there; `reply` once done. Progress also changes through `change`. |

| `terminal` (`Terminal`) | `op` and fields below | Open, type into, resize, watch, rename or close a terminal. |

`Terminal` is discriminated by `op`:

| `op` | Fields | Answer |
|---|---|---|
| `open` | `id`, `hamlet?: string`, `cols`, `rows` | Opens a shell in the hamlet's folder under a sandbox that writes only that folder, or the chief's own login shell with no sandbox when `hamlet` is left out. The connection watches it: the first `terminal` message has `to` and names it. A refusal is a `reply`. |
| `input` | `terminal`, `data: string`; no `id` | Types `data`, keys and pastes as the terminal reads them, into the shell. No answer. A `commands` terminal or an ended shell ignores it. |
| `rename` | `id`, `terminal`, `name: string` (1–64) | Names it: its `name` changes and `named` becomes true, so the name shows over its `title`. `reply`; a blank name is refused. |
| `resize` | `terminal`, `cols`, `rows`; no `id` | No answer. |
| `watch` | `id`, `terminal` | A `terminal` message with `to` and `reset: true` that redraws it, then its output as it comes. A `reply` when there is no such terminal. |
| `unwatch` | `terminal`; no `id` | No answer. |
| `close` | `id`, `terminal` | Ends its processes and removes it; `reply`. |

Columns and rows are 1 to 1000; `data` holds at most 65536 bytes.

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
| `character` | `scholar: ScholarId`, `text: string` | Replace the scholar's character, as a commit on its `main`. |
| `memory` | `of: Keeper`, `path: string`, `text: string` | Replace a memory file of a scholar's `main` or a hire's lodge, by its path under `memory/`; empty text removes it. |
| `limits` | `hamlet?: string`, `reason?: string`, `set: {section: string, key: string, value: unknown}` | Change one limit. The server checks the value. |
| `limits` | `hamlet?: string`, `reason?: string`, `whole: object` | Replace the limits document, with defaults for omitted entries. |
| `settings` | `set: {model?: string, reflectEvery?: integer, allowNet?: boolean, effort?: "off" \| "minimal" \| "low" \| "medium" \| "high" \| "xhigh" \| "max" \| null, newcomers?: "open" \| "chief" \| "hand", character?: string, endpoints?: Endpoint[], workspace?: {url: string, token: string} \| null}` | Change only the supplied settings. `effort` is how hard models think by default, the model's own for `null`; an agent's own effort, set with `/effort`, overrides it. `newcomers` says who lets a scholar an agent made into the city: nobody needs to (`open`), a chief (`chief`, the default) or the Hand (`hand`). `character` is what a blank agent an agent hires starts from, at most 4000 characters. `workspace` chooses a remote workspace, or the local one for `null`; the server keeps it with its credentials and works in it from its next start. |
| `catalogue` | `model: string`, `set: {images?: ImageInput \| null, price?: Price \| null, note?: string}` | Change a catalogue entry; `price: null` clears its price override. `images` says the model takes images within those limits (`maxWidth?`, `maxHeight?`, `maxBytes?`, `perLine?`, none of them zero, the rest at their defaults); `images: null` clears it back to what the model's provider lists. |

### Read documents

Each row is a `Read` form with `type: "read"` and `id`.

| `what` | Fields | `ReadResult` payload |
|---|---|---|
| `prompt` | `agent: AgentId` | `prompt: [{name, text}]`: the composed prompt, section by section in the order the model reads them, each named for its source (`charter`, `machine`, `project`, `character`, `memory`, `skills`, `place`, `realm`, `scholars` or `errand`). |
| `history` | `agent: AgentId` | `history: Past[]`: numbered chief conversations, with current flag, title, author, times, message and token counts. |
| `models` | `refresh?: boolean` | `models: Listed[]`: name, `title` (what its provider calls it; empty when it gives no name), price, price source, context size, note, `efforts` (the efforts the model can be asked for, weakest first), `defaultEffort` (`null` while unknown), `fast` (whether its provider has a fast tier) and `images?` (what it takes of images: `maxWidth?`, `maxHeight?`, `maxBytes?` and `perLine?`; absent when it takes none). `refresh` refreshes discovery. |
| `image` | `image: ImageId`, `size?: "original" \| "thumbnail"` | `image: {mime: string, data: string}`: the image's bytes in base64, as they were sent for `original` (the default), or a small copy for `thumbnail`. A refusal replies when the realm keeps no such image. |
| `papers` | `of: Keeper`, `full?: boolean` | `papers: Papers`: character, memory files (`path`, `description`, `text`), the paths a hire's lodge `learned` against its scholar, and skill names/descriptions. By default, character and memory texts are trimmed and cut to 6000 Unicode code points each; `full: true` returns them uncut. |
| `charter` | None | `charter: {text: string, own: boolean}`: text and whether the chief supplied it. |

### Login steps

Each row is a `Login` form with `type: "login"` and `id`.

| Fields | Action and answer |
|---|---|
| `start: {provider: string, method: "key" \| "subscription"}` | Start sign-in. Server `login` messages carry the session ID and progress. |
| `reply: {login: string, value: string}` | Supply a key or answer for that session; `reply`. More server `login` steps can follow. |
| `cancel: string` | Cancel that session ID; `reply`. |

Sessions belong to the initiating connection. Disconnecting cancels them.

The provider `claude` signs in with `method: "subscription"` alone. pi runs
Claude's own sign-in and the server sends one `url` step, the link to
Anthropic's sign-in page, then `done` once the Claude program has the login;
no `reply` is asked for. The credential the server keeps only marks the
sign-in. The sign-in completes only in a browser on the server's machine.

## Server → client

These are all members of `ServerMessage`. Each row has `type` as shown.

| Type (schema) | Fields | Meaning | Who receives it |
|---|---|---|---|
| `state` (`State`) | `epoch`, `revision`, `chief: string`, `confinement: Confinement`, `hand: AgentId`, `hamlets: Hamlet[]`, `agents: Agent[]`, `scholars: Scholar[]`, `limits: RealmLimits`, `waits: Wait[]`, `grants: Grant[]`, `settings: SettingsView`, `providers: ProviderView[]`, `realm: string`, `library: LibrarySkill[]`, `closed: ClosedHamlet[]`, `deeds: {name: string, usage: string, description: string}[]`, `terminals: TerminalView[]` | Full realm snapshot. | Every client that said hello; a new client gets its own initial snapshot. |
| `change` (`Changes`) | `epoch`, `revision`, `changes: Update[]` | Realm updates since the previous revision. | Every client that said hello. |
| `transcript` (`Transcript`) | `to?: RequestId`, `chat: ChatRef`, `number?: integer`, `items: Item[]`, `earlier: string \| null`, `last: {what: string, at: integer} \| null`, `working: boolean`, `pending: integer`, `usage: UsageView` | A page, or replacement page after a reset. `number` is the chief conversation's stable number. | Only clients watching that chat; a requested page goes to its requester. |
| `turn` (`Turn`) | `chat: ChatRef`, `number?: integer`, `ops: TranscriptOperation[]` | Live transcript operations, in order. | Only clients watching that chat. |
| `reply` (`Reply`) | `to: RequestId`, `kind: "notice" \| "error" \| "rewound"`, `text: string`, `upload?: string`, `image?: ImageId`, `images?: AttachedImage[]`, `file?: FileId`, `files?: AttachedFile[]` | Request receipt, refusal or words recovered by rewind, with the images and files of the line taken back (`images`, `files`), each with its `at` in the words returned. | Only the connection that asked. |
| `read` (`ReadResult`) | `to: RequestId` and one of `prompt`, `history`, `models`, `image`, `papers`, `charter` as above | Requested document or list. | Only the connection that asked. |
| `login` (`LoginProgress`) | `login: string`, `to?: RequestId`, `step: LoginStep` | Sign-in session progress. | Only the connection that started the session. |
| `fault` (`Fault`) | `text: string`, `to?: RequestId` | Protocol, watch or connection failure. | Only the affected connection. |
| `terminal` (`TerminalOutput`) | `terminal: string`, `data: string`, `reset: boolean`, `to?: RequestId` | Bytes a terminal printed, in base64. With `reset`, they redraw it from a blank screen of its size: the answer to a watch, or a catch-up for a connection whose queue filled and that skipped output. A redraw starts with the size its first byte was printed at and marks each resize after, as `ESC ] hamlet;size;<cols>;<rows> BEL`, so each part is drawn at the size it was printed at (`hamlet_protocol::client::replayed` splits them). The server keeps the last 512 KB of what a terminal printed, so a redraw ends with what its trim may have cut away: on the alternate screen, that screen as it shows, then the input modes its program set, such as reading the mouse and bracketed paste. Both ends resize a screen with `hamlet_protocol::screen::resize`, which wraps what was printed again at the new width, as a terminal window does. | Only connections watching it. |

`Update` replaces the named view or removes it. Its `type` is `hamlet`
(`hamlet`), `agent` (`agent`), `agent-gone` (`id`), `scholar` (`scholar`), `scholar-gone` (`id`), `wait` (`wait`),
`wait-closed` (`id`, `how`), `grants` (`grants`), `settings` (`settings`),
`providers` (`providers`), `terminal` (`terminal`) or `terminal-gone` (`id`). Library and closed-hamlet list changes, and hamlet
removal, send a whole `state` instead.

`Confinement` says what keeps the agents' commands from the rest of
the machine. It is discriminated by `kind`: `sandbox` with the sandbox's
`name`, `insecure` when they run without one, with the server account's
full reach and no enforced network switch, or `remote` when the workspace
is another machine.

`Agent` is a summary: `id`, `kind` (`hand`, `hire` or `worker`), `hamlet`, `scholar`, `parent`, a hire's `pin` (the version of its scholar it is held at, `null` while it follows `main`), the address, which is its `name` (`hand`, `hire#<id>`, `worker#<id>`), a hire's or a worker's `nickname` (`null` while a hire goes by its scholar's name and a worker by its address), `purpose`, `model` (the one it runs now), `ownModel` (the one chosen for it, `null` while it follows its hamlet's or the realm's), `effort` (its own, `null` while it follows the realm's), `fast` (whether it asks for the provider's fast tier), `mayor`, `busy` and
`faulted` flags, `activity`, `saying`, `spent`, `since`, `skills`, a hire's `allowance` (dollars a budget period, left out while it has none), a hire's `toolsOff` (the built-in tools turned off for it, left out while none are), `consulting` and
`conversations` (the peer conversations it holds), and `left?` once it has left. It contains no
transcript. `Hamlet`, `Wait`, `Grant`, `SettingsView` and `ProviderView`
are the complete views defined in `hamlet_protocol`. `SettingsView.workspace`
is `{url, running, move?}`: the remote workspace chosen and the
one the server works in until it restarts, each `null` for the local one. It
never holds the token. `move` is the latest move of the lodges to the chosen
workspace since the server started: `state` (`moving`, `moved` or `failed`),
lodges `done` of `of`, and its outcome as `text`.

`Item` has `id: string`, `at: integer`, `kind`, `text: string`, and optional
`chief`, `author`, `rewindable`, `tool`, `images` (the `AttachedImage`s a chief's
line carried) and `files` (its `AttachedFile`s), each with its `at` in `text`; the transcript holds ids and never bytes. Its kind is `user`, `assistant`,
`reasoning`, `tool`, `notice`, `error` or `answered`, the chief's answers
to questions. Peer inputs use `author`, not `chief`. Item IDs identify an
entry and content block (`<entryId>:<block>`); use them as opaque cursors. Tool details are `name`, `summary`, nullable
`exit`, `ms`, `failure` and `change`. A tool argument summary holds at most
200 Unicode code points. A `Change` preview holds at most 200 lines, each
at most 240 characters, plus total and added/removed counts.

`TerminalView` is a terminal the server runs: `id`, `name`, `kind`,
`hamlet` (null for one of no hamlet: the chief's own
unbounded shell, or the Hand's or a hamlet-less worker's), `agent` (the `AgentId`
that opened it, null when the chief did), `cols`, `rows`, `cwd`, `exit`
(null while its shell runs), `sandboxed`, the `title` its programs set,
`named` (whether the chief or its agent named it, its name then shown over
the title) and `last`, the last command typed into it or, for `commands`, the
last its agent ran (null before one). A typed command is the line entered
after a prompt, read once the shell echoes its Enter and kept only while
that line still shows the prompt typing began after; what a full-screen
program is typed is not one, and nor is what a prompt does not echo.
`kind` is `shell`, which takes input, or `commands`: one per agent, showing
each command its shell tool runs and that command's output as it comes,
read-only. A terminal's agent or hamlet leaving removes it. The server
keeps each terminal's last output across restarts; a shell that ran
starts again below it.

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

A page holds at most **100 items**, not 100 turns or durable entries.
`clients/requests.rs` takes the last 100 folded items before the
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
`rewind` explicitly refuses a peer or past `ChatRef` in
`clients/requests.rs`. The other five messages take an `AgentId`, not a
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
discriminated by `kind`: `approval` with its `danger` (`network`, `unconfined`, `destructive`, `reserved`, `edit`, `command` or `send`, files an agent sends over what one send holds), the agent's wish as
`says` and how long `always` lasts (`session`, `grant`, or `null` when it is
not offered); `fault`; `question` with its `questions`; `objection`; or
`newcomer` with the `scholar` waiting at the harbor, whose answers are
`welcome` and `dismiss`, which leaves it waiting. `subject` is the line of
`about` a one-line view quotes. A
refused `edit` changes nothing and replies with an error. `check` replies in words and changes the connection status in
settings; a model check waits at most 90 s.

## The society in state

Every state and change carries the society and realm in full:

- `Agent.consulting` and `conversations` describe library work and incoming
  peer conversations. `Agent.left` describes a departure, and only an agent
  that left has one.
- `Hamlet.roads` carry road ends by `AgentId`, their kind and traffic.
  `Hamlet.mayor` is the hire at the root of its tree. `Hamlet.at` is the hamlet's position, or `null`
  for a hamlet never set down, which stands on the first free site.
- `Scholar.arrival` is `housed`, `waiting` at the harbor, `turned`
  away, or `apart`: a blank hire's own, which is not of the city and has
  no house. `Scholar.arrivalBy` names who last changed it, a chief or
  `hand`, and is `null` while it is as it was made. `Scholar.house` is a housed scholar's spot from the city's
  middle, `{x, z, turn}`, its door facing `turn` radians about the
  vertical, and `null` for one at the harbor. The server chooses the spot
  of a scholar let in; `/move-house` sets it elsewhere.
- `State.library` and `closed` carry the shared shelf and removed hamlets.
- `State.limits` are the realm's own limits; `Limits` are a hamlet's.
- `State.realm` names the realm's folder on the server's machine so local
  links and pictures can exclude it, including through symlinks.

Archived agents remain in state with `left` until erased. Peer turns contribute to activity and
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
| `reply`, kind `error`, with `to` | An ordinary requested operation is refused: an edit, read, answer, check or conversation action, or a typed command. |
| `fault` with `to` | A malformed request with a recoverable UUID, an open/page refusal, a watch failure still associated with its opening request, or an unexpected request-handler failure, a panic included. |
| `fault` without `to` | A handshake or connection failure, input with no valid request ID, or a watch failure after its opening request has been answered. |

A fault does not itself imply disconnect. Malformed input after hello,
second hello and request-handler failures leave the connection open.
Handshake refusals, message size violations and unexpected connection
failures close it as described above. The server's
`clients/connection.rs` and `clients/requests.rs` own these paths. Watch failures use
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
2. `cargo test -p hamlet_protocol` keeps a sha256 of the client schema
   with `VERSION` in `crates/hamlet_protocol/client_schema.fingerprint`.
   When the shapes change it rewrites the file and fails once: commit
   the file. Doc comments are not part of the fingerprint. `VERSION`
   stays at 0.1.0 until Hamlet's first release; after it, a change that
   breaks a peer moves it by semver.
3. Re-record `crates/hamlet_protocol/frames/` when the wire sessions
   change. The offline Rust recorder's command is in the
   [acceptance README](../crates/hamlet_acceptance/README.md#recording-protocol-frames).
   Ordinary tests only read these fixtures.
4. Update this page. `cargo test -p hamlet_protocol` checks that every
   client and server message type is named here.
