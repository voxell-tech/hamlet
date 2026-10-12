# AGENTS.md

Instructions for anyone (human or agent) working on this codebase.

## Must-read docs

- [`docs/README.md`](docs/README.md) is the index of the docs: where to
  start and what each page explains.
- [`docs/tour.md`](docs/tour.md) is the map: where everything is and where
  to change what. Start there.
- The design is kept outside this repository, in `../hamlet_spec/` where
  that folder exists: `SPEC.md` for the core and `world/HAMLET.md` for the
  hamlet and its world. Code refers to their requirements by ID (C1, H10,
  ...). Read them before non-trivial changes.
- [`docs/endless-world.md`](docs/endless-world.md) is how the land goes on
  with no edge, how a town is founded wherever there is room, and how a
  town is moved. Read it before touching the ground, the sites or
  `crates/hamlet_world/src/moving.rs`.
- What is known and deliberately left for later is kept outside this
  repository, in `../hamlet_backlog/README.md` where that folder exists.
  Check it before reporting something as a new problem, and add to it when
  something is deferred. Proposals for what is not built live beside it.
  `docs/` is only for what exists: no progress notes, no screenshots.
- [`docs/code_convention.md`](docs/code_convention.md) and
  [`docs/comment_convention.md`](docs/comment_convention.md) are the rules
  for code and comments. Where they differ from the summary below, they win.

## Checks

All four must be clean before a commit. Formatting needs nightly, because
`rustfmt.toml` uses unstable options.

```sh
cargo +nightly fmt
cargo build --all-targets
cargo clippy --all-targets
cargo test
```

The repository is a Cargo workspace; the root commands cover every crate.
To work on one, add `-p <crate>`, for example `cargo test -p hamlet_rules`.
The crates and what each holds are in [`docs/tour.md`](docs/tour.md).

`cargo test` never contacts a model server or the internet. The two tests
that do are compiled only with `cargo test --features live-tests`; run them
only when the owner asks.

Never run the `hamlet` binary without `HAMLET_HOME` set to a scratch folder:
with the default home it works on the owner's real notes, skills and logs.

## Comment convention

Comments should be concise: a sentence or a fragment, not a paragraph
unless the thing being explained genuinely needs it. Write one only when
the code doesn't already say what it's doing, or on a `pub` item (function,
struct, field) whose meaning a caller can't get from its name and
signature alone. Don't restate what the code already makes obvious.

Don't use banner-style separator comments:

```rust
// -----------------------------
// Section
// -----------------------------
```

or

```rust
// ========= Section ============
```

Prefer a plain `///` doc comment on the item that follows, or nothing at
all if the module is small enough that structure doesn't need announcing.

Don't use em dashes in comments either. Use a period, comma, or
parentheses instead.
