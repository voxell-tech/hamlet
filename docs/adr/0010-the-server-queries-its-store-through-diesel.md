---
status: accepted
---

# The server queries its store through Diesel

The server reaches `realm.sqlite` through Diesel's SQLite backend, on
the one connection `Store` holds. Each query is written in Diesel's
query builder against `schema.rs`, which `cargo test -p hamlet_server`
generates from the store's schema and rewrites when it is stale. The
schema stays the SQL beside each module, created in one transaction for
a new store, and a store of another version is still refused.

## Considered options

- rusqlite with SQL in strings. No column or type is checked until the
  statement runs, and a renamed column is found by a failing test.
- Diesel's migrations. They keep a table of their own and upgrade an
  older store, where the store has one version and refuses the others.
- SQLx. Its checked queries need a database or a cache of every query
  at build time, and its API is async where the server's store is not.

## Consequences

- A query that names a missing column, compares unlike types or reads
  a row into the wrong shape does not compile.
- What the query builder cannot say stays SQL in a `diesel::dsl::sql`
  fragment: SQLite's JSON functions and `date`.
- SQLite is still bundled, now through `libsqlite3-sys`.
- Every query takes `&mut SqliteConnection`, so a read needs mutable
  access to the store.
- A store path must be UTF-8, as Diesel opens a database by URL.
- Closing the store cannot return an error: Diesel closes the
  connection when it drops, and panics if SQLite refuses.
