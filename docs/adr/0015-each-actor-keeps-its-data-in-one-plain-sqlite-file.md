---
status: accepted
---

# Each actor keeps its data in one plain SQLite file

This supersedes the part of
[ADR 0012](0012-the-realm-is-a-database-of-tables-changed-in-transactions-with-a-jobs-outbox.md)
that keeps the whole realm in one `realm.sqlite`.

Each actor keeps its durable data in one SQLite file of its own,
`actors/<kind>/<key>.sqlite` under the realm folder. The host opens the file
with `rusqlite`, in write-ahead mode with `synchronous=NORMAL`, and in
exclusive locking mode, so no other process opens it while the host runs.
The host locks the realm folder at start, so a second host on the same
folder is refused. The runtime's tables use the prefix `_actor_`, and the
host's row is `_hamlet_actor`. Hamlet owns the versions of both. The actor's
own tables have no prefix. After a clean stop the file is a plain SQLite
database: `sqlite3 -readonly` reads it, and `PRAGMA integrity_check` checks
it.

## Considered options

- One database for every actor. Rejected: every actor's writes would share
  one write lock, an actor's data could not be copied without copying
  everyone's, and a stuck actor could hold up the others.
- A storage engine of the reference project's own, with its database files
  kept inside it. Rejected: it ran as a third process at about 176 MB and
  480 wakeups a second when idle, and it has no Windows build.
- The reference project's replicated storage for copy-on-write forks.
  Rejected: about 135,000 lines. A fork can copy the file instead.
- `synchronous=FULL`. Rejected: write-ahead mode with `NORMAL` keeps every
  committed transaction across a process crash. Only a power loss can lose
  the last commits. The kill kit tests the process crash.

## Consequences

- A change that spans two actors is not one transaction. It is a row of
  steps in the realm, each safe to run again.
- An actor's file can be read with `sqlite3` while the server is stopped.
