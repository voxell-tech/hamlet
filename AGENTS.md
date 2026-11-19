# AGENTS.md

## Must read

- [`docs/README.md`](docs/README.md) indexes the docs.
- [`docs/tour.md`](docs/tour.md) is the map of the code. Start there.
- [Comment convention](docs/comment_convention.md)
- [Code convention](docs/code_convention.md)
- The design is in [`docs/spec/`](docs/spec/). Code refers to its
  requirements by ID (C1, H10, ...). Read it before non-trivial changes.
  Decisions that are hard to reverse are in [`docs/adr/`](docs/adr/).
- [`CONTEXT.md`](CONTEXT.md) is the glossary: Hamlet's words, and the ones
  to avoid.
- [`docs/islands.md`](docs/islands.md) before touching the ground, the
  islands, the sites, the harbor or `crates/hamlet_realm/src/moving.rs`.
- Deferred work is in `../hamlet_backlog/README.md` where that folder
  exists: check it before reporting a new problem, and add to it when you
  defer something. Proposals go in `../hamlet_backlog/proposals/`, never
  in this repository. Outside `docs/spec/` and `docs/adr/`, `docs/` is
  only for what exists: no progress notes, no screenshots.

## Branches

Prefix every branch with the name of the person it is for, as in
`nixon/thinking-effort`.

## Pull requests

- The description says only what changed: no section for what was left
  out or comes next, and none listing checks or tests. Say those in chat.
- Show what is easier seen than read: a diagram of a new flow or
  structure (Typst, compiled with `typst compile diagram.typ
  diagram.svg`, the `.typ` kept outside the repository), or screenshots
  when the window's appearance changes.
- Attach files with `gh` 2.99 or newer; the text after `#` is the alt
  text. Never host an image on a branch, a gist, a release or another
  site. Files are PNG, JPEG, GIF, WebP, SVG, MP4, MOV or WebM, up to
  10 MB.

```sh
gh pr edit 123 --attach './before.png#Before' --attach './after.png#After'
```

## Running safely

- Run `hamlet` only with `HAMLET_HOME` set to a scratch folder, and stop
  background servers with `HAMLET_HOME=<scratch> hamlet stop`. Bare
  `hamlet` uses `~/.hamlet` and leaves a server running.
- Run `hamlet_app` only through `hamlet dev`, or with `HAMLET_VIEW` set to
  a scratch folder. Bare `hamlet` and `hamlet window` need both scratch
  paths.
- Stop `hamlet dev` with Ctrl-C.

```sh
HAMLET_HOME=/tmp/hamlet-try cargo run -p hamlet_cli -- dev
```

- Install the harness with `npm ci --prefix harnesses/pi`, on the Node in
  `harnesses/pi/.nvmrc`. Other commands run as
  `cargo run -p hamlet_cli -- <command>`; env vars are in the
  [README](README.md#environment-variables).

## Checks

All of these must be clean before a commit. `cargo xtask check` runs
nine steps in this order, each also `cargo xtask <step>`, and CI calls
them by name:

1. `format`: rustfmt's check on the workspace and xtask. Needs the latest
   nightly (`rustup update nightly`).
2. `clippy`: `-D warnings` on both.
3. `cross`: clippy on `hamlet_os` and `hamlet_testing` for
   `x86_64-pc-windows-gnu`, with no C compiler.
4. `deny`: `cargo deny check`. Needs `cargo install --locked cargo-deny`.
5. `test`: `cargo build --all-targets`, then `cargo test`, doc tests
   included.
6. `acceptance`: the suite below.
7. `harness`: the server's tests with the `harness` feature, below.
8. `doc`: `cargo doc` with `-D warnings`.
9. `node`: pi's checks, below.

`cargo xtask fmt` formats Rust and pi. Plain `cargo` runs the stable
toolchain `rust-toolchain.toml` names (`rustup update stable` if it is
too old). With `CI` set, the steps pass `--locked`. `vendor/fynix` is a
git submodule that nothing changes: `cargo xtask` fetches it, or run
`git submodule update --init vendor/fynix` once.

```sh
cargo +nightly fmt
cargo build --all-targets
cargo clippy --all-targets
cargo test
```

Add `-p <crate>` to work on one crate. Never run live-provider or
remote-infrastructure tests unless the owner explicitly asks.

pi's checks, after `npm ci` in `harnesses/pi/`:

```sh
npm run check   # TypeScript and Biome
npm test
```

The acceptance suite in `crates/hamlet_acceptance` runs real servers on
scratch realms, so plain `cargo test` compiles none of it. It needs the
built CLI and server and the pi install:

```sh
cargo build -p hamlet_cli -p hamlet_server
cargo clippy -p hamlet_acceptance --all-targets --features acceptance
cargo test -p hamlet_acceptance --features acceptance
```

`HAMLET_CLI` selects a CLI binary outside `target/debug/hamlet`;
`HAMLET_ACCEPTANCE_SERVER` an explicit server command.

The server's `harness` tests start pi and build the server with fault
points, so run them after the acceptance suite:

```sh
cargo clippy -p hamlet_server --all-targets --features harness
cargo test -p hamlet_server --features harness
```

### Schemas and versions

- The runtime protocol is the types in `hamlet_protocol::runtime`; pi
  reads a schema generated from them. When it is stale,
  `cargo test -p hamlet_protocol` rewrites it and fails: commit it.
- The client protocol is the types in `hamlet_protocol::client`, whose
  `#[schemars(...)]` attributes carry the limits serde cannot check; the
  server checks every client message against the derived schema. Update
  `docs/protocol.md` when it changes. When a message's shape or a type's
  name changes, `cargo test -p hamlet_protocol` rewrites
  `crates/hamlet_protocol/client_schema.fingerprint` and fails once:
  commit it.
- Every version stays at 0.1.0 until the first release.

### Sandbox

After changing `crates/hamlet_sandbox/`, run the sandbox acceptance tests
on Linux with working bubblewrap, git, curl and Python 3; they do not
prove macOS Seatbelt. A Windows server runs only with `--insecure`;
Linux and macOS refuse it.
