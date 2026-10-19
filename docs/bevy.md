# Bevy as it is used here

What Bevy 0.19 offers, which of it this project leans on, and the habits
that keep a frame cheap and a change correct. It was written from the
0.19 release notes, the 0.18 to 0.19 migration guide, the Bevy sources and
a review of every crate that touches the engine (2026-10-10). The rules in
[`code_convention.md`](code_convention.md) still win where they differ.

Sources: <https://bevy.org/news/bevy-0-19/>,
<https://bevy.org/learn/migration-guides/0-18-to-0-19/>, and the crates
under `~/.cargo/registry/src/*/bevy_*-0.19.1`. Check an API there before
trusting memory: the engine moves every release.

## What 0.19 changed that matters here

| Change | What it means in this code |
|---|---|
| A resource is a component on an entity of its own | A broad query (`Query<Entity>`, `Query<()>`, `EntityRef`) now meets every resource and conflicts with `Res<T>`. Filter it, or add `Without<IsResource>`. Do not derive `Resource` and `Component` on one type |
| `Assets::get_mut` returns an `AssetMut` | The asset is marked modified only when it is written through. Read first, write on a difference |
| Observers take `.run_if(..)` | A condition on an observer replaces an early return at its top |
| `commands.delayed().secs(n)` | Work for later without a timer component. It cannot be cancelled, so keep the entity and check it |
| `SystemState::get` returns a `Result` | As does a custom `SystemParam::get_param`. `Single` and `Populated` fail there, and the system is skipped |
| `NextState::set` to the same state runs the enter schedule again | The window relies on it to show itself twice. `set_if_neq` is the one that does not |
| `Replace` became `Discard` | `on_replace` hooks are `on_discard` |
| Text is laid out by Parley | `TextFont::font` is a `FontSource`, `font_size` a `FontSize` |
| The render graph is systems | A custom pass is a system in `Core3d`, ordered against the engine's own |
| `bsn!` scenes | Not used here. The panels are built by `bevy_fynix`, and the world's meshes are made in code |

## Which tool for which job

| The need | The tool | Here |
|---|---|---|
| A fact about one thing | A component | `Citizen`, `Busy`, `Housed` |
| A fact nothing may edit in place | `#[component(immutable)]`, replaced by inserting it again | Everything the record decides (`docs/core.md`) |
| One thing points at another, and the other should know | A relationship | `LivesIn` and `Citizens`, `TalksTo`, `RaisedBy`, `About` |
| A link kept as a bare `Entity` and found by scanning | Make it a relationship, with `linked_spawn` where one goes with the other | `Sign` and `Signed` |
| One of a kind for the whole app | A resource | `Society`, `Stage`, `Selected` |
| Something happened, and whoever cares answers at once | An observer event (`commands.trigger`, `On<E>`) | `UserMessage`, `SessionOutput`, `Carried` |
| Something happened, and a system reads it in its turn | A message (`MessageWriter`, `MessageReader`) | `Played`, `Cue`, `SetDown` |
| A component came or went | `On<Add, T>`, `On<Remove, T>` | Leaving and returning are `Left` |
| A mode with work on entering and leaving | A state, `OnEnter`, `OnExit`, a computed state | `Layout`, `Showing`, `DayPart`, `Dark` |
| A result from another thread | A channel in a resource, drained by one system | `Inbox`, `Reviews`, the model tests |
| Heavy work off the frame | A task on `AsyncComputeTaskPool`, polled each frame | The mesh builds in `generator/building.rs` |

An observer runs inside the command that triggered it, and `World::trigger`
does not flush. Code that triggers from `&mut World` and then reads what the
observers queued calls `world.flush()` first, as `session::drain` does after
each message.

## A frame costs what its systems touch

The world is drawn only when something asks (`Animating`, which
the realm's view sets and the window's `pace` reads), so work
that runs every frame is also work that keeps the app from resting.

1. **Gate a system on what it waits for.** `run_if(resource_changed::<T>)`,
   a condition on a channel being non-empty, `in_state(..)`. A system that
   begins with `if nothing { return; }` still ran, and if it takes
   `&mut World` it stopped every other system for that. `session::drain`
   and `review::hear` run only when their channel holds something.
2. **Write only on a difference.** Dereferencing a `Mut`, a `ResMut` or an
   `AssetMut` for writing marks it changed, equal value or not. Everything
   gated on that change then runs: transform propagation, the layout of a
   `Node`, the preparing of a material and of every mesh that wears it.
   Compare first, or use `set_if_neq`. `light_lanterns` once rewrote five
   shared materials a frame and had every lantern prepared again.
3. **A `ResMut` taken is not yet a change, a `&mut` through it is.**
   `iter_mut()` over a map in a resource marks the resource changed even if
   no value is touched, and then `resource_changed` gates nothing.
4. **Do not build a query each call.** `World::query`, `query_filtered` and
   `try_query` make a `QueryState` and match every archetype. In a system
   take a `Query`. In exclusive code that runs often keep a
   `SystemState`, or run a cached system.
5. **No file is read in a frame.** A `stat` is the limit (`agents::watch`).
   Reading and parsing belong to the worker that already has the answer, or
   to a component written when the file's time moved (`Seen`).
6. **Look for the loop inside the loop.** A closure that calls
   `query.iter().any(..)` inside `for x in &query` is quadratic and reads
   like one line. Index by the entity or by the id the generator already
   keeps (`Drawn`).
7. **Share handles.** One material and colours in the vertices let the
   renderer batch a town's houses. A material per entity does not batch
   and is prepared one by one. The flags of one length wave with one
   material (`flag::Waving`).
8. **Ask for a frame only for what is seen.** Something that moves on its
   own writes `RequestRedraw` while it is in view (`ViewVisibility`, read
   after `VisibilitySystems::CheckVisibility`), not while it exists.
   `flag::keep_waving` does.

## Order

Systems in one schedule run in any order unless told. Where one writes
what another reads in the same frame, say so: `.chain()`, `.before(..)`,
`.after(..)`, or a set (`WorldSystems`, `Step`, `Cues`, `VfxSystems`).

Two orders are easy to get wrong:

- A label placed over the world from a `GlobalTransform` must run after
  `TransformSystems::Propagate`, but the layout that reads the `Node` it
  writes ran before propagation. It is one frame late. Compute the place
  with `TransformHelper` and run before `UiSystems::Layout`. `label::Lens`
  is that for the world's camera and whatever a label points at, and
  `label::put` moves a label only when its place differs, and
  `label::above` is the corner of one that hangs over a point.
- A system added with no set next to a chain that is `.before(WorldSystems)`
  may run on either side of it. The camera's steering is a set,
  `pick::Steering`, and the bridge's `aim` runs after it.

## Plugins

A plugin is added, never called (`code_convention.md`). A plugin that needs
a value takes it from a resource inserted before it, or is a struct with
fields; it does not grow a second parameter. A plugin registers systems,
observers and resources and does little else in `build`.

## The panels

`bevy_fynix` builds a view with `&mut World` and reads the world through
signals, all inside one exclusive system. So most `world.resource..` calls
in the window and its plugins are the toolkit's shape, not a choice. What is a choice is
where a signal reads from. The pattern that fits:

> An ordinary system with queries and change detection writes a small
> `PartialEq` resource only when it differs, and the view binds to that
> resource.

`Lines`, `Bar`, `Card`, `Offered`, `Roll`, `plugin_needs`'s `Queue`,
`inspector::Papers` and the realm's `Hold` and `RobotPaint` are built this
way. A signal on `every_frame()` does
its whole read each frame, and a dock tab that is not in front still runs
its signals. A read that has no resource of its own takes its check from
`moved::<R>()` or `either(..)` in `hamlet_sdk/src/checks.rs`, or from
`rolled` in `hamlet_agents_api/src/roll.rs`.

A command about an agent or a hamlet goes through `chat::decree`, which
gives it to that hamlet whoever the chat is with. `chat::send` is for what
the chief typed and for what is about the open chat or the whole realm.

## Failing

A value from the disk, the network, a model or the keyboard is never
unwrapped. `get` an entity that may have gone (`get_entity`,
`try_insert`, `try_despawn`), and `world::adopt` a child whose parent may
have. A send on a channel can fail when the other end has gone: a job
counted before the send must be uncounted when it does.
