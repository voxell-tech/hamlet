# Proposal: the Rust server and its harnesses

Rebuild the server in Rust, as the crate `hamlet-server`, and run agents'
conversations in harnesses apart from it. The decision is
[ADR 0001](../adr/0001-agents-run-on-a-rust-server-and-harnesses-run-their-conversations.md).
The concepts and rules stay as [`docs/spec/`](../spec/README.md) states
them. Today's server is TypeScript, in `server/`, with Hamlet's rules and
pi-durable in one Node process
([below](#todays-server)). It is the reference for how Hamlet behaves until
the Rust server replaces it.

## The end

A Rust server and no TypeScript server. Everything that works today works
with `hamlet-server` and, beside it, Node processes running the pi
harness. The pi harness is a package of its own, for example
`harnesses/pi/`, and `server/` is deleted.

It is done when the acceptance suite passes against the Rust server. The
suite is black-box: it starts a server process on a scratch home and drives
it through protocol v0 with faux scripts, knowing nothing of how the server
is built. It passes against the TypeScript server first, and the same run
passing against the Rust server is the bar for deleting `server/`.

The proposal is work to build, starting from the crate's foundation. The
old Rust runtime crates (`hamlet_core`, `hamlet_rules`, `hamlet_model`,
`hamlet_home`, `hamlet_sandbox`) are material to reuse where it fits, not
code assumed to match the TypeScript server.

## The shape

```
Window, hamlet CLI (Rust)
        |
        |  protocol v0, one WebSocket with a token (ADR 0004)
        v
hamlet-server (Rust)  ---------------------------  the server's store
  the rules, the record, waits, budgets,
  prompts, tool permissions, the sandbox
        |
        |  the harness link: private, versioned
        v
Runtime (Node: pi-durable + pi-ai 1.2.0)  -------  harnesses/<instance>/
  model calls, history, tool results,              (the runtime's store)
  checkpoints, compaction, forks
```

The `hamlet` command starts `hamlet-server`, and `hamlet-server` starts
and watches its runtimes. The crate is `crates/hamlet-server/`, its
package and binary `hamlet-server`, imported as `hamlet_server`. The pi
harness is a Node package of its own, outside `server/` and not part of
the Rust build.

## Who owns what

Each thing has one owner, which alone writes it. Neither process opens the
other's store or writes its tables.

| The server owns | A runtime owns |
|---|---|
| Agents, hamlets, membership, names, the mayor's office, roads, departures and the archive | A conversation's messages and tool results, which are its transcript |
| Letters, questions, votes, tasks and their caps, and inputs waiting to be admitted to a conversation | Which inputs it has admitted, by request ID |
| What waits for a chief, approvals, standing permissions and objections | Checkpoints of work in progress |
| Budgets and the spend ledger | The usage and cost it recorded for each model call |
| The charter, characters, limits, settings and allowed models | The configuration a conversation runs with |
| Each agent's harness and each conversation's binding | Compaction, forks and the rest of its own history |
| Markers of the results of operations | The results it recorded for the agent |
| Prompt material, which tools an agent may run, the sandbox and where tools run | Translating these into pi's sections, tools and hooks |
| Protocol v0, the connection's token and what it publishes to clients | Nothing a client sees directly |
| Credentials, the stored model lists, and sign-ins as the chief sees them | Provider work done on request with pi-ai: model calls, a sign-in's steps, refreshing a login and fetching model lists; it keeps none of it |

The server may cache a transcript to serve clients, but never keeps a
second transcript of its own. Notes, skills, the library's skills, tools
added by hand and the folder a hamlet works on stay files (ADR 0005).
Credentials stay out of both stores (S7). The server holds them in
`credentials.json` and hands a runtime the one a call needs, in memory,
when it allows the attempt. Signing in and refreshing a login run in a
runtime's pi-ai at the server's request, and the server keeps what they
return. Nothing provider-specific is written again in Rust.

## The home on disk

| Path under the home | Holds |
|---|---|
| `realm.sqlite` | The server's store: hamlets' records, the realm, waits and approvals, spend, the library's record, the chief's documents, the bindings between conversations and harnesses, and markers. Not conversations, and not files such as notes and skills |
| `harnesses/<instance>/` | One runtime's store, opened only by that runtime |
| `credentials.json`, `models.json`, `server.json`, `server.log` | As in [`home.md`](../home.md) |
| `houses/`, `library/`, `tools/`, `librarian/`, `.gifts/`, `.erasing/` | As in [`home.md`](../home.md) |

The server's store is not called `home.sqlite` because that is the
TypeScript server's pi-durable storage. Reusing the name would let the old
server mistake a migrated home for a fresh one.

`hamlet-server` claims a home exactly as `server/src/home/lock.ts` does:
it listens on the local socket named from the home's path as the Rust app
names it, for both the path as given and its canonical form. A home is
then served by at most one server of either kind (S2, S21). The runtimes
are started by the server that holds the claim and stop with it.

A store that is missing or belongs to another home or runtime is an error.
The server never replaces it with an empty one (S25).

### Gifts (H25)

The server owns file uploads in memory by connection and stages their
bytes under the claimed home's `.gifts/`. A manifest reserves its full
size across connections for the resolved agent: at most 32 MiB, inclusive,
and three waiting uploads per connection. Invalid parts discard the whole
upload. Disconnects and 60 seconds without a part discard it too; startup
removes abandoned stages. No upload metadata or file bytes go in SQLite,
so this needs no migration (ADR 0005).

House and stage paths are opened component by component without following
links, using the same opened-home anchor as notes. Completed top-level
trees move into the inbox with no-replace renames, retaining the reference
server's numbered names and rolling back earlier roots if a later one
fails. Only a complete delivery admits the chief's instruction through
the existing conversation input path. Its receipt is immediate; the
instruction does not owe another receipt when the model finishes.

Skill gifts copy a bounded no-follow tree, including empty folders and
ordinary permission bits, without setuid or setgid. Starters use the
embedded `hamlet_home` assets, whose bytes match the TypeScript assets,
independently of edits to the shared library. Successful skill gifts keep
a `SkillGiven` decision with the resolved giver, refresh the client state,
and cause no conversation turn. Recipient resolution, including a farewell
after departure, belongs to `Service::agent` and is checked again before
an uploaded gift is delivered.

As in the reference server, delivery and the following conversation
admission or skill decision are separate effects. This does not promise
exactly-once gifts across a process crash between those effects, nor an
all-or-nothing multi-root gift across power loss. Uploads do not resume
after a restart. Remote workspace delivery remains outside this port.

## Harnesses, runtimes and bindings

A harness is a kind of program that runs conversations: pi-durable is the
first. A runtime is one running process of a harness, with a stable ID, a
pinned adapter and harness version, and its own store under
`harnesses/<instance>/`.

- An agent has a harness for its new conversations, kept by the server.
- A conversation has a binding: its runtime, the runtime's ID for the
  conversation, and the adapter and harness versions it began with.
- Agents whose harness and version match may share one runtime. Another
  harness, another version or isolation asked for gets another runtime.
  An agent is not a process.
- Changing an agent's harness keeps its identity, house, roads and
  permissions, and changes only its later conversations. An existing
  conversation never moves (S17).
- A runtime's version never changes under it. To upgrade one agent, its
  new conversations go to a new runtime. Opening an existing store with a
  new version needs a step that checks the two are compatible, or migrates
  the store.

A one-off conversation (a ballot, the mayor's review, a library call) is a
conversation like any other, bound to a runtime.

## The harness link

The server and a runtime talk through one private interface defined by
Hamlet. It is not a public endpoint, and it exposes no storage and none of
pi's task APIs. Its transport is the runtime's standard input and output,
which only the server holds: one JSON message per line. The messages are
defined in TypeBox in `harnesses/pi/src/link.ts`, checked in as
`harnesses/pi/link.schema.json`, and generated into the server's Rust
types, so both sides are tested against the one schema. Link version 1 is
the only one.

### Starting and versions

1. The server starts the runtime with its ID, its store's path and the
   link's address (`--runtime`, `--store`, `--link stdio`), and says when
   the store may be made, which is only before the runtime first began.
2. The runtime answers with the link versions it speaks, its adapter and
   harness versions, the runtime ID its store records, and its
   capabilities.
3. The server refuses a link version it does not speak, a version that
   differs from the binding's, or a store whose runtime ID does not match.
   A refusal is an error in words for a chief, never a fallback, and the
   refused runtime is not started again. A store that is missing is
   refused the same way.
4. Otherwise the server welcomes it with the tools it may offer, then
   creates the conversations it owes and submits the inputs it owes.

Every message carries a version and an ID. A reply names the ID it answers.
Errors are messages, not closed connections.

### The server asks a runtime

| Request | Carries |
|---|---|
| Create a conversation | Who it is for and its configuration; the answer is the runtime's conversation ID, which the server keeps in the binding |
| Configure | A change of model, tools or settings, applied from the next model call |
| Submit | An input with its request ID, sender and task |
| Cancel | Stop a conversation's running turn and drop its queue (C25) |
| History | A page of the transcript before a cursor |
| Status | What each bound conversation is doing, and what it recovered |
| Fork | Copy a conversation up to an entry, for a rewind (C27); refused by a harness without forks |
| Compact | Compact now; refused by a harness without compaction |

### A runtime asks the server

| Request | Answered with |
|---|---|
| Prompt material | The sections and tools for the next model call (C11, H35) |
| A model attempt | Allowed, with what is needed to make it, or stopped with the words of what ran out (H8, H12) |
| Usage | The usage of an attempt, under the attempt's identity; acknowledged once charged |
| Scrub | A value with all credentials currently known to the server removed, before the runtime keeps provider output (C32, S7) |
| An operation | A tool call or a change to the record, under its operation identity; answered with its result, a wait, or a refusal |
| A wait's answer | The chief's answer to a wait the runtime is holding open |

A runtime also sends committed updates of its conversations: items, text,
tool results, status and usage, only once kept (S3). The server turns them
into protocol v0's `turn` and `transcript`.

### IDs

| ID | Made by | Unique within |
|---|---|---|
| Runtime ID | The server, kept in the server's store and the runtime's store | The home |
| Conversation ID | The runtime | Its runtime |
| Request ID of an input | The server, kept before it is submitted | Its conversation |
| Operation identity | The runtime ID with a key local to the runtime | The home |
| Attempt identity | The runtime ID with a key local to the runtime | The home |

Request IDs keep today's forms: `chief:<id>`, `retry:<uuid>`,
`reflect:<number>:<ordinal>`, and an ID for each letter, question, reply
and ballot taken from the server's record of it.

Pi's task IDs are numbers local to its storage, so an operation identity
is the runtime ID and the task ID together. A provider's tool-call ID is
never treated as unique.

### Admitting inputs

The server keeps every input it owes a conversation, in order, before
submitting it. A runtime admits a request ID once: submitting it again
admits nothing new and answers as the first time did. The server marks an
input admitted only when the runtime says it kept it, and submits again
after a restart or a lost reply. Letters, questions, votes, follow-up work
and reflections all go this way, keeping each conversation's order (C23),
the sender, and the task's caps across agents (H12), so each is admitted
once (S23).

pi-durable admits by request ID inside the commit that admits: submitting
looks the request ID up in the conversation's submissions in that same
transaction and answers with the submission it finds. Its table indexes
the request ID without making it unique, so this holds because one
runtime process alone writes its store. Shown with pi-durable's own APIs
by a process killed with SIGKILL after admitting and a new one submitting
the same ID (`harnesses/pi/test/admission.test.ts`), and through the
server with the runtime killed before and after pi admitted an input.

### Operations and markers

Every tool call and every change to the record is an operation the
runtime asks the server for. The server then:

1. finds a marker for the identity and, if there is one, returns its
   result;
2. refuses the identity if it was used with a different request;
3. checks the request against the authority, permissions, waits, limits
   and budget as they are now;
4. commits the change, its charge and the marker of its result in one
   transaction;
5. returns the result, which the runtime records in its own store.

A reply lost on the way is asked for again and gets the marked result, with
no second change or charge (S19). Markers are kept for good; a rule for
clearing them needs its own reasons.

This makes changes to the record safe to repeat. It does not make a shell
command or a call to another service happen exactly once. Such an
operation is marked as begun before it runs. Asked for again with no result,
it is not run again: it answers that the call was cut off, with the output
it had (S3).

### Allowing at the time of the effect

A hook in the runtime, such as pi's `beforeTool`, can ask early so that a
wait shows at once, but its answer is never the last word. The prototype
showed that pi-durable, recovering a tool call it had begun, can carry it
out without running `beforeTool` again. The server decides when the
operation is carried out (S18):

- What waits for a chief is kept by the server under the operation's
  identity. A runtime that connects again finds the same wait.
- A refused, taken back, stopped or lapsed permission allows nothing new.
- An operation that already finished returns its marked result.
- Taking a permission back undoes nothing it already allowed.

### Model attempts and spend

Before every model call the runtime asks the server, which applies H8 and
H12. After it, the runtime reports the attempt's usage under its
identity, and the server charges it once however often it is reported
(S20). The pi harness holds a call's last event until the server has
charged it, so pi keeps no answer that is not charged and there is nothing
to reconcile on reconnecting. The cost charged is what the endpoint says
it billed, when it says, else pi-ai's price.

A provider can answer a request whose answer is then never recorded. It
shows as an allowed attempt with no usage, charged nothing. What it should
be charged is open.

The server holds the credentials, model lists and sign-ins. It hands the
one credential a call needs to the runtime, in memory, in its answer
allowing the attempt; pi-ai stores nothing.

#### Models and spend ownership (step 4b)

`credentials.json` and `models.json` belong to Rust. The chief's catalogue
is a chief document in `realm.sqlite`, separate from fetched lists. A
refresh can change a listed price and context, but keeps a chief's price,
allowance and note, including cards no longer returned by the provider.
This closes the question of which store owns catalogue edits and which
side merges a refresh (S7).

Provider work uses `provider_request` and `provider_result` on the private
link. Rust asks for built-in descriptions, a model list, a hello, or a key
sign-in; pi-ai does the provider work with in-memory credentials and no
credential or model-list store. Custom OpenAI-compatible listing and key
adapters live in the pi package. The sign-in belongs to the requesting
connection; cancellation or disconnect discards its eventual result.
Requests are independent, bounded, and are not persisted or retried by
the server. OAuth and subscription sign-ins remain deferred.

The server chooses the model at each allowed attempt: a chief's pin,
then an allowed agent choice, the hamlet model, and the home default. It
snapshots that model's price with the attempt. The runtime uses that model
and its one credential, rather than retaining the credential in a
conversation's configuration. `HAMLET_API_KEY` is removed from the child
environment. Before handing provider events to pi-durable, the adapter
snapshots their mutable partials and sends them through `scrub` on the
private link. Rust removes every currently known credential from text,
thinking, tool arguments and failures; the adapter also removes the
attempt's credential locally. The server does not hand the runtime other
providers' keys for scrubbing. If scrubbing fails, no unchecked output
reaches pi's store.
Pi keeps cumulative partials and final messages, so a credential split
across provider chunks is removed from the accumulated message (C32).
A future consumer that concatenates raw deltas must also hold back
possible credential prefixes across chunks.

The ledger records elapsed model time, first-token time when a delta
arrives, token categories, reasoning tokens, the end reason and a billed
charge when present (C31). An explicit zero bill stands over a catalogue
price. Otherwise positive pi-ai cost stands, then the attempt's price;
successful unreported calls estimate four bytes per token, while failed
calls do not invent tokens. Raw provider usage beyond these fields is
not yet retained.

Daily and monthly books follow local calendar days. A budget reset appends
the model and road ledgers' high-water marks, so a charge in the same
millisecond after a reset still counts. A reset changes the period's
report, never the ledger or a task's lifetime spend (H8, H12). This closes
the question of reset ordering. The pi runtime identifies the running
input, not a newer queued input, on an attempt; Rust namespaces it by
runtime. Shared task identities and a separate payer for errands across
agents or hamlets still need the society owner. A refused attempt's words
show as the budget notice in the chief's conversation.
An allowed call whose answer is lost remains the open charging question
above.

#### Compaction and model changes (step 4c)

A conversation keeps its identity and transcript when a model choice or
pin changes. Its next allowed attempt uses the current choice, with the
chief's pin first. A request already allowed keeps its selected model,
credential and price through its response; a restart makes a new attempt
under the current choice. The binding's initial model is a runtime seed,
not a permanent selection. This closes the question of model binding.

The link gives pi the selected context windows before admitting inputs
and before resuming a store. Pi keeps 20% for an answer and begins
background compaction another 10% earlier, as TypeScript configures it.
Each allowed attempt refreshes its model metadata. Unreported usage
estimates include history and the tool definitions embedded in pi's
system entries; reported token categories remain unchanged (C28, C31).

For a final cut-off with no visible answer, Rust decides whether recovery
is allowed. Reflection and a first line with no history do not recover.
Pi durably hands the original run and inputs to a recovery task, aborts
the cut-off generation, and creates a blocking summary task. The summary
and the one successor generation retain the original input's charge
identity even while newer lines queue. A restarted old generation cannot
request the provider once it has handed off its run. The exact user entry
remains in context; recovery never trims or reconstructs its words.

An empty or cut-off summary, a provider failure, and a second cut-off end
with the corresponding TypeScript fault words. Every summary still asks
Rust for allowance, so lowering the budget can prevent it. Stopping the
recovery settles the original input as unanswered. Real-process tests
kill the server before the old generation is aborted and after the
summary is committed, then check one summary, one retry, unchanged input
and original-task charges. Provider answers lost before being charged
remain subject to the open charging question above.

### Cancelling

Stopping a turn (C25) marks its unadmitted inputs dropped in the server's
store before the runtime is told, so no restart submits them again, and
keeps that a cancel is owed. The runtime ends the running turn and drops
its queue. A cancel owed to a runtime that is down is sent when it is
ready again. Inputs kept since remain in the server until the cancel's
acknowledgement is committed. This closes the lost-acknowledgement race:
replaying the cancel after a server crash cannot stop a newer input that
the server already sent. A real-process test withholds the acknowledgement,
keeps a newer line, kills the server, and checks that line answers once
after restart. Operations of the stopped turn
that arrive later are refused, and its waits lapse (H34). Shown with a
turn waiting for a chief and an input queued behind it, and with the
runtime killed while both were in pi. An input pi answers `unanswered`
is marked dropped atomically with its terminal status and is never resent.

### Recovering

When the server opens a home it claims it, opens its store, starts each
runtime with bound conversations, checks each as above, asks for their
status, submits again what was not admitted, and lets each runtime carry on
its own interrupted work. A runtime that fails stops only its own
conversations; the rest of the realm goes on, and work waiting on an answer
from one of them waits (S22). The server starts it again after 250 ms,
doubling with each failure up to 30 s, and forgets the failures once it
has run for a minute. It never gives up on its own; when it should is
open.

A runtime whose link to the server closes stops its work and exits, so no
agent runs without its server (S21). It notices by the end of its
standard input.

A message that fails while being handled, including a duplicate bound
conversation ID, refuses and stops that runtime alone (S22). A refused
runtime cannot acquire new bindings. The server reports the fault and
keeps serving the other runtimes; retrying an invalid runtime message
cannot terminate the realm.

Who acts in an operation is never taken from the runtime or a tool's
arguments. The server derives the actor from the conversation's binding
and the record as it is when the operation runs.

A rewind forks in the runtime, then the server moves the chat to the fork
in a change it can recover. Waits in the part taken out lapse. A rewind
undoes no change to the record and no effect outside.

### Conversations (step 4a)

A chief's chats, their sessions, the transcript each runtime kept, the
inputs, what waits for a chief, the gate's decision of each call, the
asking within a turn, session trust and grants are in `realm.sqlite`. The
server folds the protocol's transcript from the items a runtime keeps and
its live state, as `server/src/transcript.ts` does: items named
`<entry>:<block>`, live items `live:<n>`, pages of 40, and a rebound chat
reset and sent whole. Paging reads the kept items, not the runtime. A
line's reply goes out once its input settles. A new or resumed session's
reply waits until its notice is kept, so what the client opens next shows
it.

The prompt is composed in the server for each model call, as
`server/src/prompt/` composes it, and the budget words are held for the
whole turn of an input. A final answer with no words and no call fails
its turn. An answer cut off for room fails at once, with the words of a
failed summary, because the link has no compaction (C28).

Each tool call is one operation the server carries out. The runtime holds
no tool, not even a file operation. The gate decides at `allow` and keeps
the binding and a digest of the input, tool and arguments with the call.
A changed request cannot reuse an allowance or a marked result. Legacy
calls without that identity are refused, with their old outcomes retained.
Automatic allowances recheck current grants and trust at execution; an
explicit per-call ruling and a verified marked result stand. A held call
waits, across a crash, until a chief answers it. Its runtime is then woken
to ask again. Cancellation durably refuses unmarked calls, including
questions, even after the runtime acknowledges the stop. A chief's choices
to always allow are kept as session trust for edits and commands, or as
grants for the network and unconfined runs.
File operations walk held directory descriptors without following an
unchecked link; write previews use the same read policy as the agent.
A command runs confined by bubblewrap or Seatbelt, ported from
`server/src/sandbox/`. The server probes the sandbox at start and does not
start without one. `bash`, `write` and `edit` keep a begun marker before
they run. One that a restart interrupted answers that it may have
partially run, and is not run again. `read` is safe to repeat, though
TypeScript marks it unsafe. Credentials are scrubbed from what a tool
shows and from live text, holding back the start of a secret that may be
split across updates.

A closing home asks for the reflections it owes and waits up to two
minutes for them. Then it closes each runtime in order and kills a tool
still running. A second signal exits at once with 130.

Hand-added tools (C17) are loaded by Rust from `tools/*/manifest.json`,
in folder order. Reserved or duplicate names, invalid schemas and
invalid limits are reported in the machine prompt. The link replaces the
runtime's definitions when manifests change; execution reads the current
manifest. Programs receive JSON on stdin, run from their manifest folder,
and get only their declared reach, limits and environment. Each call has
private scratch space, removed when it ends. The tool folder and the
home's gifts and erasure staging stay read-only even when declared
writable. Hand tools keep the same unsafe begun marker as shell commands.

Deliberate differences from TypeScript:
- A tool call's summary lists its arguments in key order.
- A faux script's step is taken back when the server refuses the call
  that took it, as TypeScript refuses before the model is reached.

This closes the open questions of how the gate reaches the record (it
answers in the server, with the record as it is then), of history
cursors (paging reads the server's kept items), and of whether shell
commands and writes keep a begun marker (they do).

## Capabilities of a harness

A runtime declares what it can do. What Hamlet never gives up is not
optional: a harness that cannot meet it is not offered for Hamlet's work,
whatever it declares (S24).

| Capability | Needed |
|---|---|
| Every tool call goes through the server as an operation | Always |
| Every model call is allowed by the server first, and its usage reported with an identity | Always |
| Inputs admitted once by request ID, in order | Always |
| Cancelling a running turn and its queue | Always |
| Carrying on its own work after a restart | Always |
| Committed updates, sent only once kept | Always |
| History in pages | Optional; paging is refused without it |
| Forks, for rewinds | Optional; a rewind is refused without it |
| Compaction | Optional; a conversation that outgrows its model fails without it (C28) |

What a harness lacks is refused in words for a chief, never done another
way. Each new harness needs its own check against this table and its own
tests; what is shown for pi does not carry over to it.

## Tools and the sandbox

Tools run apart from the conversation (ADR 0003), and the server decides
where: in the sandbox on the chief's machine (C18, S12) or in a remote
workspace ([`remote-workspaces.md`](remote-workspaces.md)). The runtime
holds no host tools. For pi, each tool call is one operation the server
carries out (see [Conversations](#conversations-step-4a)).

## Protocol v0

The window and the `hamlet` command keep protocol v0 as it is. The TypeBox
definition in `server/src/protocol.ts`, with its exported schema and the
Rust types generated from it, stays the source of the protocol while
`server/` exists.

The acceptance suite (see [The end](#the-end)) is the check that the Rust
server speaks it and behaves as the TypeScript server does. The recorded
sessions that `hamlet_client` replays move out of `server/test/frames/`,
so they outlive `server/`.

## Moving homes and moving back

Development uses scratch homes only. For a home the TypeScript server made:

- Moving is offline, with no server running, and leaves the original
  home as it was.
- Conversations keep their IDs and numbers. What waits, inputs not yet
  answered and the links between chats and conversations are kept. Nothing
  pending is dropped without a word.
- The records are checked after import and before the home is served.
- Moving back restores the server's store and the runtimes' stores
  together, never one alone.
- The old and new servers never open one home at once (S2).

No home is switched until moving and moving back are tested.

## Today's server

`server/` holds Hamlet's rules as pi-durable extensions in one Node
process over one pi-durable storage, `home.sqlite`
([`home.md`](../home.md)); [`tour.md`](../tour.md#the-server) maps its
modules. Each concept below has to keep its behaviour when it moves.

| Hamlet (spec) | In today's server, on pi-durable |
|---|---|
| A conversation (C22) | A conversation; the chief's, the letters' and the questions' each one |
| The prompt's parts (C11) | Sections of Hamlet's extensions, rendered before each request; the budget section reads words fixed at the turn's start |
| Tools by role (H35), a ballot's one tool | `configure()` of the conversation's tools; applies from the next request |
| Read, write, edit, bash, search, find (C16) | pi-durable's `read`, `write` and `edit` on Hamlet's environment; Hamlet's `bash` wraps pi's with the network flag, timeout and scrubbed output; `search` and `find` are Hamlet's own |
| Tools added by hand (C17) | Tools registered by a loader extension from the home's tools folder |
| The gate (C7, H11) | A `beforeTool` hook in the oversight extension |
| The hamlet's record, the realm, oversight, the library's record, limits, settings | Session-scoped documents; a tool commits a change with a marker of its result, so a repeated call returns that result |
| A task and its caps (H12) | A document of the task's counts, read by the gate and the budget check |
| Spend (H8) | pi-durable's kept responses and aggregate summary usage, charged in integer micro-dollars to Hamlet's ledger by hamlet and local day, agent and task; roads are `road` ledger lines |
| Letters (H30) | A submission to the receiver's conversation with the sender; its answer is submitted back to the sender's conversation |
| Questions (H30) | A tool that submits to the receiver's conversation and waits for it, as pi-durable's subagent example does, so it survives a restart |
| Votes (H13) | One durable task per vote, with hearing, ballot and settling phases; each model call is an errand |
| Errands | Durable one-off conversations, deduplicated by request id and charged like other model work; ballots and the mayor's answer keep their own prompt, while library calls, merged notes and review use supplied instructions |
| Reflection (C5) | A queued server input with its own task and a reserved ordinal, kept across a restart; zero allows only reflection a chief asks for; with a nonzero count, an orderly close waits up to 120 s for owed reflections able to run |
| Summarising (C28) | pi-durable's compaction, set to C28's thresholds |
| Taking a conversation back (C27) | A fork from the entry before the message, with the conversation pointed at the fork (ADR 0002) |
| What the window watches (S5) | `watchEvents()` per open conversation, and the documents' changes for the realm's state |

Its tests in `server/test/` include offline proofs against pi-durable 1.2.0
with a fake provider: sections and `configure()` reaching the next request,
a budget check ending a run cleanly, a `beforeTool` wait surviving a
restart, a question surviving a restart on either side, the sandbox passing
`registerEnvConformance`, and a record change committed with its marker.

What it does not yet meet, and the Rust server must:

- C31: first-token timing is not recorded for every call; raw provider
  usage and each summarising call's timing, billed charge and end reason
  are not recorded. Summaries' model time is not charged (H8).
- C32: an assistant's own messages keep their text, reasoning and
  tool-call arguments unscrubbed in pi's storage.

## Order of work

1. **Foundation.** The crate, claiming the home, `realm.sqlite`, and
   markers.
2. **The harness link and the record.** The pi harness package begins, in
   its own folder. The server starts and watches a Node pi-durable
   runtime; one conversation runs end to end with a faux model, and its
   tool calls run in the server under a marker. In the server: agents,
   membership, names, roads and the mayor's office. Admitting inputs is
   proven here.
3. **Protocol v0 from Rust.** The existing window and CLI connect to the
   Rust server. The acceptance suite is written and passes against the
   TypeScript server; the Rust server passes what it covers so far, and
   more with each later step. The recorded frames move out of
   `server/test/`.
4. **The mayor alone.** Chat, tools, the gate, spend and limits, models and
   logins, and the prompt. Cancelling and charging usage once are proven
   here.
5. **The society and the realm.** Letters, questions, votes, the library,
   leaving and merging, and founding hamlets, each recovering across
   runtimes.
6. **Moving homes.** Moving a TypeScript home and moving it back, both
   tested.
7. **Switching.** The whole acceptance suite passes against the Rust
   server. The `hamlet` command starts `hamlet-server`, `server/` is
   deleted, and the old Rust runtime crates are removed. The pi harness
   package stays, in its own folder.

Steps 6 and 7 are apart because no home may switch before moving back is
tested, and the suite's whole pass is its own gate.

## The prototype

A standalone experiment, not Hamlet code: a Rust process with a small
schema of roads, approvals, a budget and markers, and a Node worker running
real pi-durable 1.2.0 with its own SQLite file, over a Unix socket. Only
the model was faux. A script killed the processes at fixed points with
SIGKILL, restarted them, and checked both stores and the kept transcript.
All 10 cases passed in four runs, the last from the packaged files.

| Case | Shown |
|---|---|
| Normal | One road, one charge, one kept tool result, and the final answer |
| Server killed before its commit | Nothing kept; the same call ran again and laid one road, charged once |
| Server killed after its commit, before replying | The call ran again and got the marked result, with no second charge |
| Worker killed after the result, before pi kept it | pi ran the same task ID again; the server gave the marked result; one tool result was kept |
| Both killed while an approval waited, then allowed | The same wait came back; nothing happened before it was allowed |
| The same, then refused | No road and no charge; a refused tool result |
| Approval taken back after pi kept its intent to run | pi did not run `beforeTool` again; the server refused at the time of the effect |
| Controls | A second road charges again; a duplicate under a new identity is refused; a road over the cap is refused; changed arguments under an old identity are refused; an agent that is not mayor is refused |
| Control: change and marker committed apart | Killing the server between them lost the original result, and the check caught it |
| Control: the tool marked unsafe to replay | pi refused to run it again, and the check caught it |

It showed that a hook in the harness is not enough after a restart; that
the change, its charge and its marker must commit together; and that pi's
task IDs need the runtime's ID beside them.

It did not show:

- Hamlet's behaviour as a whole in Rust, or moving an existing home and
  moving it back.
- A second harness, or two harnesses working together.
- Admitting inputs across a crash, or letters, questions and votes
  recovering.
- Cancelling.
- Charging model calls, or recovering a lost provider answer.
- Authentication, the sandbox, or shell commands and other outside effects.
- Packaging on any platform, other platforms than Linux, or power loss.
- Two servers at once.

Its ten cases run again against this server and the pi harness, as real
processes, in `crates/hamlet-server/tests/harness/`, with admitting
inputs across a crash, cancelling, the handshake's refusals and a real
endpoint beside them.

## Risks

- Two languages and two protocols to keep in step: protocol v0 and the
  harness link.
- Every tool call and every file operation crosses a process boundary.
  The cost of that for file tools is not measured.
- pi-durable is experimental and pinned; an upgrade means a new runtime
  and a compatibility check, and still lands in the pi harness package.
- Recovery now spans two stores. A bug in admission, markers or usage can
  lose work or charge twice, in ways the TypeScript server could not.
- Behaviour drifts while two servers exist. The TypeScript server stays the
  reference, and the acceptance suite and the spec are the checks.
- Markers kept for good grow without bound.
- A migrated home has no `home.sqlite`. Nothing yet stops the TypeScript
  server, run later on that home, from founding a fresh one beside it.
- The server still needs Node and the pi harness package installed.
- `node:sqlite` is marked experimental in Node 24.
- OpenAI's login listens on port 1455, which the Codex CLI uses too; a clash
  fails the login with a clear error.
- Windows has no sandbox, and there is no `--insecure` mode.
- The acceptance suite is only as good as what it drives. Behaviour it
  does not reach can differ between the two servers unnoticed.

## Open questions

- When the server gives up on a runtime that keeps failing.
- Cancelling in the states of recovery not yet tested. Inputs pi answers
  as unanswered are marked dropped.
- What an attempt whose provider answer was lost is charged.
- Whether markers are ever cleared, and when.
- How letters, questions and votes recover when the two agents are in
  different runtimes.
- The format and order of moving a home: whether the pi runtime's store is
  built from `home.sqlite`'s conversations or begins as a copy of it.
- Moving back once both stores have changed since the move.
- What stops the TypeScript server from founding a fresh home on a migrated
  one.
- Whether protocol v0 shows an agent's harness and a harness's
  capabilities, as optional fields.
- Where the protocol's TypeBox source lives once `server/` is deleted.
- Where the acceptance suite and the recorded frames live, and how the
  suite selects the server it starts.
- Whether to import homes of the retained Rust runtime
  ([`core.md`](../core.md#the-retained-runtimes-home)), decided before
  step 7.

## Left for later

Moving a conversation to another harness; a second harness in use; loading
harnesses from outside the repository; one process per agent as a rule;
upgrading a runtime in place without a check; shell commands or outside
effects that happen exactly once; several servers on one home; where the
server is deployed and where tools run then; git on a remote machine, with
a proxy or a short-lived GitHub App token inside it (ADR 0007); several
chiefs on one home; several folders per hamlet; syncing a folder from the
chief's machine; a window that starts its own server.
