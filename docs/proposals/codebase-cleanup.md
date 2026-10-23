# Codebase cleanup

A plan to bring the code in line with
[the code convention](../code_convention.md), in six phases: the rules,
safety and CI, the server's seams, tests and workspace, the Bevy side,
and naming and typing. It keeps
[ADR 0004](../adr/0004-the-window-is-a-client-of-the-servers-state.md):
the server keeps checking each client message against the schema derived
from the protocol types; this plan only moves the rules into the types.

## Decisions

- Line width and rustfmt: width 70 and the nightly options in
  `rustfmt.toml` stay. No reformat commit.
- Errors: error types derive `Error` and `Display` with `thiserror`.
  `Refusal` gains both, with variants for the recurring cases; the 32
  helpers returning `Result<_, String>` become small enums.
- Smart pointers: ownership and borrowing come first; a smart pointer is
  justified in its PR.
- Module files: no `mod.rs` stays.
- App crate: the window package moves from the root to
  `crates/hamlet_app`, and the root becomes a pure workspace.
- Protocol version: a fingerprint test of the client schema fails CI
  until the protocol version is bumped. The schema check of ADR 0004
  stays.

## Phase 0: the rules

Built: the convention is [`code_convention.md`](../code_convention.md),
its lints are the workspace block in `Cargo.toml` and `clippy.toml`, and
what it left out is in the backlog.

## Phase 1: safety and CI

Built: a failed request or pi line ends only itself
([`tour.md`](../tour.md), `serve.rs`, `requests.rs` and `harness.rs`),
the one override list is `hamlet_testing::OVERRIDES`, the `faults`
feature refuses a release build, CI runs the acceptance and harness
steps apart on three operating systems with pinned toolchains, and
fynix is the vendored submodule `vendor/fynix`, a path dependency
pinned to one commit, which xtask fetches and CI checks out.

## Phase 2: server seams

Built: one match names each request's owner, the client schema has a
fingerprint, and a client applies changes with `State::apply`.
`RequestId` holds only a hyphenated UUID, so Rust cannot build a request
id the schema refuses; the schema check stays at the edge. An idle tick
runs no SQLite read: it rebuilds the state only when it may have
changed, as `Since` in `runtime.rs` defines, and the tools folder is
read every two seconds. `Service::db()` is the home's database, and
`Merges::active_mut()` holds the lane's invariant
([`tour.md`](../tour.md), [`protocol.md`](../protocol.md)). Left: the
non-empty string wrapper; `serde_path_to_error`, which finds no path
through the internally tagged `ClientMessage`; and the idle tick's read
of every living house's skills from disk, about 1.27 ms with 101 houses.

## Phase 3: tests and workspace

Built: the fixtures crates share are in `hamlet_testing`: `Scratch`,
the faux `Script`, the loopback OpenAI endpoint, a server on a scratch
home and the override list, and no crate depends on
`hamlet_acceptance`. The server's record, marker and protocol tests sit
beside their modules, its modules are crate-private, and the harness
driver's body is a module behind the `harness` feature. The helpers
several modules' tests share are in one `testing.rs` in
`hamlet_server`, `hamlet_ui` and `hamlet_realm`, and the realm's
`bare()` adds the real `hamlet::plugin`. `tests/server.rs` needs no
Node; a served home's tests are the `crashes` and `harness` targets,
behind the `harness` feature; the OAuth scenarios are in acceptance.
The root is a pure workspace, and `crates/hamlet_app` holds the window
and its seven examples and turns on the product's Bevy features;
`hamlet_assets` and `hamlet_desktop` use Bevy's parts, so
`cargo test -p hamlet_assets` builds no renderer. `cargo xtask check`
is six steps, `lints`, `test`, `acceptance`, `harness`, `doc` and
`node`, which CI runs by name, with cargo deny and typos in `lints`.
The sleeps that waited for an event poll for it
([`tour.md`](../tour.md), [`AGENTS.md`](../../AGENTS.md)). Left: the
three sleeps for the tools folder's rescan, which no client sees, and
the four that check something did not happen where no later event
exists; the resources `bare()` still inserts by hand; and the
generator's own `testing` module, which reaches its private items.

## Phase 4: the Bevy side

1. Order the frame drain. `ClientSystems::Receive` in `PreUpdate` and
   `Send` in `Last`. The drain runs only for a waiting frame or a
   non-empty outbox. fynix's system set is private and the vendored
   fynix does not change, so Hamlet orders itself around fynix by
   schedule, never by naming its systems. As proof, the 67 settle loops
   in UI tests become single updates.
2. Wake on frames. The connect task gets a waker that pokes the reactive
   event loop after each frame. This is the one place a boxed closure is
   needed, justified in its PR.
3. Move gift uploads. The folder walk and file reads leave `orders.rs`
   for `hamlet_client`, run under `spawn_blocking` and use rustix.
   `hamlet_realm` drops libc. `orders.rs` keeps deeds and moves, at about
   300 lines.
4. Public sets in realm and ui. `RealmSystems` with `Steer`, `Aim`, `Act`
   and `Bridge` chained before `WorldSystems`, and `PanelSystems` with
   `Follow` and `Bind` before fynix mounts. The ten
   `.before(WorldSystems)` edges and the `.after(pick::aim)` edges become
   `in_set`. The three aim systems are private again.
5. Index and relationships. An `Index` resource, kept by component hooks,
   for agents, hamlets, waits, chats and roads replaces every per-lookup
   query. A road becomes two relationships with linked spawn;
   `prune_roads` is deleted.
6. Gate the exclusive systems. `run_if` on `settle`, `models::follow` and
   `run_committed`. `chats::watch` gives way to add and remove observers.
7. Time, not frames. `PATIENCE` becomes a `Duration` on real time. The
   three-frame counters become ordering after the receive set.
8. Small fixes. Tints as a resource updated when the palette changes; the
   flag settings cloned from the resource rather than parsed again;
   `EntityEvent` for the events aimed at a chat or a wait; named
   `SystemParam` structs for the three fifteen-parameter systems; the
   three shadowing names renamed; overview's duplicate registration
   deleted; one `scene::plugin` shared by the realm and the examples.
   `docs/bevy.md` cites systems that exist.

Done when: a server frame reaches the realm, the roll and fynix in the
same update, and no UI test needs more than one update to settle.

## Phase 5: naming and typing

1. Glossary renames. `Settlement` to `HamletId`, `Citizen` to
   `AgentNumber`, `Mail` to `Letter`, leader to mayor, the four limits
   names to two, the four `Session` types qualified by role, `Backend`
   to `Workspace`.
2. Typed values. Society tools return a typed outcome, and `once`
   serializes it at the marker. The two `.expect` calls on marker JSON go.
   The request ids the serve loop, the gifts and the merge lane still
   carry as `String` become `RequestId`, so `reply` and `fault` take the
   type and lose their fallback for an id that is not one.
3. Refusals. `Error` and `Display` on `protocol::Refusal` through
   `thiserror`, variants for the recurring cases, and the 32
   `Result<_, String>` helpers turned into small enums.
4. Names. `is_` and `has_` on boolean methods; `Re`, `Mb` and `KState`
   spelled out; the five stutter names fixed, with a schema regeneration
   and a pi update; `ref_` and `move_` renamed.
5. Lint cleanup. The remaining expects reviewed, the Bevy crates' prints
   routed through the log, the server given one diagnostic function. The
   realm's raw `openat` and the CLI's `kill` switched to rustix, then
   `unsafe_code` denied, with module-level expects.

Done when: the lint block passes with no expects outside the listed
unsafe modules, and no type carries a word from a CONTEXT.md avoid list.

## Deliberately not in the plan

- No grep check for glossary words. Review catches drift; a grep cannot
  tell the senses of "user" or "workspace" apart.
- No `PluginGroup`, no prelude modules, no `missing_docs` outside
  `hamlet_protocol`. Two consumers do not justify them.
- No `Service` rewrite and no trait for local versus remote. Enum
  dispatch is right for two closed variants. The five parallel enums
  shrink to two only if Phase 5 finds them painful.
- No change to the schema check beyond moving rules into types, because
  ADR 0004 chose it.
- No reformat: width 70 and the nightly options stay.

## Effort and order

| Phase | Effort | Order |
|---|---|---|
| 0 Rules | Half a week | Follows the decisions above |
| 1 Safety and CI | One week | After 0 |
| 2 Server seams | Two to three weeks | After 1 |
| 3 Tests and workspace | Two weeks | After 1; beside 2 on its own branch, since they touch different crates |
| 4 Bevy side | Two to three weeks | After 3, for the feature changes |
| 5 Naming and typing | Two weeks | Interleaved: file by file inside the other phases' PRs, not one sweep |
