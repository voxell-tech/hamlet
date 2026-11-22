# Bevy as it is used here

Which of Bevy 0.20 this project leans on, and the habits that keep a
frame cheap and a change correct. The rules in
[`code_convention.md`](code_convention.md) win where they differ.

Check an API in the crates under `~/.cargo/registry/src/*/bevy_*-0.20.1`
before trusting memory: the engine moves every release.

## What changed in 0.19 that matters here

- A resource is a component on an entity of its own. A broad query
  (`Query<Entity>`, `Query<()>`, `EntityRef`) now meets every resource
  and conflicts with `Res<T>`: filter it, or add `Without<IsResource>`.
- `Assets::get_mut` returns an `AssetMut`, marked modified only when
  written through. Read first, write on a difference.
- `commands.delayed().secs(n)` cannot be cancelled, so keep the entity
  and check it.
- `NextState::set` to the same state runs the enter schedule again;
  `set_if_different` does not.
- `Replace` became `Discard`: `on_replace` hooks are `on_discard`.

## Which tool for which job

| The need | The tool |
|---|---|
| A fact about one thing | A component |
| A fact nothing may edit in place | `#[component(immutable)]`, replaced by inserting it again (server facts in client entities, `docs/tour.md`) |
| One thing points at another, and the other should know | A relationship, with `linked_spawn` where one goes with the other |
| One of a kind for the whole app | A resource |
| Something happened, and whoever cares answers at once | An observer event (`commands.trigger`, `On<E>`) |
| Something happened to one thing | An `EntityEvent` aimed at that entity |
| Something happened, and a system reads it in its turn | A message (`MessageWriter`, `MessageReader`) |
| A component came or went | `On<Add, T>`, `On<Insert, T>`, `On<Discard, T>` |
| A map that must follow a component | `on_insert` and `on_discard` hooks on an `immutable` component (`hamlet_client`'s `Index`) |
| A mode with work on entering and leaving | A state, `OnEnter`, `OnExit` |
| A result from another thread | A channel in a resource, drained by one system gated on it |
| Heavy work off the frame | A task on `AsyncComputeTaskPool`, polled each frame |

An observer runs inside the command that triggered it, and
`World::trigger` does not flush. Code that triggers from `&mut World` and
then reads what the observers queued calls `world.flush()` first.

## A frame costs what its systems touch

The world is drawn only when something asks (`world_view::pace`), so work
that runs every frame also keeps the app from resting.

1. **Gate a system on what it waits for:** `run_if(resource_changed::<T>)`,
   a non-empty channel, `in_state(..)`. A system that begins with
   `if nothing { return; }` still ran, and one that takes `&mut World`
   stopped every other system meanwhile.
2. **Write only on a difference.** Dereferencing a `Mut`, a `ResMut` or an
   `AssetMut` for writing marks it changed, equal value or not, and
   everything gated on that change runs. Compare first, or use
   `set_if_neq`. `iter_mut()` over a map in a resource marks the resource
   changed even if no value is touched.
3. **Do not build a query each call.** `World::query` and its kin match
   every archetype. Take a `Query` in a system; keep a `SystemState` in
   exclusive code; find by id or name in `hamlet_client`'s `Index`.
4. **No file is read in a frame.** A `stat` is the limit.
5. **Share handles.** One material per kind lets the renderer batch.
6. **Ask for a frame only for what is seen.** Something that moves on its
   own writes `RequestRedraw` while it is in view (`ViewVisibility`),
   not while it exists (`flag::keep_waving`).

## Order

Systems in one schedule run in any order unless told. Where one writes
what another reads in the same frame, say so: `.chain()`, `.before(..)`,
`.after(..)`, or a set (`ClientSystems`, `RealmSystems`, `WorldSystems`,
`PanelSystems`, `Cues`, `VfxSystems`).

`ClientSystems::Receive` applies the server's frames in `PreUpdate`, and
`ClientSystems::Send` sends the outbox in `Last`. A task that hands over
work calls the `Wake` resource, which wakes winit's reactive loop.

fynix's sets are private to the vendored crate, so the panels order
themselves around it by schedule. `PanelSystems::Follow` and then `Bind`
run in `PreUpdate`, after `ClientSystems::Receive`, `InputSystems` and
`PickingSystems::Last`, so a server frame, a key and a click reach the
panels' state before fynix mounts in `Update` (`panel_sets`; the test
apps add it too). A system that reads what fynix wrote stays in
`Update`.

Three orders are easy to get wrong:

- A label placed over the world from a `GlobalTransform` must run after
  `TransformSystems::Propagate`, but the layout that reads the `Node` it
  writes runs before propagation, so it is one frame late. Compute the
  place with `TransformHelper` and run before `UiSystems::Layout`
  (`label::Lens`, `label::put`).
- A realm system that runs before the world goes in a step of
  `RealmSystems` (`Steer`, `Aim`, `Act`, `Bridge`), chained before
  `WorldSystems` in `Update`, never next to it with no set.
- A field's text reaches its handler only when the field loses focus:
  Bevy sends `FocusLost` in `PostUpdate`
  (`InputFocusSystems::FocusChangeEvents`) and fynix's observer commits
  the text then. Code that clears the focus and reads the text runs after
  that set (`welcome::run_committed`), not after a count of frames.

## Plugins and panels

A plugin that needs a value takes it from a resource inserted before it,
or is a struct with fields; it does not grow a second parameter.

`bevy_fynix` reads the world through signals inside one exclusive
system. The pattern that fits: an ordinary system with queries and change
detection writes a small `PartialEq` resource only when it differs, and
the view binds to that resource (`hub::Papers`). A signal on
`every_frame()` does its whole read each frame, and a dock tab that is
not in front still runs its signals (see `hamlet_core/src/roll.rs`).

A command about a hamlet goes through `chat::decree`, to its lead; one
about the realm through `chat::command`, to the Hand; `chat::send` is for
what the chief typed in a Hub's field.

## The server

`hamlet_server` uses `bevy_ecs` and `bevy_app` alone, with no frame
([ADR 0015](adr/0015-the-server-runs-as-a-bevy-app-with-no-frame.md)).
What differs from the window:

- **An input is an update, not a frame.** `app/runner.rs` sleeps until an
  input comes, writes it as a message and updates the app once:
  `PreUpdate` reads the input into a message of its kind, `Update` runs
  the system for that kind, `PostUpdate` keeps the input and follows it
  (`app/follow_up.rs`, `FollowUpSystems`), and `Last` sends what it
  queued (`SendSystems`). A set orders systems only in the schedule it is
  configured in. `create_app` in `lib.rs` is the way in.
- **A handler is a system for one kind of input (`add_handler`), and
  never awaits.** What awaits runs on a task (`app/tasks.rs`), whose
  result is an input.
- **An input is one transaction.** `First` begins it; `Core::change` and
  `attempt` make their changes in it; the first step of `PostUpdate`
  keeps it. What an input sets off goes out only after: queue a runtime's
  line with `Runtimes::send`, a client's message with
  `Connections::send`, and write what follows as a message `PostUpdate`
  reads, as `InputEnded` is. A tool run starts in `PostUpdate` from
  `UnstartedToolRun`.
- **The database is the truth.** An entity stands only for what lives in
  the process (a connection, an upload, a tool run, a runtime). A
  decision inside a transaction reads the database, never an entity, and
  never one spawned by the same input: it appears only once its commands
  apply.
- **A system that fails fails its input.** Handlers return Bevy's
  `Result`; `PostUpdate` rolls the input back, drops what it queued and
  tells whoever sent it (`InputOrigin`). A new system takes only the
  resources and queries it reads, not the whole `Core` param.

## Failing

A value from the disk, the network, a model or the keyboard is never
unwrapped. Use `get_entity`, `try_insert` and `try_despawn` on an entity
that may have gone, and `hamlet_realm`'s `adopt` for a child of a parent
that may have gone. A job counted before a channel send must be
uncounted when the send fails.
