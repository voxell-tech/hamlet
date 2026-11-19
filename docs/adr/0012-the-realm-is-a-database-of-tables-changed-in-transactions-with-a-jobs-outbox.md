---
status: accepted
---

# The realm is a database of tables, changed in transactions, with a jobs outbox

This replaces [ADR 0005](0005-the-home-is-the-servers-store-the-harnesses-stores-and-files.md).

A realm keeps its structure in one SQLite database, `realm.sqlite`, of
`STRICT` tables with keys, foreign keys and checks, one table for each
thing (see `schema.sql`). A JSON column holds only an opaque payload: a
marker's outcome, a transcript item, a decision's detail, a chief's
settings. What a profile knows is in git (ADR 0011), and each harness
runtime keeps its conversations in a store of its own that only it opens.
Rows of profiles, agents, conversations, tasks and waits are never
deleted, so their ids are never given twice.

**Every change is one transaction.** A change names its actor (a chief, an
agent or the realm), is checked in one place against who may do what, makes
its rows, keeps a decision in the record when the spec says so, and notes
what it touched. Only after the commit does the server send those entities
to the clients watching them. A refused change leaves nothing.

**Markers make a request happen once.** A tool call is kept by its runtime
and call key, a client request by its chief and id, with the digest of what
was asked, in the same transaction as the change and its charge. Asked
again, the call gets the result the marker holds; asked again with
different arguments, it is refused (S19). Markers are kept for good. A
model attempt is admitted once and its usage charged once (S20) by its key
in the same way.

**Jobs are an outbox.** What must happen to files or repositories after a
commit is a row written in the same transaction: `plant` (make a profile's
repository), `worktree`, `commit`, `take-in` (of `main`), `erase-agent`,
`erase-profile`, `erase-hamlet`, `merge`, `split`, `shelve` (a skill) and
`dream-end`. A thread of its own, on a connection of its own, runs the jobs
after each commit and every job not done at start. Each job is safe to run
twice, and a key keeps two of one effect from waiting at once.

The database has one schema and no upgrades: a database of a version not
compatible with the server's is refused (ADR 0013). A folder with neither
`realm.sqlite` nor `realm.id` becomes a new realm; a database missing
beside its `realm.id`, or one of another realm, is an error, never
replaced by an empty one (S25).

## Considered options

- Entities as JSON documents. Rejected: nothing checks a key, a reference
  or a state, and a change spanning two could not be one transaction.
- An event log with projections. Rejected: every read would need a
  projection kept in step, and clients are sent entities, not events.
- Run the effect inside the transaction. Rejected: a repository or folder
  cannot roll back, git work is slow, and it would hold the write lock.
- Do the effect after the commit with no record. Rejected: a crash between
  the two loses it.
- A queue outside the database. Rejected: a change and its job could not
  commit together.
- One pi-durable storage, JSONL files, or one database per hamlet. Rejected
  for the reasons ADR 0005 gave.

## Consequences

- A realm cannot be read with a text editor; inspecting it needs the server
  or SQLite.
- A realm is claimed by one server at a time, and the harnesses it runs
  open their stores only while it holds the claim (S2).
- A crash leaves either the change and its job, or neither; the job is done
  once more at the next start.
- Every tool that changes a record is safe to run twice, and goes through
  the one path that commits the change with its marker.
- Queries go through Diesel (ADR 0010).
