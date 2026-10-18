# The SDK and the plugins

A plugin is a unit of function added to the window: a panel in the dock, and
commands that menu lines and keys run. `hamlet_sdk` is what a plugin is built with, `plugins/` holds the
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
| `command` | `Command`, the one action; `RunCommand`, which runs one by its id; `Handler` and `Opens`, what a built-in one does; `Forward`, a command the plugin carries out itself; `Notice`; and `Commands`, a read-model of every command |
| `menu` | `Menu`, `MenuEntry`, a line a plugin adds to a menu of the top bar, and `Menus`, a read-model of the lines that may show |
| `keys` | `Key`, a shortcut a plugin adds, `chord` (reading and writing `Cmd+Shift+N`), and `Keys`, a read-model of the shortcuts with whether each runs |
| `dock` | `Panel`, `add_panel`, `panel` and `lit_panel`, `show_tab`, `open` and `TabShown` |
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
  world. It declares a panel, with `panel` or with `panel_by_id`; a command,
  with `command` (a function that runs it), `opens` (it brings a panel
  forward, opening it if it is closed) or `forwarded` (the plugin carries it
  out itself, as an installed plugin does); a line of a menu, with
  `menu_item`; and a shortcut, with `key`. A new kind of contribution is a
  variant of `Declared`, a method of `Entry` and an arm of `Entry::spawn`, all
  in `plugin/entry.rs`. The WASM host makes an `Entry` of what an installed
  plugin declared and spawns it.
- Nothing is a list that is added to and removed from. What a plugin
  declares is an entity, and a host (the menu bar, the key handler, the
  Plugins page) reads those entities with an ordinary system that keeps a
  small `PartialEq` resource (`Menus`, `Keys`, `Commands`) only when it
  differs.
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

### Commands

`Command { id, title, icon }` is an entity `ContributedBy` the plugin that
owns it, and `id` is the name the owner exports it by. `command::full(plugin,
id)` makes `plugin/id`, which is the id `RunCommand` takes. Any part of the
window runs a command with `commands.trigger(RunCommand { id })`; an observer
finds it:

1. A plugin that is not running (`Switched off`, `stopped`, `not started
   yet`) or not there runs nothing. The reason is logged and put in the open
   chat as a notice (`Notice`), such as ``probe/wave did not run: Probe is
   switched off``.
2. A command with a `Handler`, a `fn(&mut Commands)`, runs it. This is a
   built-in plugin's command. One with `Opens` brings the panel of that id
   forward through `dock::open`, adding it to the dock first if it is closed.
3. Any other command is the plugin's own to carry out, and `Forward` is
   triggered. `hamlet_wasm` puts it in the inbox of an installed plugin as
   `command` with the sender `chief` (see [`wasm.md`](wasm.md)); the chief
   needs no `plugin.use` for its own commands.

`Commands`, a `PartialEq` resource, lists every command with its id, title,
owner and whether the owner is running, for the views that show them.

### Menu lines

`MenuEntry { menu, section, label, command, icon }` is an entity `ContributedBy`
a plugin; `command` is the name of one of that plugin's own commands, and a line
whose command is not there does not show. `menu` is `Menu::App` (HAMLET) or
`Menu::View`. A menu lists its lines by `section`, then by label, then by plugin
id, with a hairline between sections; the icon is the entry's, or the command's.
`Menus` is the read-model of the lines that may show, and `hamlet_ui::menu`
draws it when a menu is open. A click triggers `RunCommand` for the line's
command.

A plugin that is not built in shows its lines only while the chief allows it
`ui.menu` (`capability::allowed`). A line held back is still an entity, so
Allow shows it and Don't allow or Ask next time takes it off at once.

The window's own lines are the plugin `window` (`hamlet_ui::menu::window`): Found
a Hamlet, Run in Background and Quit in HAMLET, and the camera and the sky in
VIEW. Quit is in section 100 so that it ends the menu whatever plugins add.
The Skills plugin adds a VIEW line that opens its panel.

### Keys

`Key { chord, command }` is an entity `ContributedBy` a plugin, with the name
of one of its commands. A `Chord` is bevy_fynix's: a key by its place on the
keyboard and the modifiers held. It is written as `Cmd+Shift+N`, `Ctrl+K` or
`F5` (`keys::chord::parse`, and `Display` writes it back). `Cmd` is the
command key of a Mac (Super) and Control elsewhere, so a chord written with it is
one shortcut on every platform; `Ctrl` is Control; `Alt`, `Shift` and `Super`
are as they read. Escape and Enter are not shortcut keys.

`keys::press` turns a key that went down with the modifiers held into
`RunCommand`, from the read-model `Keys`. A chord without Cmd, Ctrl or Super
does nothing while a text field has the focus, so typing is never taken for a
shortcut. Where several bindings have the same chord:

- one built-in plugin's chord runs, and a plugin's chord that a built-in one
  has never does (`Standing::Shadowed`);
- two or more bindings of the same kind (two plugins, or two built-in plugins)
  run neither (`Standing::Clash`).

The Plugins page flags both. A plugin that is not built in has its keys only
while the chief allows it `ui.keys`, as for menu lines.

### What is not a command yet

The chief's slash commands, the table `ALL` in `hamlet_rules::commands`, are
what the chief's decrees are made of: each names who carries it out (the
session, the oversight, a hamlet or the realm). They are not `Command`
entities, and no menu line or key points at them; the window's lines run
commands of the `window` plugin, which act through the world. The world's
camera keys (F to come in close, Escape to let go) and the commit keys of a
text area are handled where they apply and are not chords.

## plugins/

One crate each, named `plugin_<name>`, with `pub fn plugin(app: &mut App)`
that adds its `HamletPlugin`. `plugins/all` is `hamlet_plugins`: `Builtin`
is a `PluginGroup`, and each plugin is an optional dependency behind a
feature of its name, all on by default. The binary and the gallery add
`Builtin` after `UiPlugin` or `SdkPlugin`.

| Plugin | What it adds |
|---|---|
| `plugin_skills` | The Skills panel: the selected agent's own skills over the library, and a VIEW line that opens it. TAKE OUT asks through `Confirm` and then runs `/unshelve` as the chief |

Settings has a Plugins page that lists every `PluginInfo` with a switch
that sends `Enable` or `Disable`, and shows what each asks for and what the
chief chose ([`permissions.md`](permissions.md)).
