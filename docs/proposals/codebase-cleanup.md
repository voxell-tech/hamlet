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
fynix is a git dependency pinned to a commit.

## Phase 2: server seams

The one request match, the client schema's fingerprint, `State::apply`
and the split of the serve loop's files are described in
[`tour.md`](../tour.md) and [`protocol.md`](../protocol.md). What
remains:

1. Rules into types. `RequestId` as a UUID newtype, and a non-empty
   string wrapper, so Rust cannot build a message the validator refuses.
   The schema check stays at the edge as ADR 0004 asks.
   `serde_path_to_error` gives the field path in fault words, so the
   schema-walking text builders shrink.
2. Refresh only when something changed. `Store::run` and the chats move a
   revision counter; refresh rebuilds state only when it moved. Rescan the
   tools folder on a two-second timer. Measure the tick before and after
   with a hundred agents.
3. Narrow `Service` in steps. `Service::db()`. The five `expect`
   invariants in `merging/lane.rs` move onto a `Merges::active_mut()`
   method. Stop there unless Phase 5 shows more pain. No rewrite.

Done when: the server runs no SQLite read on an idle tick.

## Phase 3: tests and workspace

1. Grow `hamlet_testing`. `Scratch`, the faux `Script`, the OpenAI
   loopback endpoint, server launch and the override list move there.
   Delete the three other `Scratch` definitions. `hamlet_server` drops its
   dependency on `hamlet_acceptance`.
2. Move tests inline. `tests/record.rs` to `record/tests.rs`,
   `tests/markers.rs` to `operation/tests.rs`, `tests/protocol.rs` into
   `protocol`. `limits`, `sandbox`, `record`, `service`, `protocol` and
   `spend` become `pub(crate)`. The driver binary's body moves into a
   feature-gated module.
3. One testing module per crate. `testing.rs` in `hamlet_ui` and
   `hamlet_realm` holds the helpers that 19 files import from other
   modules' tests. The realm's `bare()` adds the real plugin instead of a
   copy.
4. One gate for Node. `tests/server.rs` splits into a plain server target
   and a crashes target with `required-features`. `oauth.rs` moves to
   acceptance, since it needs no fault points.
5. Move the app. `crates/hamlet_app` holds the binary and the seven
   examples. The root is a pure workspace without `default-members`.
   Update xtask's binary list, `release.yml` and `tour.md`.
6. Bevy features per crate. `default-features = false` on the workspace
   `bevy` entry. `hamlet_assets` and `hamlet_desktop` switch to Bevy's
   sub-crates. The app crate enables the product set.
7. xtask steps CI calls. Steps named `lints`, `test`, `acceptance`,
   `harness`, `doc` and `node`, which CI runs by name. cargo-deny and
   typos in `lints`, `cargo doc` with `-D warnings` in xtask.
8. Fix the sleeps. The two sleeps that wait for an event become `until`
   polling; the four "did not happen" checks first wait for a later event.

Done when: `cargo test -p hamlet_server --lib` needs no Node and no server
process, `cargo test -p hamlet_assets` links no renderer, and the test
tree matches the layout in the convention.

## Phase 4: the Bevy side

1. Order the frame drain. `ClientSystems::Receive` in `PreUpdate` and
   `Send` in `Last`. The drain runs only for a waiting frame or a
   non-empty outbox. Make fynix's system set public upstream and move
   the pinned rev to it. As proof, the 67 settle loops in UI tests
   become single updates.
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
  shrink to two only if Phase 2 step 3 leaves them painful.
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
