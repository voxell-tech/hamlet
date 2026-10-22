# AGENTS.md

## Must read

- [`docs/README.md`](docs/README.md) is the index of the docs: where to
  start and what each page explains.
- [`docs/tour.md`](docs/tour.md) is the map: where everything is and where
  to change what. Start there.
- [Comment convention](docs/comment_convention.md)
- [Code convention](docs/code_convention.md)
- The design is in [`docs/spec/`](docs/spec/). Code refers to its
  requirements by ID (C1, H10, ...). Read it before non-trivial changes.
  Decisions that are hard to reverse are in [`docs/adr/`](docs/adr/), and
  proposals for what is not built in [`docs/proposals/`](docs/proposals/).
- [`CONTEXT.md`](CONTEXT.md) is the glossary: the words for Hamlet's
  concepts, and the ones to avoid.
- [`docs/endless-world.md`](docs/endless-world.md) before touching the
  ground, the sites or `crates/hamlet_realm/src/moving.rs`.
- What is known and deliberately left for later is kept outside this
  repository, in `../hamlet_backlog/README.md` where that folder exists.
  Check it before reporting something as a new problem, and add to it when
  something is deferred. Outside `docs/spec/`, `docs/adr/` and
  `docs/proposals/`, `docs/` is only for what exists: no progress notes,
  no screenshots.

## Running safely

The Rust binaries are `hamlet` in `crates/hamlet_cli`, `hamlet-server` in
`crates/hamlet_server`, and the window `hamlet-app`. Use the Node version
in `harnesses/pi/.nvmrc` and install the harness with
`npm ci --prefix harnesses/pi`. No global npm command is needed.
For local work:

```sh
HAMLET_HOME=/tmp/hamlet-try cargo run -p hamlet_cli -- dev
```

`dev` builds the checkout's server and window. `HAMLET_SERVER_BIN` and
`HAMLET_APP` select existing binaries and skip their respective builds.
Use `cargo run -p hamlet_cli -- <command>` for other commands, or install
all three with `cargo xtask install`. That source install keeps using the
checkout's pi package. Packaged archives include pi beside the binaries
on Linux, or in `Contents/Resources/pi` in the macOS app.
`HAMLET_PI_HARNESS` selects pi's `src/main.ts`; `HAMLET_NODE` selects Node.
`HAMLET_SERVER` is a remote URL, never a server executable path.

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

All four must be clean before a commit. `rust-toolchain.toml` pins the
stable toolchain plain `cargo` runs. Formatting needs a nightly, because
`rustfmt.toml` uses unstable options: the dated one named in `rust-nightly`,
which CI uses too. `cargo xtask check` runs these and the feature-gated and
Node checks below. `cargo xtask fmt` formats Rust and pi.

```sh
cargo +$(cat rust-nightly) fmt
cargo build --all-targets
cargo clippy --all-targets
cargo test
```

The root commands cover every crate. To work on one, add `-p <crate>`, for
example `cargo test -p hamlet_server`.

Offline tests use faux models or loopback fixtures. Do not run live-provider
or remote-infrastructure tests unless the owner explicitly asks.

`harnesses/pi/` is a Node package. Run `npm ci` in it, then these checks
with the Node version in `harnesses/pi/.nvmrc`. They must be clean before
a commit too:

```sh
npm run check   # TypeScript and Biome
npm test
```

The acceptance suite in `crates/hamlet_acceptance` starts real server
processes on scratch homes and drives them through the client protocol, so plain
`cargo test` compiles none of it. It needs the built CLI and server,
and `npm ci --prefix harnesses/pi`. It must be clean before a commit too. It never
contacts a model server or the internet: models are faux, or a loopback
fake of its own. `cargo xtask check` runs it; by hand, at the root:

```sh
cargo build -p hamlet_cli -p hamlet_server
cargo clippy -p hamlet_acceptance --all-targets --features acceptance
cargo test -p hamlet_acceptance --features acceptance
```

The pi harness in `harnesses/pi/` is a Node package of its own, on the
same Node, with the same two checks run in that folder. The server's
tests that start it as a real process, crashes included, are compiled only
with the `harness` feature, so plain `cargo test` needs no Node. They need
`npm ci --prefix harnesses/pi`, never contact a model server or the
internet, and must be clean before a commit too. They build the server
with fault points, so run them after the acceptance suite, which needs the
plain one, as a command of their own. `cargo xtask check` runs them; by
hand, at the root:

```sh
cargo clippy -p hamlet_server --all-targets --features harness
cargo test -p hamlet_server --features harness
```

The runtime protocol, between the server and a harness runtime, is
defined by the Rust types in `hamlet_protocol::runtime`, which a harness
written in Rust uses directly. pi reads its types and checks from a schema
generated from them: `cargo test -p hamlet_protocol` rewrites that schema
when it is stale and fails, so commit the rewritten file.

`HAMLET_CLI` selects a CLI binary outside `target/debug/hamlet` for the
acceptance suite. `HAMLET_ACCEPTANCE_SERVER` selects an explicit server
command; otherwise it tests the default `hamlet server` path.

The client protocol, between the window or CLI and the server, is defined
by the Rust types in `hamlet_protocol::client`, one module per area. Their
`#[schemars(...)]` attributes carry the limits serde cannot check, such as
a request id's UUID format; the server checks every client message against
the schema derived from them. Update `docs/protocol.md` when the protocol
changes.

The client tests replay the faux sessions in `crates/hamlet_protocol/frames/`.
See `docs/protocol.md` for their recording workflow.

On Linux, tools run under bubblewrap rather than Seatbelt. After changing
`crates/hamlet_server/src/sandbox/`, run the real sandbox acceptance tests
on Linux with working bubblewrap, git, curl and Python 3. Linux checks do
not prove macOS Seatbelt behavior. Windows has no server sandbox.
