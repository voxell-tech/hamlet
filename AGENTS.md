# AGENTS.md

## Must read

- [`docs/README.md`](docs/README.md) is the index of the docs: where to
  start and what each page explains.
- [`docs/tour.md`](docs/tour.md) is the map: where everything is and where
  to change what. Start there.
- [Comment convention](docs/comment_convention.md)
- [Code convention](docs/code_convention.md)
- [`server/AGENTS.md`](server/AGENTS.md) before working in `server/`.
- The design is in [`docs/spec/`](docs/spec/). Code refers to its
  requirements by ID (C1, H10, ...). Read it before non-trivial changes.
  Decisions that are hard to reverse are in [`docs/adr/`](docs/adr/), and
  proposals for what is not built in [`docs/proposals/`](docs/proposals/).
- [`CONTEXT.md`](CONTEXT.md) is the glossary: the words for Hamlet's
  concepts, and the ones to avoid.
- [`docs/endless-world.md`](docs/endless-world.md) before touching the
  ground, the sites or `crates/hamlet_world/src/moving.rs`.
- What is known and deliberately left for later is kept outside this
  repository, in `../hamlet_backlog/README.md` where that folder exists.
  Check it before reporting something as a new problem, and add to it when
  something is deferred. Outside `docs/spec/`, `docs/adr/` and
  `docs/proposals/`, `docs/` is only for what exists: no progress notes,
  no screenshots.

## Running safely

The Rust CLI is `hamlet` in `crates/hamlet_cli`; the Rust window is
`hamlet-app`. Use the Node version in `server/.nvmrc` and install server
dependencies with `npm ci --prefix server`. No global npm command is needed.
For local work:

```sh
HAMLET_HOME=/tmp/hamlet-try cargo run -p hamlet_cli -- dev
```

`dev` builds the checkout's window before starting the server. `HAMLET_APP`
selects an existing window and skips that build. Use `cargo run -p hamlet_cli
-- <command>` for other commands, or install with `cargo install --path
crates/hamlet_cli`. An installed CLI still needs the server package and Node;
`HAMLET_SERVER_DIR` overrides the package path and `HAMLET_NODE` selects Node.

Bare `hamlet` uses `~/.hamlet` by default and leaves a server running in
the background. Agents run `hamlet` only with `HAMLET_HOME` set to a scratch
folder, and stop background servers with `HAMLET_HOME=<scratch> hamlet stop`.
Run `hamlet-app` only through `hamlet dev`, which sets a scratch `HAMLET_VIEW`,
or with `HAMLET_VIEW` set to a scratch folder yourself. Set both scratch
paths for bare `hamlet` and for `hamlet window`. `HAMLET_APP` can name a
window built outside the repository's `target/` folder.

The server keeps the agents' files; the window keeps its size, layout and
look in `HAMLET_VIEW`, else the OS's config folder for Hamlet. Closing the
window ends only its process; agents keep working on the server. Stop
`hamlet dev` with Ctrl-C when done.

## Checks

All four must be clean before a commit. Formatting needs nightly, because
`rustfmt.toml` uses unstable options.

```sh
cargo +nightly fmt
cargo build --all-targets
cargo clippy --all-targets
cargo test
```

The root commands cover every crate. To work on one, add `-p <crate>`, for
example `cargo test -p hamlet_rules`.

`cargo test` never contacts a model server or the internet. The tests that
do are compiled only with `cargo test -p hamlet_core --features live-tests`;
run them only when the owner asks.

The agent server in `server/` has two checks of its own, run in that folder
with the Node version in `server/.nvmrc`. They must be clean before a
commit too, and `npm test` never contacts a model server or the internet
either.

```sh
npm run check   # TypeScript and Biome
npm test
```

`npm test` builds the Rust CLI before running the server and CLI contract
tests. For direct Vitest runs, build `hamlet_cli` first. `HAMLET_CLI` selects
a CLI binary outside `target/debug/hamlet`.

Protocol v0's Rust types in `crates/hamlet_protocol/src/v0.rs` are
generated, never edited by hand. After changing the protocol, export its
schema with `npm run schema` in `server/`, then regenerate the Rust types
at the root:

```sh
cargo run -p hamlet_protocol_gen
```

Update `docs/protocol.md` when the protocol changes.

The client tests replay the faux server sessions in `server/test/frames/`.
To re-record them offline, run in `server/`:

```sh
HAMLET_RECORD_FRAMES=1 npx vitest --run test/frames.test.ts
```

On Linux, tools run under bubblewrap rather than Seatbelt. After changing
`server/src/sandbox/`, run the tests there too with
`server/scripts/test-linux.sh`, in the OrbStack machine `hamlet-sandbox`
(Debian with Rust, bubblewrap, git, curl, python3 and the Node of
`.nvmrc`).
