# The home

The Rust server keeps one home, `~/.hamlet` unless `HAMLET_HOME` or
`hamlet-server --home` selects another. Use a fresh scratch home for
development. `crates/hamlet_server/src/home.rs` defines `Home` and `Layout`.

| Path under the home | Owner and contents |
|---|---|
| `realm.sqlite`, its WAL and shared-memory files | Rust `Store`: records, chief/society documents, bindings, pending inputs, oversight, spend and operation markers |
| `realm.id` | The identity also recorded in `realm.sqlite` |
| `harnesses/<instance>/` | That runtime's own store: conversations, checkpoints, model responses and input admission |
| `credentials.json` | Server-owned installation ID, connection token and provider credentials |
| `models.json` | Server-owned fetched model listings; chief catalogue overrides live in `chief_document` |
| `server.json` | Running server's URL, PID and start time, never its token |
| `server.log` | Detached server's output, written by the CLI's process launch |
| `houses/<hamlet>/<number>/` | Agent files: `NOTES.md`, `skills/`, `inbox/` |
| `library/` | Shared skills, read-only to agent tools |
| `librarian/NOTES.md` | Instructions included in librarian consultations |
| `tools/` | Hand-added tool manifests and scripts |
| `.gifts/`, `.society-files/`, `.library-staging/` | Private staging and recoverable file snapshots, outside agents' reach |

The first hamlet is `home`, its founder agent 0, named `founder`.
`mayor` addresses the current office. Characters are server records;
authoritative transcripts belong to the harness, not house files.

## Claim and storage

`claim.rs` takes local socket claims for both the supplied and canonical
home paths. A second server cannot open the same home under another
spelling. The claim outlives the open store and its harness processes.

`Store::open` checks SQLite's application ID and schema version and applies
its ordered internal migrations. `Home::open` checks the `realm.id` and
database identity pair. A missing or mismatched part is an error, not a
reason to make a fresh database. Restore a complete matching backup.

An experimental TypeScript `home.sqlite` is refused even beside a Rust
database. Those homes were never deployed; there is no import or rollback
tool. Choose a new `HAMLET_HOME` for the Rust server. This does not remove
the Rust store's own versioned schema upgrades.

`Store::run` keeps record changes and their result marker in one
transaction. Calls are identified by runtime and key; tool admission also
checks the bound conversation and request digest. File effects have their
own staging/recovery paths. An interrupted shell command is not replayed
as if it were a transactional record change.

## Credentials and documents

`credentials.json` is mode 0600 and replaced whole via a temporary file and
rename. It holds `installationId`, `connection: {token}` and provider
credentials. Provider attempts receive needed credentials over the private
link in memory. Pi asks the server to scrub text before keeping provider
snapshots or final answers; tool output and client messages are scrubbed too.
`HAMLET_API_KEY` is an in-memory override, not a file update.

ChatGPT and OpenRouter support subscription sign-in. ChatGPT credentials
are separate from OpenAI API keys. The server serializes credential
refreshes per provider and saves a rotation before acknowledging it.
An already-started refresh can finish after its client disconnects;
logout or a newer sign-in invalidates its result. Retired credentials and
transient sign-in answers remain in memory for scrubbing, not on disk.

`chief_document` holds the charter, settings and work-folder choices. A
hamlet's limits live only in its record, so a limits edit is one record
change. Character belongs to the recorded agent. House notes are opened
relative to the retained home descriptor without following links.
`HAMLET_MODEL`, `HAMLET_BASE_URL`, `HAMLET_REFLECT_EVERY` and `HAMLET_NET`
override stored settings for this process.

Settings selects the workspace for the next start. The active connection
and its token stay fixed until restart, even after Settings changes.
`HAMLET_WORKSPACE_URL` and `HAMLET_WORKSPACE_TOKEN` override that selection
for this process. Remote houses live at the workspace manager; records,
credentials, transcripts and the shared library remain under this home.
Moving houses copies the living agents' files to the selected target;
restart selects it. See [remote workspaces](sandbox.md#remote-workspaces).

## Starting and stopping

`hamlet-server --home <folder> --port 0` publishes a free loopback address.
`runtime::serve` owns authentication, ordered connections and shutdown;
`Service` composes domain owners. `--faux` selects an offline response script,
not a harness entry. `HAMLET_PI_HARNESS` selects that entry instead.

SIGTERM or the first SIGINT closes clients with code 1001, settles shutdown
work and removes `server.json`. A second SIGINT exits 130. Closing a client
or window does not stop the server.

## Starter skills

`crates/hamlet_server/assets/starters/` supplies the embedded skills through
`src/starters.rs`; `assets/charter.md` is the default charter. Library
initialization installs starters once, preserving later chief changes.
The chief can also give an agent a starter through the protocol.
