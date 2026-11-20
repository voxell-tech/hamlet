# Client protocol

The window, `hamlet chat` and the CLI use one authenticated WebSocket to
Hamlet's server. There is no HTTP API. The wire carries JSON messages.

The contract is the Rust types in
[`hamlet_protocol::client`](../crates/hamlet_protocol/src/client.rs), one
module per area (`state`, `chat`, `waits`, `images`, `files`,
`documents`, `imports`, `limits`, `settings`, `terminals`,
`transcript`); the names below are their type names. Their doc comments
and generated schema say what each field means. The handshake's `v` is
`VERSION`, in semver; a server takes a client whose version is
compatible with its own by semver's caret. Objects reject extra fields.

## Transport and handshake

The server listens on `127.0.0.1` only, over plain `ws://`. Local clients
read the URL from `server.json` in the realm's folder and
`connection.token` from `credentials.json` there. The folder is
`HAMLET_HOME`, else `~/.hamlet`.

For a server on another machine, forward its loopback port through SSH.
Set `HAMLET_SERVER` to the tunnel's local `ws://127.0.0.1:<port>` URL and
`HAMLET_TOKEN` to that server's connection token. `HAMLET_SERVER` requires
`HAMLET_TOKEN` and bypasses local discovery. Keep the token private;
plain WebSocket is not encrypted.

Send `Hello` first, within 10 seconds:

```json
{"type":"hello","v":"0.1.0","token":"<connection token>"}
```

A successful hello receives a full `state` before live changes, which
tells the client the server speaks its version.

| Failure | Answer and connection |
|---|---|
| No hello in time | `fault`; close **1008** |
| Another message first, or a malformed one | `fault`; close **1008** |
| Wrong token | `fault`; close **4001** |
| A `v` not compatible with the server's | `fault` naming both versions; close **4002** |
| A second hello | `fault`; stays open |
| Unexpected connection failure | `fault`; close **1011** |

A client-to-server WebSocket message holds at most **1 MiB**. An
oversized one gets a best-effort `fault` and close **1009**. Images and
files travel in chunks (`PutImage`, `PutImagePart`, `PutFile`,
`PutFilePart`). A server shutdown closes clients with **1001**. These
rules are in
[`clients/connection.rs`](../crates/hamlet_server/src/clients/connection.rs).

## Shared shapes

Ids are in `hamlet_protocol::client`: `AgentId` and `ProfileId` are
integers; `ImageId` and `FileId` are the SHA-256 of the bytes, in
lowercase hex; `RequestId` is a UUID in its 36-character hyphenated form
only. `ChatRef` is `{with: AgentId}`: the one conversation of that agent
(the Hand, an agent of a hamlet or a worker), shared by every chief and
every client, and kept, read as it was, after the agent leaves. The
server resolves typed addresses (`hand`, `lead@<hamlet>`, `agent#<id>`,
`worker#<id>`) in chief commands; messages carry ids.

An `AttachedImage` or `AttachedFile` has `at`, a byte offset into the
line's `text`, on a character boundary and at most its length; any other
`at` refuses the line. An attachment without `at` goes after the text.

Every client message except `hello`, `close` and the `terminal` ops
`input`, `resize` and `unwatch` carries `id: RequestId`.
The answer's `to` matches it; `Say.to` is an `AgentId`, a different
thing. A `reply` settles the request, not necessarily all work in a
conversation. The server makes the durable inputs' own ids
(`chief:<say id>`, `retry:<UUID>` and so on) in
`conversations/chats.rs` and `conversations/reflections.rs`; clients
send UUIDs, never these.

`State.epoch` identifies the running state watcher. Revision starts at
zero and increases when the realm state changes. A full `state` replaces
the client's snapshot; `change` applies its updates with
`hamlet_protocol::client::State::apply`. These counters describe realm
state, not transcripts. Reconnect with hello, which brings a full
`state`, and reopen watches.

## Client to server

All are members of `ClientMessage`. Unless marked, add
`id: RequestId`. Expected failures answer with `reply`; malformed
messages and watch failures can answer with `fault` (see
[Errors](#errors)).

| Type | Does |
|---|---|
| `hello` | Authenticate; no `id`; full `state` |
| `say` | A chief line or typed command; images and files sent before with `put-image` and `put-file` |
| `put-image`, `put-image-part` | Upload an image in chunks; `reply.upload`, then `reply.image` |
| `put-file`, `put-file-part` | Upload a file in chunks; `reply.upload`, then `reply.file` |
| `answer` | Answer a wait |
| `stop`, `resume` | Hold or release an agent's work, or all |
| `rewind` | Take back a chief input and what followed |
| `compact` | Compact an agent's conversation now |
| `withdraw` | Take back the chief's own queued line |
| `import-project` | Bring in another program's project from uploaded `Imported` files |
| `retry` | Ask an agent to carry on |
| `edit` | Edit a chief-owned document or setting |
| `open` | Watch a conversation and fetch a page; `transcript` or `fault` |
| `close` | End a watch; no `id`; no answer |
| `login` | Start, answer or cancel a connection-owned sign-in |
| `check` | Check a model or the connection |
| `read` | Fetch one document or list; `read` or failure `reply` |
| `move-worktrees` | Copy worktrees to the workspace chosen in Settings |
| `terminal` | Open, type into, resize, watch, rename or close a terminal |
| `mcp` | Add, change, remove or restart an MCP server, or find and import those other programs list |

Limits a client must respect on `say`: at most 20 images, each within
what the agent's model takes (a line with images is rejected when it
takes none); at most 10,000 files, 256 MiB each and 1 GiB together. A
client asks its chief before sending more than 32 MiB or 1000 files.
Files are copied into the agent's worktree under `inbox/` and are not
given to the model as content. Uploads: an image is at most 20 MiB (PNG,
JPEG, WebP or GIF), a file at most 256 MiB, and an `import-project`
file at most 16 MiB (50 files, 128 MiB together). `/compact` typed
through `say` does what `compact` does.

`mcp` carries an `act` discriminated by `do` (`put`, `remove`,
`restart`, `find`, `import`); see `mcp`. `state.settings.mcp` shows each
server, its state and tools, and what `find` last found; an `env` value
is never sent, and a `null` value in `put` keeps the one the entry has.

`terminal` is discriminated by `op` (`open`, `input`, `rename`,
`resize`, `watch`, `unwatch`, `close`); see `terminals`. `open` starts
the chief's own login shell, with no sandbox. Columns and rows are 1 to
1000; `data` holds at most 65536 bytes. `input`, `resize` and `unwatch`
carry no `id` and get no answer.

### Questions and answers

A question wait's `detail` carries `questions`: one to ten questions
with at most six `options` each. Send `choice: "reply"` and `picks` in
question order, each `{kind: "offer", index}` (zero-based) or
`{kind: "words", text}`. Every question needs one valid answer; blank
words, unavailable options and words where `text_input` is false are
refused without closing the wait. Picks never accompany `text` or any
choice but `reply`. A plain `text` reply answers a single question that
accepts words. `dismiss` needs neither. Known credentials are removed
before questions or answers are stored or sent.

### Edit, read and login forms

`edit` is discriminated by `doc` (`charter`, `character`, `memory`,
`limits`, `settings`, `catalogue`), `read` by `what` (`prompt`,
`models`, `image`, `papers`, `charter`) and `login` by its one field
(`start`, `reply`, `cancel`); their types are in `documents`, `limits`,
`settings` and `chat`. A rejected `edit` changes nothing and replies
with an error. `charter` with `null` restores the built-in one; an empty
memory `text` removes the file.

Login sessions belong to the initiating connection; disconnecting
cancels them. The provider `claude` signs in with `method:
"subscription"` alone: the server sends one `url` step, then `done` once
the Claude program has the login, and asks for no `reply`. It completes
only in a browser on the server's machine.

## Server to client

All are members of `ServerMessage`.

| Type | Who receives it |
|---|---|
| `state` | Every client that said hello: the full realm snapshot |
| `change` | Every client that said hello: `Update`s since the previous revision |
| `transcript` | A page, or the replacement after a reset; only watchers of that chat, a requested page to its requester |
| `turn` | Live transcript operations, in order; only watchers of that chat |
| `reply` | Receipt, rejection (`kind: "error"`) or words recovered by rewind (`"rewound"`); only the asker |
| `read` | The requested document; only the asker |
| `login` | Sign-in progress; only the connection that started it |
| `fault` | Protocol, watch or connection failure; only the affected connection |
| `terminal` | A terminal's output in base64; only watchers |

A `Terminal` message with `reset` redraws the terminal from a blank
screen: the answer to a watch, or a catch-up for a connection whose
queue filled. The redraw marks each resize as
`ESC ] hamlet;size;<cols>;<rows> BEL` (`hamlet_protocol::client::replayed`
splits them). The server keeps the last 512 KB a terminal printed. Both
ends resize a screen with `hamlet_protocol::screen::resize`.

`Update` replaces the named view or removes it. Library and
closed-hamlet list changes, and hamlet removal, send a whole `state`
instead. `Confinement` (`sandbox`, `insecure` or `remote`) says what
keeps agents' commands from the rest of the machine. `SettingsView.workspace`
never holds the token. `Hamlet`, `Agent`, `Wait`, `Grant`, `Profile`,
`SettingsView`, `ProviderView` and `TerminalView` are complete views in
`hamlet_protocol`.

An `Item`'s ids are opaque cursors (`<entryId>:<block>`); the transcript
holds ids and never image or file bytes. A message from another agent is
a `user` item with `author`, `from` and `message` (`notification`,
`request`, `response` or `error-response`) instead of `chief`. `queued`
marks an input no turn has taken up yet.

`TranscriptOperation` is discriminated by `op`, not `type` (`add`,
`append`, `close`, `remove`, `status`, `usage`, `reset`). Clients apply
them with `hamlet_protocol::client::transcript::apply`. An `add` with
`replaces` swaps a provisional or queued item; a queued item goes last,
any other before the queued ones. `reset` clears the transcript and a
new `transcript` follows.

## Conversations

State carries agents' summaries, not transcripts. `open {chat}` watches
one conversation and returns the newest page first. A connection can keep
several watches and receives live `turn` operations only for those
chats. `close {chat}` ends that connection's watch; disconnect ends all.

A page holds at most **100 items**, not 100 turns
(`clients/requests.rs`). Pages are fetched newest first, but **items
inside each page stay oldest to newest**. `earlier` is the first
returned item's id if older items remain, else `null`; send
`open {chat, before: earlier}` for the preceding page. An unknown cursor
is a fault. Paging leaves the live watch running, even if the page
request fails.

A conversation runs one turn at a time: a chief's lines and retries
first, then agents' messages as they came. Notices, a merge's input, an
error response and compactions are not turns and go at once. Waiting
inputs are the transcript's last items, with `queued`, in run order.
`stop` holds the queue until the chief's next `say`, which runs first,
or `resume`; `status` carries `held` meanwhile. It also carries
`compacting` while the earlier messages are summarized, and `context`,
the tokens the model's context held when last measured.

Without a watch, conversation activity appears only through realm
`change` messages (activity, saying, spend, busy). Turns of
notifications, requests and responses count toward activity and spend
even when no chief watches.

## Ordering and notices

An empty notice `reply` to a `say` acknowledges that its submission
settled, not that all work ended. A request's reply is not ordered
against a chat's `turn` and `transcript` messages. A reset and its
replacement transcript are sent together, with no message between them.

A `Wait` has a `detail` discriminated by `kind`: `approval`, `fault`,
`question`, `objection`, `newcomer` or `mcp-server` (see `waits`). A `check` replies in
words and changes the connection status in settings; a model check
waits at most 90 s.

`State.realm` names the realm's folder on the server's machine so local
links and pictures can exclude it, including through symlinks.
Archived agents remain in state with `left` until erased.

`hamlet_client` reconciles state and changes into entities. Each event
that sends a line, an answer, a stop, a resume, a rewind, a compaction or
a withdrawal carries the `RequestId` its caller made. On reconnecting it
sends `hello` again, keeps unsent requests in its Outbox and reopens its
watched chats. A lost connection retries; a refused token stops retries
and shows the server's words.

## Errors

Errors come two ways: `reply` with kind `error`, and `fault`. Both carry
readable `text` and no machine error code.

| Envelope | Use |
|---|---|
| `reply`, kind `error`, with `to` | An ordinary requested operation is rejected |
| `fault` with `to` | A malformed request with a recoverable UUID, an open/page rejection, a watch failure still tied to its request, or a request-handler failure, a panic included |
| `fault` without `to` | A handshake or connection failure, input with no valid request ID, or a watch failure after its opening request was answered |

A fault does not itself imply disconnect. Malformed input after hello,
a second hello and handler failures leave the connection open;
handshake rejections, size violations and unexpected failures close it as
above. Watch failures use `fault`, even when the cause is an ordinary
access rejection. See `clients/connection.rs` and `clients/requests.rs`.

## Changing the protocol

1. Edit the types in `crates/hamlet_protocol/src/client/`, the authority
   for both directions. A rule a type can hold goes in the type, as
   `RequestId` holds its UUID form. A limit serde cannot check, such as a
   list's length, goes in a `#[schemars(...)]` attribute: the server
   checks every client message against the schema derived from them.
2. `cargo test -p hamlet_protocol` keeps a sha256 of the client schema
   with `VERSION` in `crates/hamlet_protocol/client_schema.fingerprint`.
   When the shapes change it rewrites the file and fails once: commit
   the file. `VERSION` stays at 0.1.0 until Hamlet's first release.
3. Re-record `crates/hamlet_protocol/frames/` when the wire sessions
   change; the command is in the
   [acceptance README](../crates/hamlet_acceptance/README.md#recording-protocol-frames).
4. Update this page. `cargo test -p hamlet_protocol` checks that every
   client and server message type is named here.
