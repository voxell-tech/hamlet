# Server assets

`limits.schema.json` is a JSON Schema of the chief's limits. The server
does not read it: the limits are the Rust types in
`crates/hamlet_protocol/src/client/limits.rs`.

`charter.md`, `hand.md` and `starters/` are embedded in the server
binary by `hamlet_actors`. `crates/hamlet_actors/src/library.rs`
commits the starters to a new library's `main`.

The client protocol's JSON Schema is derived from the Rust types in
`hamlet_protocol` by `src/clients/check.rs`, not read from here.
