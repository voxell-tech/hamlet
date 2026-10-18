# The SDK and the plugins

A plugin is a unit of function added to the window: today a panel in the
dock. `hamlet_sdk` is what a plugin is built with, `plugins/` holds the
plugins, and `hamlet_plugins` lists which are built in.

```
hamlet (binary) -> hamlet_plugins -> plugins/<name> -> hamlet_sdk -> engine crates
```

A plugin crate depends on `hamlet_sdk` and the engine crates, never on
`hamlet_ui` or on another plugin.

## hamlet_sdk

| Module | What it holds |
|---|---|
| `theme`, `looks`, `icons`, `platform` | How the panels look: the theme, the buttons and fields, the icons, the typefaces and the keyboard focus |
| `feel` | `Mood` and `Lit`, the marks a view puts on a node for the feel in `hamlet_ui` to read |
| `cards` | The card of a list row and the heading over a part of a panel |
| `convo` | `Me`, the chat's session and the agent it is with. `AgentSelected`, whether the world has an agent selected. `of_agent`, a page about the agent the chat is with |
| `papers` | `Papers`, a read-model of the selected agent's notes, character and skills. Empty while no agent is selected |
| `confirm` | `Confirm`, an observer event that asks the chief a question before a command is run |
| `dock` | `Panel`, `add_panel`, `panel` and `lit_panel`, `show_tab` and `TabShown` |
| `capability` | `Capability`, the names a plugin can ask for and `NEVER`, the ones no plugin can; `Asks`, what a manifest asks for; `Choice` and `Choices`, what the chief chose; `Granted`, the verdict for each capability the manifest names. See [`permissions.md`](permissions.md) |
| `agents` | `AgentList`, a read-model of the living agents and their hamlets that a system keeps, and `SelectedAgent`, the name of the agent selected in the world |
| `events` | The catalogue of the host's events a plugin may subscribe to: `selection-changed` and `agents-changed`, each behind `read.agents`. See [`wasm.md`](wasm.md) |
| `tree` | The tree of a plugin's panel: `Node`, the `PluginTrees` read-model of what each panel shows, the view built from it with the looks of the panels, and the `Interaction` event of a click or typing in it |
| `plugin` | The plugin types below |

`Cx`, `AnyView`, `View` and `with_cx` are the view types of the window.
`SdkPlugin` is the plugin to add before any other: it keeps fynix's
`DockRegistry` in step with the `Panel` entities and reads the papers.

### A panel

A `Panel` is an entity with an id, a title, an icon and a `Body`: a
`fn() -> AnyView`, or a `fn(&str) -> AnyView` that is given the panel's id
(`Panel::by_id`), for panels that share one function. An observer on `Add`
puts its window in the `DockRegistry`, and one on `Remove` takes it out of
the registry and out of the dock. A panel that no plugin owns is added with
`app.add_panel(Panel::new(..))`.

### The plugin types

`HamletPlugin` is the one entry point:

```rust
pub trait HamletPlugin {
    fn manifest() -> Manifest;
    fn entry(entry: &mut Entry);
}
```

- `Manifest`: id, name, version, the plugins it needs and wants (an id and a
  version range each, read by `Range`), what it exports and imports (`Exports`
  and `Imports`: commands, events and collections, see [`wasm.md`](wasm.md))
  and whether it is built in. `Manifest::built_in("id")` starts one.
- `Entry` is what `entry` declares into. It reaches neither the app nor the
  world. It declares a panel, with `panel` or with `panel_by_id`, and a
  command, with `command`. A new kind of contribution is a variant of
  `Declared`, a method of `Entry` and an arm of `Entry::spawn`, all in
  `plugin/entry.rs`. The WASM host makes an `Entry` of what an installed
  plugin declared and spawns it.
- `Command` is an entity with an id and a title, `ContributedBy` the plugin
  that exports it. It is the command other plugins run through the host. Menu
  entries, keys and the rest will point at it.
- `app.add_hamlet_plugin::<P>()` spawns the plugin's entity with its
  `PluginInfo` (id, name, version, built in), its `Manifest`, its
  `EntryPoint` and its `Status`, then runs `entry`.
- `Status` is `Starting`, `Running`, `Disabled` or `Failed`, replaced by
  inserting it again.
- What a plugin declares is spawned as an entity with `ContributedBy` the
  plugin. `Contributions` on the plugin is `linked_spawn`, so what a plugin
  added goes with it.
- The `Disable` event, by plugin id, despawns the contributions and sets
  `Status::Disabled`. `Enable` runs `entry` again.
- `Blocked` holds why a plugin is held back or was switched off: a plugin it
  needs is not there, not the right version or not running. A system of
  `plugin::plugin` switches off the plugins whose needs stop being met and
  switches them on again when they are (`plugin::needs` has the logic).

## plugins/

One crate each, named `plugin_<name>`, with `pub fn plugin(app: &mut App)`
that adds its `HamletPlugin`. `plugins/all` is `hamlet_plugins`: `Builtin`
is a `PluginGroup`, and each plugin is an optional dependency behind a
feature of its name, all on by default. The binary and the gallery add
`Builtin` after `UiPlugin` or `SdkPlugin`.

| Plugin | What it adds |
|---|---|
| `plugin_skills` | The Skills panel: the selected agent's own skills over the library. TAKE OUT asks through `Confirm` and then runs `/unshelve` as the chief |

Settings has a Plugins page that lists every `PluginInfo` with a switch
that sends `Enable` or `Disable`, and shows what each asks for and what the
chief chose ([`permissions.md`](permissions.md)).
