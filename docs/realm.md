# The realm

The server keeps one realm, `~/.hamlet` unless `HAMLET_HOME` or
`hamlet_server --home` selects another folder. Use a fresh scratch folder
for development. `Realm` is in `crates/hamlet_server/src/realm.rs` and
`Layout`, the paths below, in `realm/layout.rs`.

| Path under the realm | Owner and contents |
|---|---|
| `realm.sqlite` and its WAL files | `hamlet_store`'s `Store`: everything the realm knows |
| `realm.id` | The identity also recorded in `realm.sqlite` |
| `harnesses/<instance>/` | That runtime's own store |
| `credentials.json` | Installation ID, connection token and provider credentials |
| `server.json` | Running server's URL, PID, start time and confinement, never its token |
| `profiles/<id>.git/` | Each profile's repository, the Hand's included |
| `hand/` | The Hand's worktree |
| `hamlets/<hamlet id>/<agent id>/` | An agent's worktree: `memory/`, `skills/` and `inbox/` |
| `workers/<id>/` | A worker's scratch folder while it runs |
| `library.git/`, `library/` | The library's repository, and its `main` checked out, read-only to agent tools |
| `tools/` | Hand-added tool manifests and scripts, read at server start and again when they change |
| `mcp/` | `servers.json`, the MCP servers' list, read at server start and again when it changes, and each server's own folder; outside agents' reach |
| `terminals/<id>.json` | Each terminal's view and last output, outside agents' reach |
| `images/`, `files/` | What chiefs sent, kept once under the hash of its bytes, out of agents' reach |
| `dreams/` | The checkout of a profile's `main` a dream writes, out of agents' reach |

A new realm is made with its first chief and its Hand, and no hamlet.
The server's chief is named by `HAMLET_USER`, else `USER`, `USERNAME` or
`LOGNAME`, else "chief".

## Claim and storage

`Realm::open` takes a local socket claim on the folder's canonical path
(`realm/claim.rs`), so a second server cannot open the same realm under
another spelling.

`Store::open` refuses a database whose SQLite application ID is not its
own or whose `realm.version` is not compatible
([ADR 0013](adr/0013-every-protocol-and-the-realm-database-are-semver-at-0-1-0.md)).
The schema has no upgrades. A missing or mismatched `realm.id` or
database is an error, never a reason to make a fresh one. Restore a
complete matching backup: the database, `realm.id` and `profiles/` belong
together.

Every change is one transaction, taken with the write lock from its
start. A tool call is identified by its runtime and key, a client request
by its chief and id; its marker is committed with the change and its
charge, so a call asked for again gets the first result. A write of a
profile's repository, a shell command or an edit is marked begun before
it runs and, cut off by a crash, is answered as interrupted, never run
twice. The files and repositories a change needs are made afterwards by
jobs, kept in the same transaction and run again after a crash until done
([ADR 0012](adr/0012-the-realm-is-a-database-of-tables-changed-in-transactions-with-a-jobs-outbox.md)).

A request or a runtime's line whose handling fails, a panic included,
has its transaction rolled back and is answered with a `fault` to the
client or an `error` to the runtime. The server goes on.

## Profiles and worktrees

A profile's repository holds `CHARACTER.md`, `memory/` and `skills/`;
`main` is what the profile knows, `agents/<agent id>` is an agent's
branch
([ADR 0011](adr/0011-profiles-are-git-repositories-agents-are-branches-worktrees-are-checkouts.md)).
The server alone writes a repository, through `hamlet_git`. A worktree is
the checkout of an agent's branch without `CHARACTER.md`, plus `inbox/`,
which is never committed. After a turn that changed a worktree a `commit`
job commits it, and when the agent is idle and `main` has moved a
`take-in` job merges `main` into the branch, so a turn never sees `main`
change under it (`knowledge/run.rs`).

## Credentials and documents

`credentials.json` is mode 0600 and replaced whole via a temporary file
and rename. Provider attempts receive credentials over the runtime
protocol in memory. Text is scrubbed of secrets before pi keeps it, in
tool output and client messages, and in a failure's words before they are
logged. `HAMLET_API_KEY` is an in-memory override, not a file update.

Claude's login stays with Claude's own program, which pi runs; the
server keeps only a mark of the sign-in. A rotated credential is saved
before it is acknowledged. Retired credentials and transient sign-in
answers stay in memory for scrubbing, never on disk.

The chief's documents (charter, settings, catalogue overrides, limits)
are rows of the `document` table. Characters and memory are files; the
server opens them through directories it holds open, without following
links.

`HAMLET_MODEL`, `HAMLET_BASE_URL`, `HAMLET_REFLECT_EVERY` and
`HAMLET_NET` override stored settings for this process; an agent's or
hamlet's own model choice still stands over `HAMLET_MODEL`.

## Starting and stopping

`hamlet_server --home <folder> --port 0` publishes a free loopback
address, and `server.json` only after the realm is open and started
(`app/startup.rs`). `--faux` selects an offline response script;
`HAMLET_PI_HARNESS` selects the harness entry.

SIGTERM or the first SIGINT closes clients with code 1001, ends every
sign-in, waits up to 15 s for a credential rotation begun, stops the
runtimes and removes `server.json` (`app/shutdown.rs`). A second SIGINT
exits 130. Closing a client or window does not stop the server.

The server logs on stderr; `RUST_LOG` selects what. When it cannot
serve, its last line is `Error: ` and why, in plain text.
