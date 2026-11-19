# Code convention

## The Rust API Guidelines are the base

This convention builds on the
[Rust API Guidelines checklist](https://rust-lang.github.io/api-guidelines/checklist.html)
and says nothing where the two agree. The documentation items (C-CRATE-DOC,
C-EXAMPLE, C-LINK, C-HIDDEN, C-FAILURE) are required only in
`hamlet_protocol`, whose doc comments become its schemas; the publication
items (C-METADATA, C-RELNOTES, C-STABLE, C-SEALED) do not apply, as no crate
is published. Comments follow [the comment convention](comment_convention.md).

## A name says what a thing is, in the glossary's words

- A boolean method reads as a question: `is_`, `has_`, `can_`.
- A type's name does not repeat its module's or its parent's: no
  `LoginStartStart`.
- When a crate holds several types of one kind, each is prefixed by its role:
  `ToolSession`, `WorkspaceSession`.
- An id is a newtype, not a `String` alias.
- A `bool` or `Option` argument that changes what a function does becomes an
  enum.
- No abbreviation that is not a word in [`CONTEXT.md`](../CONTEXT.md), and
  never its avoid-list words: `KeeperState`, not `KState`.

## A defect is an `anyhow` error, an expected outcome a typed rejection

A defect, which should never happen, is an `anyhow::Result`; an expected
outcome, such as a name already taken, is a typed rejection the caller can
match on. An error type derives `Error` with `thiserror`; no
`Result<_, String>`.

## Ownership and borrowing come first

`Arc`, `Rc`, `Box<dyn Trait>`, `Mutex`, `RwLock`, `Cell` and `RefCell` carry a
runtime cost; when one is needed, the PR says what plain ownership or
borrowing was tried and why it did not work.

## Turbofish, not an annotated binding

Pin a generic call's type on the call (`.collect::<Vec<_>>()`), not on the
binding.

## A rule a lint can check is a lint

Every such rule is set in the workspace lint block in `Cargo.toml` and in
`clippy.toml`, and a lint is silenced with `#[expect(lint, reason = "...")]`
at the narrowest scope.

`unsafe_code` is denied. A module that needs it says so once, on the module,
with `expect(unsafe_code, reason = "...")` (a `cfg_attr` when only one OS's
build has unsafe code), and every unsafe block carries a `// SAFETY:` comment.

The server's system calls go through `hamlet_os`
([ADR 0009](adr/0009-system-calls-that-differ-by-os-go-through-hamlet-os.md)),
and `crates/hamlet_server/clippy.toml` refuses `Command::new` there.
Elsewhere, system calls go through rustix.

## Test our own code, and only what can break

A test covers logic written here where a mistake is hard to see in review, or
what the system depends on: the sandbox, grants, data integrity, wire
contracts, lifecycle and core rules. Do not assert that a `clamp` clamps,
restate a line as a table of asserts, or test a constant, wording or layout.
When it is a toss-up, leave it out.

## A test sits beside what it tests

Unit tests are inline in their module, and move to `tests.rs` in the
module's folder past roughly 300 lines. Helpers shared by tests live in the
crate's one `testing` module. `tests/` holds only tests through the public
API or the executable. Scenarios of the whole product go in
`hamlet_acceptance`, except fault-point scenarios, which stay in
`hamlet_server` behind `required-features`. Fixtures shared by crates live in
`hamlet_testing`.

## Other rules

- A shader takes its colours as uniforms set from Rust, from where the other
  colours live (`assets/themes/`, `assets/palette.toml`).
- A module's plugin is added with `app.add_plugins((.., flag::plugin, ..))`,
  never called as `flag::plugin(app)`, and comes before the module's other
  items.
- A size, speed, timing or colour of the world is written once, in the file
  for its subject under `assets/`, and read by its settings struct's `Default`
  (`hamlet_assets::Tuning`), never again in code.
- No `mod.rs`: use the filename.
- When code moves, every `use` that names it changes to its new home; no
  `pub use` keeps the old path alive.
