# Code convention

## The Rust API Guidelines are the base

This convention builds on the
[Rust API Guidelines checklist](https://rust-lang.github.io/api-guidelines/checklist.html)
and says nothing where the two agree. Hamlet departs from it on purpose
in two places:

- The documentation items (C-CRATE-DOC, C-EXAMPLE, C-LINK, C-HIDDEN,
  C-FAILURE) are required only in `hamlet_protocol`, whose doc
  comments become the descriptions in its two schemas: the runtime
  one the pi harness carries, and the client one the server checks
  each message against. The other crates are not libraries anyone
  else builds on, and [the comment convention](comment_convention.md)
  asks for fewer words.
- The publication items (C-METADATA, C-RELNOTES, C-STABLE, C-SEALED)
  do not apply: no crate is published.

## A name says what a thing is, in the glossary's words

- A boolean method reads as a question: `is_`, `has_`, `can_`.
- A type's name does not repeat its module's or its parent's: no
  `LoginStartStart`.
- When a crate holds several types of one kind, each is prefixed by
  its role: `ToolSession`, `WorkspaceSession`.
- An id is a newtype, not a `String` alias.
- A `bool` or `Option` argument that changes what a function does
  becomes an enum.
- No abbreviation that is not a word in [`CONTEXT.md`](../CONTEXT.md):
  `ReplyAddress`, not `Re`; `Meshing`, not `Mb`; `KeeperState`, not
  `KState`.
- Use CONTEXT.md's words, never those on its avoid lists.

## A defect is an `anyhow` error, an expected outcome a typed refusal

Errors come in two layers. A defect, which should never happen, is an
`anyhow::Result` and carries its context up. An expected outcome, such
as a name already taken, is a typed refusal the caller can match on.

An error type derives `Error` and `Display` with `thiserror`. No
`Result<_, String>`: the words a person reads are built from the typed
value at the edge where they are shown.

## Ownership and borrowing come first

`Arc`, `Rc`, `Box<dyn Trait>`, `Mutex`, `RwLock`, `Cell` and `RefCell`
carry a runtime cost: an allocation, a count or a lock. When one is
needed, the PR says what plain ownership or borrowing was tried and why
it did not work.

## Turbofish, not an annotated binding

When a generic call's type needs pinning down, prefer turbofish on
the call itself (`.collect::<Vec<_>>()`, `.parse::<i32>()`, and so on)
over annotating the binding's type to steer inference.

## A rule a lint can check is a lint

Every rule a lint can check is set in the workspace lint block in
`Cargo.toml` and in `clippy.toml`, so clippy enforces it, not a
reader. A lint is silenced with `#[expect(lint, reason = "...")]` at
the narrowest scope.

`unsafe_code` is denied. A module that needs unsafe code says so once,
on the module, with `expect(unsafe_code, reason = "...")`; when the
module builds on every OS but only one OS's build has unsafe code, the
attribute is a `cfg_attr` for that OS. Every unsafe block carries a
`// SAFETY:` comment. Those modules are the CLI's `process.rs` and
Windows console in `terminal.rs`, `hamlet_desktop`'s `app_icon.rs`,
the workspace's `supervise.rs` and `hamlet_os`'s `fs/sys/windows.rs`
and `process/sys/windows.rs`.
The server's system calls go through `hamlet_os`
([ADR 0009](adr/0009-system-calls-that-differ-by-os-go-through-hamlet-os.md)),
and `crates/hamlet_server/clippy.toml` refuses `Command::new`
there. Elsewhere, system calls go through rustix.

## Test our own code, and only what can break

A test covers logic written here. Bevy and the other crates we build
on have their own tests: do not assert that a `clamp` clamps, that a
reflected value survives a save, or that a component is where it was
just inserted.

Skip a test that only restates the line it covers: a `match` written
out again as a table of asserts, a flag set from `is_some()`, a
one-line formula. If the test would change in step with every edit of
the code and catch nothing else, it is not worth having.

Keep tests where a mistake is easy to make and hard to see in review:
maths with a sign or a space to get wrong, a round trip through our
own format, a behaviour a user asked for by name.

Beyond those, test only what the system depends on: the sandbox and
its limits, grants, data integrity, wire contracts, lifecycle,
dependency resolution and the core rules. Do not test a single small
function, a constant, wording, layout, or anything a feature request
would routinely change.

When it's a toss-up whether a test earns its place, leave it out.
Remove a test that no longer earns its place when you find it, and a
test that only checks another crate's behaviour.

## A test sits beside what it tests

Unit tests are inline in their module. Past roughly 300 lines of
tests, they move to `tests.rs` in the module's folder
(`record/tests.rs`), declared with `#[cfg(test)] mod tests;`. Helpers
shared by tests live in the crate's one `testing` module, never
imported from another module's tests.

`tests/` holds only tests that use the crate through its public API or
its executable. Scenarios of the whole product go in `hamlet_acceptance`,
except fault-point scenarios, which stay in `hamlet_server` behind
`required-features`. Fixtures shared by crates live in `hamlet_testing`.
A test suite is never a dependency of another crate.

## Colours are parameters of a shader, not constants in it

A shader takes its colours as uniforms, set from Rust. This holds
above all for colours that are a matter of taste or of the theme: the
palette, the accents, anything a setting might one day change. A
`const` colour in WGSL can't be animated, themed or changed at run
time, and it hides from anyone reading the theme or the palette.

The values live where the other colours live (`assets/theme.toml` for
the panels, `assets/palette.toml` for the world) and reach the shader through
the material. Numbers that are part of the shader's own arithmetic,
such as a falloff or a threshold, may stay in the shader.

## Plugins are added, not called

A module's plugin is a `fn plugin(app: &mut App)` or a `Plugin` struct, and
either way it is added with `app.add_plugins((.., flag::plugin, ..))`. Do
not call it as a function (`flag::plugin(app)`): added, it is named in the
app's list of plugins and cannot be added twice by mistake.

## The plugin comes first

A module's plugin comes before its resources, components, systems and
other structs, enums and functions, so what the module adds to the app is
read before how it works. Only constants, statics and type aliases may
stand above it, under the `use` and `mod` lines. A `Plugin` struct is
followed by its impls, with nothing else between them.

## A tuning value is written once, in its file under `assets/`

A size, a speed, a timing or a colour of the world goes in the file for
its subject under `assets/`, and its field goes in that subject's settings
struct with a doc comment saying what it is. The struct's `Default` reads
the file (`hamlet_assets::Tuning`), so do not write the value again in
code, in a `Default` or anywhere else.

Shaders are the exception to "assets are files under `assets/`": a shader
stays beside the code that binds it and is embedded from there.

## No mod.rs

Use the filename instead of `mod.rs` in a folder.

## Moved code is imported from where it now lives

When code moves to another module or crate, every `use` and every
qualified path that names it changes to its new home, in the code, the
tests and the examples. Do not add a `pub use` that keeps the old paths
working: a move is permanent, and a re-export left to spare the edits
hides where things live. A re-export is for a deliberate public API
only.
