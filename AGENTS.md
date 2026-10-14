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
  ground, the sites or `crates/hamlet_world/src/moving.rs`.
- What is known and deliberately left for later is kept outside this
  repository, in `../hamlet_backlog/README.md` where that folder exists.
  Check it before reporting something as a new problem, and add to it when
  something is deferred. Outside `docs/spec/`, `docs/adr/` and
  `docs/proposals/`, `docs/` is only for what exists: no progress notes,
  no screenshots.

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
do are compiled only with `cargo test --features live-tests`; run them only
when the owner asks.

Never run the `hamlet` binary without `HAMLET_HOME` set to a scratch folder:
with the default home it works on the owner's real notes, skills and logs.
