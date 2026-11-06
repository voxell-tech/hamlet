# The realm

The server keeps one realm, `~/.hamlet` unless `HAMLET_HOME` or
`hamlet_server --home` selects another folder for it. Use a fresh scratch
folder for development. `crates/hamlet_server/src/realm.rs` defines `Realm`
and `layout.rs` defines `Layout`.

| Path under the realm | Owner and contents |
|---|---|
| `realm.sqlite`, its WAL and shared-memory files | `hamlet_store`'s `Store`: who exists and answers to whom, roads, the record, tasks and spend, conversations and their inputs, waits and grants, the chief's documents, markers and jobs |
| `realm.id` | The identity also recorded in `realm.sqlite` |
| `harnesses/<instance>/` | That runtime's own store: conversations, checkpoints, model responses and input admission |
| `credentials.json` | Server-owned installation ID, connection token and provider credentials |
| `models.json` | Server-owned fetched model listings; the chief's catalogue overrides live in the database |
| `server.json` | Running server's URL, PID, start time and confinement, never its token |
| `server.log` | Detached server's output, written by the CLI's process launch |
| `scholars/<id>.git/` | Each scholar's repository, the Hand's included |
| `hand/` | The Hand's lodge |
| `hamlets/<hamlet>/<number>/` | A hire's lodge: `memory/`, `skills/` and `inbox/`, a checkout of its branch |
| `workers/<id>/` | A worker's scratch folder while it runs |
| `library.git/`, `library/` | The library's repository, and its `main` checked out, read-only to agent tools |
| `tools/` | Hand-added tool manifests and scripts |
| `staging/`, `dreams/` | Uploads in parts, and the checkout of a scholar's `main` a dream writes, out of agents' reach |

A new realm is made with its first chief and its Hand. It has no hamlet: a
chief founds the first with `/found`, or asks the Hand to. The library is
made with the starter skills as its first commit.

The server reads `tools/` once, when it starts, so a tool a chief puts there
by hand reaches the agents at the next start.

## Claim and storage

`Realm::open` resolves the folder to its canonical path, and `claim.rs` takes
a local socket claim on that path, so a second server cannot open the same
realm under another spelling. The claim outlives the open store and its
harness processes.

`Store::create` makes the schema in a new `realm.sqlite`, and `Store::open`
refuses an existing one whose SQLite application ID is not its own or whose
`realm.version` is not compatible with the schema's
([ADR 0013](adr/0013-every-protocol-and-the-realm-database-are-semver-at-0-1-0.md)).
The schema has one version and no upgrades. `Realm::open` checks the
`realm.id` and database identity pair. A missing or mismatched part is an
error, not a reason to make a fresh database. Restore a complete matching
backup: the database, `realm.id` and `scholars/` belong together.

Every change is one transaction, taken with the write lock from its start;
the connection runs with WAL, full sync and foreign keys. A tool call is
identified by its runtime and key, a client request by its chief and id; its
marker is committed with the change and its charge, so a call asked for again
gets the first result. A society call is safe to ask again for that reason:
its answer is kept in its change's transaction, or what it began (a letter, a
worker, an errand) is found again by its call. A write of a scholar's
repository, a shell command or an edit is marked begun before it runs and,
cut off by a crash, is answered as interrupted, never run twice. The files
and repositories a change needs are made afterwards by jobs, kept in the
same transaction and run again after a crash until they are done
([ADR 0012](adr/0012-the-realm-is-a-database-of-tables-changed-in-transactions-with-a-jobs-outbox.md)).

At start, after the claim and the checks, every job not done runs again, a
turn that ended before the stop and was not followed yet is followed, the
waits that were open wait again, and the runtimes are started and their
conversations carry on.

A client request or a runtime's line whose handling fails, a panic included,
leaves its transaction rolled back and is answered with the failure: a
`fault` to the client, an `error` to the runtime, which ends that turn with
the server's words. The server goes on.

## Scholars and lodges

A scholar's repository holds `CHARACTER.md`, `memory/` and `skills/`; `main`
is what the scholar knows, `hires/<agent id>` is a hire's branch, and a tag is
a version
([ADR 0011](adr/0011-scholars-are-git-repositories-hires-are-branches-lodges-are-checkouts.md)).
The server alone writes a repository, through `hamlet_git`. A lodge is the
checkout of a hire's branch without `CHARACTER.md`, plus `inbox/`, which is
never committed. After a turn that changed a lodge a `commit` job commits it
to the branch, and when the hire is idle and `main` has moved a `take-in` job
merges `main` into the branch and writes the merged files into the lodge, so
a turn never sees `main` change under it. `knowledge.rs` names the branch and
finds a lodge; `knowledge/run.rs` runs the jobs. The character is read from the
branch's tip for the prompt, and edited on a scholar's `main` through the
protocol.

## Credentials and documents

`credentials.json` is mode 0600 and replaced whole via a temporary file and
rename. It holds `installationId`, `connection: {token}` and provider
credentials. Provider attempts receive needed credentials over the runtime
protocol in memory. Pi asks the server to scrub text before keeping provider
snapshots or final answers; tool output and client messages are scrubbed too.
`HAMLET_API_KEY` is an in-memory override for the settings' model's
endpoint, not a file update.

ChatGPT and OpenRouter support subscription sign-in. ChatGPT credentials
are separate from OpenAI API keys. The server serializes credential
refreshes per provider and saves a rotation before acknowledging it.
An already-started refresh can finish after its client disconnects;
logout or a newer sign-in invalidates its result. Retired credentials and
transient sign-in answers remain in memory for scrubbing, not on disk.

The database holds the chief's documents as rows of the `document` table: the
charter, the settings, the model catalogue's overrides, the librarian's
instructions and the realm's own limits. A hamlet's limits are a column of its
row, so a limits edit is one change. A character is a file of its scholar's
`main`, and memory is files of a lodge or of `main`; the server opens them
through directories it holds open, without following links.
`HAMLET_MODEL`, `HAMLET_BASE_URL`, `HAMLET_REFLECT_EVERY` and `HAMLET_NET`
override stored settings for this process. `HAMLET_MODEL` is the settings'
model, so an agent's or hamlet's own choice still stands over it.
`HAMLET_BASE_URL` is the address of that model's endpoint.

The settings also hold when dreams run: whether they are on, the local hours
they start in, and the most one may cost.

## Starting and stopping

`hamlet_server --home <folder> --port 0` publishes a free loopback address.
`serve::serve` owns the ordered start and shutdown, `clients/connection.rs`
the sockets, and `clients/requests.rs` authentication and who answers each
request; `Core` composes the domain owners. `--faux` selects an offline
response script, not a harness entry. `HAMLET_PI_HARNESS` selects that entry
instead.

SIGTERM or the first SIGINT closes clients with code 1001, settles shutdown
work and removes `server.json`. A second SIGINT exits 130. Closing a client
or window does not stop the server.

## Starter skills

`crates/hamlet_server/assets/starters/` supplies the embedded skills, built
into the server by `knowledge/library.rs`; `assets/charter.md` is the default
charter and `assets/hand.md` the Hand's first character. The library's first
commit holds the starters once; a skill taken out or changed later stays so.
