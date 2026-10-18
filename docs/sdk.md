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
| `grant`, `agents` | `Granted`, the capabilities of an installed plugin, and `AgentList`, a read-model of the living agents and their hamlets that a system keeps |
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

- `Manifest`: id, name, version, the plugins it needs (an id and a version
  range) and whether it is built in. `Manifest::built_in("id")` starts one.
- `Entry` is what `entry` declares into. It reaches neither the app nor the
  world. For now it declares a panel, with `panel` or with `panel_by_id`.
  A new kind of contribution is a variant of `Declared`, a method of
  `Entry` and an arm of `Entry::spawn`, all in `plugin/entry.rs`. The WASM
  host makes an `Entry` of what an installed plugin declared and spawns it.
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
that sends `Enable` or `Disable`.
