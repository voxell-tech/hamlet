# hamlet_acceptance

The acceptance suite for Hamlet's server. Each scenario starts a server
process on a scratch realm and checks its behaviour from outside: through
the client protocol over the authenticated WebSocket, and through the
`hamlet` command. Every message a server sends is parsed into
`ServerMessage`, written back and compared with what arrived, so a field
outside the client protocol fails the scenario that received it.

## Running it

The scenarios compile only with the `acceptance` feature, so plain
`cargo test` neither builds nor runs them. They need the built `hamlet`
and `hamlet_server`, Node from `harnesses/pi/.nvmrc`, and
`npm ci --prefix harnesses/pi`. `cargo xtask acceptance` runs them. By
hand, at the root:

```sh
cargo build -p hamlet_cli -p hamlet_server
cargo test -p hamlet_acceptance --features acceptance
```

They never contact a model server or the internet. Models are faux, or the
loopback endpoint in `hamlet_testing`'s `src/openai.rs`.

| Variable | Selects |
|---|---|
| `HAMLET_ACCEPTANCE_SERVER` | The command that starts a server, split on whitespace. Unset or empty: `hamlet server`. |
| `HAMLET_CLI` | The `hamlet` command. Unset: `target/debug/hamlet`. |

A server that cannot be started fails its scenario. Nothing is skipped.

## Starting a server

The suite runs the command with `--port 0` and `--faux <file>`; scenarios
using the loopback endpoint omit `--faux` and set `HAMLET_MODEL` and
`HAMLET_BASE_URL`. `HAMLET_HOME` is a scratch folder and
`HAMLET_REFLECT_EVERY=0` unless a scenario says otherwise. The server is
ready when `<realm folder>/server.json` names the process the suite
started.

## Faux scripts

`Faux` writes the JSON the server's faux model reads: `agents` (answers by
agent id; the Hand is 1, the first lead 2), `kinds` (by conversation kind)
and `responses` (everyone else). A conversation takes its agent's list
first, then its kind's, then `responses`. An exhausted list answers with
an error and never falls back.

A hamlet's id exists only once it is founded, so a script names it as
`{{name}}` in a step's call arguments, and the step is
`.filled_from(NAMES)`. A faux model costs nothing, so the spend scenarios
use the loopback endpoint, which bills a fixed price per answer.

The scenarios are the files in `tests/`, one per area.

## Recording protocol frames

`tests/frames/` records real server sessions through the client protocol,
on scratch realms with faux responses and a loopback endpoint, never a
live provider. Both wire directions are saved as JSONL, with tokens
redacted.

From the repository root, using the Node version in `harnesses/pi/.nvmrc`:

```sh
npm ci --prefix harnesses/pi
cargo build -p hamlet_cli -p hamlet_server
cargo test -p hamlet_acceptance --features acceptance --test frames -- --ignored
cargo test -p hamlet_protocol -p hamlet_client
```

For a custom Cargo target directory, set `HAMLET_CLI` to the absolute path
of that build's `hamlet`. Recording overwrites
`crates/hamlet_protocol/frames/*.jsonl`; ordinary tests never do. To
inspect new captures first, set `HAMLET_FRAME_DIR` to an absolute scratch
directory for the recorder and the schema package's `npm test`. Review
replacements before keeping them.
