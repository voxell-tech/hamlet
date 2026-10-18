# The WASM host

An installed plugin is a WASM component. `hamlet_wasm` loads the ones the
chief installed, runs each in a store of its own under limits, calls it
once a frame with what happened and draws the panels it declares. A plugin
never sees Bevy or the world. The built-in plugins of [`sdk.md`](sdk.md)
are Rust in the binary and do not pass through it.

The host is behind the `wasm` feature of the binary:
`cargo run --features wasm`. Without it the engine is not compiled.

## The crates

| Where | What it is |
|---|---|
| `crates/hamlet_wasm` | The host. The engine is wasmtime, pinned in the root `Cargo.toml`, and no other crate names a wasmtime type |
| `crates/hamlet_wasm/wit/hamlet-plugin.wit` | The interface, the one description of what a plugin imports and exports |
| `crates/hamlet_guest` | What a plugin is written with: the bindings, the `Plugin` trait, the `ui` tree builder and the `export!` macro |
| `guests/hello` | A plugin that logs on every tick. A workspace of its own, since it is built for `wasm32-wasip2` |
| `guests/roster` | A plugin with a panel that lists the agents, behind the grant `read.agents` |
| `assets/wasm.toml` | The limits |

In `hamlet_wasm/src/`, a file for each part:

| Part | Files |
|---|---|
| The engine and what it runs | `engine.rs` (the engine and its ticker), `limits.rs`, `instance.rs`, `call.rs`, `fault.rs`, `interface.rs` (the generated host side of the WIT) |
| What a plugin may call | `host.rs` (the linker and the store's state), `host/declare.rs` (declaring panels), `host/read.rs` (the reads), `host/request.rs` (`request` and what a call wants). `Granted` and `AgentList` are in `hamlet_sdk::capability` and `agents` |
| The install folder | `manifest.rs`, `install.rs` |
| Bringing up and the frame | `load.rs`, `admit.rs` (a plugin the chief just let in), `start.rs`, `enable.rs`, `inbox.rs`, `reply.rs` (an answer to a request), `run.rs`, `stop.rs` |
| The panels | `panel.rs`, `wire.rs` (a returned tree made into nodes), `draw.rs`, `event.rs` |

`lib.rs` has the plugin. The node model, the read-model of what each panel
shows and the view built from it are in `hamlet_sdk::tree`.

## The interface

`wit/hamlet-plugin.wit` is the interface. The host imports are:

- `log(level, message)`.
- `panel(id, title, icon)`, which declares a panel. Only `entry` declares.
  `icon` is the name of a `Glyph` of `hamlet_sdk::icons`, in any case.
- `agents() -> result<list<agent>, string>`, which needs the grant
  `read.agents`.
- `request(capability, reason) -> request-result`, which asks to use a
  capability the manifest names: `granted`, `denied` or `pending` (see
  [`permissions.md`](permissions.md)).

The plugin exports:

- `entry()`, which runs once when the plugin is brought up.
- `step(inbox) -> outgoing`, which runs for each frame in which the plugin's
  `Inbox` holds something. An `incoming` is a `tick`, a `click`, a `toggled`,
  a `text-changed`, or the answer to a `request` that was `pending`:
  `granted(capability)` or `denied(capability)`. The intents it returns are
  carried out by the host: a `log-line`, and a `redraw` of a panel.
- `panel-view(panel) -> view`, which returns the panel as it is now: a
  `tree`, or `lang`, a program in Hamlet's panel language that the host
  draws (see [`panels.md`](panels.md)).

`interface::VERSION` is the package version. A manifest that names another
one is turned down.

## The tree of a panel

A `tree` is a list of `node`s, each followed by its children and theirs, the
root first. A node is an optional `id`, a `widget` and the number of its
children. The interface cannot hold a node in a node, so the list is flat.

| Widget | What it is drawn as |
|---|---|
| `column`, `row`, `scroll` | A stack of its children. A row wraps; a scroll fills the height left |
| `label(text)` | Body text |
| `heading(text)` | A heading over a part of the panel, in the display face |
| `note(text)` | Small dim words |
| `icon(name)` | An icon of the family, by name. An unknown name gets `Menu` |
| `button(label)` | A brass button |
| `checkbox(label, checked)` | A tick and its label |
| `text-field(text)` | A well sunk into the panel |
| `divider` | A hairline |

A `button`, `checkbox` and `text-field` need an id, which names them in
events. Two siblings may not share an id. A tree with a node over the limit
of `tree_nodes`, nested deeper than `tree_depth`, or against these rules
fails the call, and the plugin is failed.

The ids make the redraw cheap. A node is matched with the node of the last
tree that has its id among the same parent's children, or its place if it
has none. A matched node keeps its view and only the props that differ are
written, so a field being typed in keeps its text and focus, and an animation
under way goes on.

## How a panel is drawn

1. `entry` calls `panel`. The host spawns a `Panel` for each declaration,
   with `Entry` and `ContributedBy` of `hamlet_sdk::plugin`, so the panels
   go with the plugin. The panel's id in the dock is `<plugin id>/<id>`, so
   that a plugin cannot take the id of another panel.
2. `Dirty` on the plugin holds the panels that are due: all of them after
   `entry`, a panel an event was sent to after the `step` that handled it,
   and a panel named by a `redraw` intent. The `Draw` set calls `panel-view`
   once for each and keeps the tree in `PluginTrees`, a resource keyed by
   the panel's id in the dock, only when it differs.
3. The panel's view is `hamlet_sdk::tree::view::panel`, built from its id (`Body::ById` in
   `hamlet_sdk::dock`). It reads `PluginTrees` through signals: the shape of
   the panel (nothing yet, a tree, stopped), and for each node its text, its
   state and its children, found by the ids from the root. Each container
   keeps its children with `each`, keyed by the ids, so the views are made
   from `hamlet_sdk::looks`, `icons` and `theme` as the native panels are.

## Events

A press on a button, a press on a checkbox and a text committed with Enter
or by leaving the field send an `Interaction` event with the panel's id in the dock, the
node's id and what was done. An observer puts it in the `Inbox` of the
running plugin that owns the panel, as a `click`, `toggled` or
`text-changed` that names the panel and node by the ids the plugin gave them.
The next `step` gets it, and the panel is drawn again after it.

A checkbox does not toggle itself: it shows `checked` of the tree, so a
plugin that ignores a `toggled` leaves the box as it was.

## The frame

A plugin is an entity with `PluginInfo` (`builtin: false`), the SDK's
`Manifest`, `Status`, `Source` (the file of its component), `Granted`,
`Instance` and `Inbox`, and `Dirty`. `Instance` holds the store: a wasmtime
`Store` is `Send` and `Sync` when its data is, so it is a component and
needs no resource of its own.

The `Update` sets `PluginSet::{Collect, Step, Apply, Draw}` run in that
order.

1. `Collect` puts a `tick` in the inbox of every running plugin.
2. `Step` runs only while some inbox holds something. It calls `step` once
   for each running plugin that has mail. A call that fails fails the plugin.
3. `Apply` runs when a plugin was stepped and carries out its intents: a
   `log-line` is written to the app's log, labelled with the plugin's id, and
   a `redraw` marks the panel due.
4. `Draw` runs when a panel is due and calls `panel-view`.

What a plugin passes to `log` is written to the log the same way, labelled
with the id. A headless run (`--headless`) installs the log only with the
`wasm` feature on.

## A plugin that stopped

A call that traps, runs out of time or memory, or returns a bad tree marks
the plugin `Status::Failed` and puts `Failure`, the reason, on it. Its
panels stay in the dock, and each shows "This plugin stopped", the reason
and a RESTART button, in place of the last tree. RESTART sends `Enable`.

`Enable` of an installed plugin that is disabled or failed brings it up
again from its file with a new store, runs `entry` again and spawns its
panels again. `Disable` despawns the panels, as for a built-in plugin.

## Grants

The capabilities, the chief's choices and the verdicts are in
[`permissions.md`](permissions.md). A host function that needs a capability
checks the plugin's `Granted` component on each call. A call without it
returns the error `read.agents not granted` and is logged with the plugin's
id. If the manifest names the capability and the chief has not decided, the
call also notes the capability as wanted, and the host raises the petition
after the call (`run::want`). `Granted` is replaced by inserting it again,
and the next call sees it.

The host puts what a function answers from into the plugin's store before a
call: the grant, and a copy of the agents only when `read.agents` is held.
The agents are `AgentList`, a resource that a system writes when the core's
agents change, so a host function never touches the world. `agents()` returns
a copy: the name of each living agent and the name of its hamlet.

### The install record

`plugins/grants.toml` records which plugins are installed and what the chief
chose for each (see [`permissions.md`](permissions.md)). A plugin that is
scanned and not recorded as installed is never compiled.

## Limits

The values are in `assets/wasm.toml`, and `Limits` in `limits.rs` says what
each means. They apply to every plugin alike, whatever it is granted.

- **Time.** The engine has epoch interruption, and a thread advances the
  epoch every `tick_ms`. A call, and the instantiation, may last `call_ms`
  worth of ticks; past it the call traps as `Fault::OutOfTime`.
- **Memory and tables.** A `ResourceLimiter` refuses a growth past
  `memory_mib` or `table_elements`, which traps the call as
  `Fault::OutOfMemory`. The pooling allocator is sized to the same memory.
- **Log lines.** A call keeps at most `log_lines` lines of `log_bytes` each.
- **Panels and trees.** A plugin declares at most `panels` panels, and a
  tree holds at most `tree_nodes` nodes nested `tree_depth` deep.
- **Text.** One text of a node (its words or its id) is at most `text_bytes`,
  and all the text of a tree, or the program a panel returns, at most
  `tree_text_bytes`. A tree over either fails the call like any bad tree: it
  is dropped and the plugin is marked failed.
- **Events.** At most `events_per_second` events from the panels are given to
  a plugin in a second. The rest are dropped, each with a warning in the log.
- **Intents.** At most `intents_per_call` intents of one `step` are carried
  out. The rest are dropped, with one warning for the call.

A saved change to the file reaches `Limits` while the app runs, and the next
call takes the new `call_ms`. The engine keeps the tick and the pool it was
built with, and a plugin keeps the memory, table and log limits it was loaded
with.

## What a plugin can import

The linker defines the `hamlet:plugin/host` interface and nothing else. A
plugin written in Rust with `std` imports the `wasi:` interfaces of its
standard library; each of those is defined as a function that traps when
called, so such a plugin loads, and one that prints or reads the clock is
failed. Any import that is neither `hamlet:plugin/host` nor `wasi:` fails
the load with the name of the import.

## Installing a plugin

```
<home>/plugins/
  grants.toml          which plugins are installed, and the chief's choices
  roster/
    manifest.toml
    plugin.wasm
```

`manifest.toml` has `id`, `name`, `version` and `interface`, and may have
`needs` (an `id` and a `range` each), `exports`, `imports` and `asks` (a
`capability`, a `reason` and optionally a `scope` each). It may also have
`kind`: `wasm` (the default) or `data`, a plugin that is a `panel.ui` and no
`plugin.wasm`, with an optional `icon` and no `interface` (see
[`panels.md`](panels.md)). The scan turns a folder down when a file is
missing, the manifest cannot be read, its `id` is not the folder's name, a
WASM plugin's `interface` is not `interface::VERSION` or an ask cannot be
granted (a name that is not a capability, one that is never granted, or no
reason); the reason is logged.

`grants.toml` records the plugins that may load. A plugin that is in the
folder and not recorded as installed is never compiled, and a missing file
installs nothing.

## Building a plugin

A plugin is a `cdylib` for `wasm32-wasip2` that depends on `hamlet_guest`:

```rust
use hamlet_guest::ui::{Node, button, column, label};
use hamlet_guest::{Incoming, Intent, Plugin};

struct Hello;

impl Plugin for Hello {
    fn entry() {
        hamlet_guest::panel("main", "HELLO", "book");
    }

    fn step(inbox: Vec<Incoming>) -> Vec<Intent> {
        Vec::new()
    }

    fn panel_view(_panel: &str) -> Node {
        column([label("Hello"), button("Go").id("go")])
    }
}

hamlet_guest::export!(Hello);
```

`panel_view` has a default that returns an empty column, for a plugin with no
panel. A plugin holds its state in statics of its own, since each call is a
call into the same instance.

`guests/roster` is a plugin built with all of it: a panel with a heading, a
Refresh button, a count of refreshes, a checkbox that shows a line of help,
and the agents from `agents()`, or a line that the read was not granted.

To build it into a home and run the app, which asks the chief to install it
(Allow gives it `read.agents`):

```sh
rustup target add wasm32-wasip2
HAMLET_HOME=/tmp/hamlet-try guests/roster/build.sh
HAMLET_HOME=/tmp/hamlet-try cargo run --features wasm
```

The default `cargo build`, `clippy` and `test` need neither the target nor
the network.

## Tests, the gallery and the bench

The tests make their plugins from core modules in text and the WIT file, so
they need no target. `fixture.rs` makes the modules, `fixture/tree.rs` lays a
tree out in a module's memory, and `fixture/home.rs` makes a scratch home and
an app that loads it.

`cargo run --example ui_gallery -- --scene plugin-roster
--out <dir>` draws the roster's panel in the dock, from the tree the roster
returns and a list of agents made up for the scene.

`cargo run --release -p hamlet_wasm --example bench -- <plugin.wasm>` times
the compile, the `.cwasm`, an instantiation and a `step` of a built plugin.
