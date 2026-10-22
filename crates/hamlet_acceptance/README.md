# hamlet_acceptance

The acceptance suite for Hamlet's server. Each scenario starts a server
process on a scratch home and checks what it does from outside: through
the client protocol over the authenticated WebSocket, with `hamlet_protocol`'s
generated types, and through the `hamlet` command. It checks behaviour
only, never the server's insides. The default server is `hamlet server`.

Passing these scenarios does not establish exhaustive behavior or platform
coverage. [Known gaps](#known-gaps) lists observable cases no scenario
checks yet. CLI lifecycle scenarios exercise detached start, reuse, status
and stop, and dev with explicit binary overrides.

Every message a server sends is parsed into `ServerMessage`, written back
and compared with what arrived, so a field outside the client protocol fails the
scenario that received it.

## Running it

The scenarios are compiled only with the `acceptance` feature, so plain
`cargo test` neither builds nor runs them. They need the built `hamlet`
and `hamlet-server`, Node from `harnesses/pi/.nvmrc`, and
`npm ci --prefix harnesses/pi`. `cargo xtask check` runs them. By hand, at the root:

```sh
CARGO_INCREMENTAL=0 cargo build -p hamlet_cli -p hamlet_server
CARGO_INCREMENTAL=0 cargo test -p hamlet_acceptance --features acceptance
```

They never contact a model server or the internet. Models are faux, or the
loopback endpoint in `src/openai.rs`.

| Variable | Selects |
|---|---|
| `HAMLET_ACCEPTANCE_SERVER` | The command that starts a server, split on whitespace. Unset or empty: the `hamlet` command's `server`. |
| `HAMLET_CLI` | The `hamlet` command, for the default server and the CLI scenarios. Unset: `target/debug/hamlet`. |

A relative executable path is resolved against the workspace root. To
start `hamlet-server` directly rather than through the CLI:

```sh
CARGO_INCREMENTAL=0 cargo build -p hamlet_server -p hamlet_cli
CARGO_INCREMENTAL=0 HAMLET_ACCEPTANCE_SERVER=target/debug/hamlet-server cargo test -p hamlet_acceptance --features acceptance --no-fail-fast
```

A server that cannot be started fails its scenario, with the end of what
it printed. Nothing is skipped.

## Starting a server

How the suite starts `hamlet server`, or `hamlet-server` when
`HAMLET_ACCEPTANCE_SERVER` names it, and when it counts it as ready.

The suite runs the command with these arguments added:

- `--port 0`, to listen on a free loopback port.
- `--faux <file>`, the absolute path of a faux script (below). Without it,
  the server uses its real model providers; the suite then points them at
  a loopback endpoint with `HAMLET_MODEL` and `HAMLET_BASE_URL`, on the
  `local` endpoint the default settings list.

Its environment is the suite's own with the ambient overrides named by
`hamlet_testing::OVERRIDES` removed: a shell's `HAMLET_MODEL`,
`HAMLET_WORKSPACE_URL`, `HAMLET_FAULT` and the like must not reach a
test server. The suite then sets:

- `HAMLET_HOME`: the home, a fresh scratch folder, or the folder a
  previous server of the same scenario used.
- `HAMLET_USER=chief`: the chief's name.
- What the scenario adds: `HAMLET_REFLECT_EVERY=0` unless it says
  otherwise, and `HAMLET_MODEL`, `HAMLET_BASE_URL`, `HAMLET_NET` or
  `HAMLET_API_KEY` where it tests them.

Stdin is closed. Stdout and stderr go to a log the suite shows on failure.

The server is ready when `<home>/server.json` holds
`{"url": ..., "pid": ..., "started": ...}` with `pid` the process the suite
started:

- `url` is the WebSocket address to connect to.
- `started` is the start time in UTC, as an ISO time to the millisecond.
- It holds no token.

The token is `connection.token` in `<home>/credentials.json`, a file only
its owner can read or write (mode 0600). The token stays the same across
servers on one home. The suite waits up to 30 seconds for `server.json`,
and fails at once if the process ends first.

One server serves a home at a time. A second one on a served home ends
with an error that says `A server already runs for <home>`.

At SIGTERM, or a first SIGINT, the server finishes in order:

- It closes every connection with code 1001.
- It removes `server.json`.
- It exits with 0.
- The next server on the home carries on what was cut off.

A second SIGINT ends it at once with 130. SIGKILL may leave `server.json`
behind; the next server replaces it.

### Files exposed to the scenarios

The suite also checks the public local-workspace files from
[`docs/home.md`](../../docs/home.md):

- `houses/<hamlet>/<number>/NOTES.md`, `skills/` and `inbox/`. The first
  agent is `home:0`. `state.houses` names the absolute houses directory.
- Arbitrary files made by tools or gifts in those houses. Merge scenarios
  check the retained copies in `merged/<absorbed-name>/`, including
  `NOTES.before.md` and conflicting files.
- `library/<skill>/SKILL.md` and the library path in the composed prompt.

Scenarios seed notes, skills and files there and check their bytes,
executable modes, moves, copies and removal. Work folders, editor scripts,
faux files and release markers live outside the home in scratch folders.
No scenario opens a database, a harness store or `models.json`, or uses
server log text for synchronisation. The only asserted startup diagnostic
is the duplicate-home refusal above; a linked houses root must also fail
startup without changing the link's target. The sandbox scenarios require
Linux, working bubblewrap, git, curl and POSIX shell tools. They plant
outside canaries and symlinks, use a loopback listener, and inspect
tool-created files and protocol outcomes.

## Faux scripts

A faux script is a JSON object of answers given in a model's place, so a
scenario knows what each model call returns. The pi harness reads it
(`harnesses/pi/src/faux.ts`). Every field is optional, and no others are
allowed:

| Field | What it holds |
|---|---|
| `models` | Nonempty array of distinct, nonempty model ID strings. Defaults to `["faux-1"]`. |
| `responses` | Steps for any request without a list of its own, in order. |
| `agents` | Steps for each agent, by `"<hamlet>:<number>"`, for example `"home:0"` for the first mayor. |
| `errands` | Steps for one-off conversations: ballots, reviews of a merge, merged notes and the library's calls. |
| `price` | `{"input": dollars, "output": dollars}` per million tokens. Without it, the faux model is priced as the catalogue assigns. |
| `tokensPerSecond` | How fast answers stream. Without it, at once. |

A request takes the next step of its list. A caller with no list of its
own takes from `responses`: an agent without a list in `agents`, a one-off
conversation when `errands` is missing, and a request of no conversation,
such as a model check. Most scenarios give only `responses`. A list that
has run out answers with a model error, `No more faux responses queued`,
and never falls back to `responses`.

Lists are shared across every conversation of that agent, including chief,
letters, questions and reflection. All one-off conversations share
`errands`, which takes precedence over an agent list. Missing `responses`
means an empty list; an explicit empty agent or errand list also fails
immediately. Each call consumes its
step when the provider is invoked, before waiting or streaming, even if it
later fails or is cancelled. Concurrent calls take successive steps in
invocation order. Each new server process starts fresh lists from its
script, including after a crash; list positions are not durable.

A request's list is chosen by the conversation it belongs to, never by
its prompt. The harness knows each conversation by its provider session:
`errands` for a one-off conversation, else the agent the server created it
for, as `<hamlet>:<number>`. A conversation keeps the key it was created
with, even after its agent moves to another hamlet; a conversation created
after the move uses the agent's new address. A request of a session the
harness does not know takes from `responses`.

A step is a string, which is the answer's words, or an object:

| Field | What it does |
|---|---|
| `text` | The answer's words. |
| `thinking` | Its reasoning. |
| `calls` | Tool calls, `[{"name": ..., "arguments": {...}}]`. An answer with calls ends with "tool use", one without with "stop". |
| `error` | A model failure with these words, in place of the answer's ending. |
| `stallMs` | Milliseconds the answer takes. Stopping the request does not cut this short. |
| `holdUntil` | A file the answer waits for, relative to the script's folder. Stopping the request ends the wait. |

`text`, `thinking` and `error` are strings, including empty strings.
`calls` is an array; each call has exactly `name` (string) and `arguments`
(a JSON object). Content is reasoning first, text second, then calls in
array order; omitted fields produce no block. `error` retains that content
but ends with an error rather than stop or tool use. `stallMs` is a
nonnegative integer and runs before `holdUntil`; the latter is a nonempty
path, absolute or resolved relative to the script file. Its existence,
not its contents, releases the response. Cancellation consumes the step
and emits no remaining content, but cannot interrupt `stallMs`.

`agents` keys match `^[^:]+:[0-9]+$`. Prices are nonnegative finite numbers;
`tokensPerSecond` is a positive finite number. Unknown fields in the script,
steps, prices and calls are rejected.

Each `models` ID is offered as `faux::<id>`, in array order, with a
128000-token context and a 16384-token output limit. IDs are case-sensitive,
not trimmed, and are not endpoint-qualified in the script. Without
`models`, the one model is `faux-1`. The default selection with `--faux` is
`faux::faux-1`, so a script with `models` that uses that selection must
include `faux-1`. `models` replaces the listing; it does not append to it.
All models use the same `price`, streaming rate and queues. Switching
models does not reset or choose another queue: the next call consumes the
next step for its conversation identity, even if the previous step used
another model. There are no per-model steps or prices.

The pi Node harness uses pi-ai 1.2.0's faux
provider for usage estimates and streaming. Chunk boundaries, generated
tool IDs and timestamps are not fixed by a script. Rates default to zero;
catalogue price overrides apply to successful responses, with cache rates
zero. Usage is estimated from the actual prompt, so the scenarios assert
accounting relationships rather than fixed faux token counts. The loopback
OpenAI fixture supplies fixed usage for exact billing assertions.

### Loopback endpoint

`src/openai.rs` lists `org/large` with a 32000-token context by default.
`list` replaces its `/models` response. `enqueue` queues completion
responses in request-arrival order, consumed before any hold. An empty
queue answers `Hello.`. Responses can set HTTP status, text, finish reason
(including `length`), OpenAI tool calls and final usage, or wait for an
absolute marker path. Defaults are HTTP 200, `stop`, 100 input tokens,
20 output tokens and a $0.0015 bill. `usage: None` omits usage; explicit
zero cost is distinct from missing cost. Listings never consume completion
responses. Dropping the endpoint closes held requests too.

The fixture records requests at the external provider boundary to check
model choice, retry, compaction and the actual prompt. This is not access
to the server's internal transcript. Ordinary tool output is not exposed
in the client protocol, but can be checked in the next model request when needed.

## Scenarios

| File | What it covers |
|---|---|
| `tests/mayor.rs` | Chat and turns, tools, approvals and standing permissions, faults, retry, stop, rewind, new and resume, paging, budget stops, questions, crash and orderly stop, activity |
| `tests/documents.rs` | The charter, characters, notes, history, the prompt, limits, settings, the catalogue, the folder and mode, refusals of unknown targets |
| `tests/connection.rs` | The hello and token, malformed and oversized messages, broadcasts, closing, `server.json` and `credentials.json`, one server per home |
| `tests/society.rs` | Letters, replies and questions, roads, protection, removing, restoring, erasing and merging agents, votes, objections, the library, what the mayor may create |
| `tests/realm.rs` | Founding hamlets by command and by vote, roads and moves between hamlets, removal asks, closing, restoring and erasing hamlets, their papers |
| `tests/gifts.rs` | Files given in parts, folders, refused paths and manifests, upload bounds, starter and copied skills, gifts to departed agents |
| `tests/models.rs` | Model checks, grants, key logins, changing listings and catalogue persistence, model selection and pins, retry, usage, compaction and the actual prompt |
| `tests/cli.rs` | `hamlet chat` on stdin and stdout, approvals across a crash, switching agents and hamlets, `hamlet edit`, `hamlet models`, `hamlet login` |
| `tests/sandbox.rs` | Linux read/write/network confinement, symlinks and missing roots, house versus project files, resource limits, timeout cleanup and hand-added tools |

## Known gaps

What a client of the client protocol or `hamlet` could observe but no scenario
checks yet, grouped by what checking it needs. Process-crash controls at
exact commit boundaries live in `crates/hamlet_server/tests/`.

- **Nothing new: the suite can drive these today.**
  - Mayor: a house `NOTES.md` edit not held in manual mode; the gate's
    lowered asking limit, three refusals, binary and empty previews,
    credentials in held text and linked paths; edits during a held wait;
    retry after a stop; withdrawal of reflection on stop; refused rewinds
    of a running turn or another conversation; the `/resume` list,
    `/budget` after spend and typed `/dismiss`.
  - Server: a late joiner mid-turn, a turn that finishes after every client
    has gone, quick lines answered in order, a resent ID on a new
    connection, a message sent just before close, and a client that does
    not read not delaying shutdown.
  - Documents and models: the folder and mode across a restart, `/folder ~`
    and folders inside, above or linked to the home; restoring the default
    charter; limits edits such as `passOf 0`, `ballotMinutes -1`,
    `hamlets.most 1.5` and a whole votes or hamlets edit; environment
    overrides not kept, a stored endpoint restored without them, and bad
    overrides; the providers list and a non-unique endpoint name; the
    shape of `credentials.json` and `HAMLET_API_KEY` never written.
  - Society: refusal words of agent creation, departure, roads, the
    membership cap and merges; restoring under a taken name or into a full
    hamlet; departure stopping a turn and its waits; vote rules (silence,
    grounding, one vote at a time, resume after a restart); objections
    after a restart, late or outlived, and the mayor's review; the
    librarian's refused verdicts and unfit offers; errands' kept answer,
    failure, timeout and abort.
  - Realm: a half-done move finished on restart, a vote blocking an
    agreement, removal stopping work and lapsing its waits, erasing then
    restarting, and a refused clean-up through a symlink.
  - Gifts and library: simultaneous gifts, size, entry and link limits on
    skills, idle expiry, rollback, a linked or deleted inbox, starters
    written once as shipped, and a deleted or edited starter left as the
    chief left it across a restart.
  - Protocol: the rest of the malformed field forms, and grants kept
    across a restart with a repeated revoke recording nothing.
- **The next model request.** Ordinary tool output is not in the client protocol.
  Search and find results, letter and librarian refusal words, the
  explanation of a held decision, sandbox hints and the offered tool
  definitions can be checked in the next request to the loopback model.
- **Real waits or a public test clock.** Daily and monthly spend rollover,
  multi-day decision ageing, model-time wording, the 90 s hello timeout,
  the 120 s reflection bound and ballot timeouts. The suite adds no private
  clock controls.
- **A second chief.** One token and a fixed `HAMLET_USER` give one chief:
  refusals in another chief's hamlet, joined waits, per-chief chats and
  queues, and a home keeping its chief when `HAMLET_USER` or `USER`
  changes.
- **An OAuth fake.** Subscription sign-ins, their codes and scrubbed
  failures, and a key kept beside an OAuth credential.
- **A pty.** The tty prompt, Ctrl-D, the terminal editor and hidden key
  input.
- **Other environments.** A real graphical window and platform bundles; a
  remote workspace; a live local model; a host without bubblewrap, for the
  unavailable-sandbox words; a non-loopback endpoint, for remote-mode
  reviews.

## Recording protocol frames

`tests/frames/` records 18 real server sessions through the client protocol,
including file gifts in chunks. It uses scratch homes, faux responses and
a loopback login endpoint, never a live provider. Both wire directions are
saved as JSONL; connection tokens and the fake login key are redacted.
IDs, timestamps and stream boundaries come from the running server.

From the repository root, using the Node version in `harnesses/pi/.nvmrc`:

```sh
npm ci --prefix harnesses/pi
cargo build -p hamlet_server
HAMLET_ACCEPTANCE_SERVER="$PWD/target/debug/hamlet-server" \
  cargo test -p hamlet_acceptance --features acceptance --test frames -- --ignored
cargo test -p hamlet_protocol -p hamlet_client
```

For a custom Cargo target directory, set `HAMLET_ACCEPTANCE_SERVER` to the
absolute path of that build's server. Recording overwrites
`crates/hamlet_protocol/frames/*.jsonl`; ordinary tests never do. To inspect
new captures without replacing the checked-in fixtures, set
`HAMLET_FRAME_DIR` to an absolute scratch directory for the recorder and
the schema package's `npm test`. Rust client replay reads the checked-in
fixtures. Review replacements before keeping them; changing server code
does not by itself require fixture churn.
