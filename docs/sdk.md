# The SDK and the plugins

A plugin is a unit of function added to the window: a panel in the dock, and
commands that menu lines, keys and the menu of a thing in the world run, a tab
of the inspector and a page of the settings. `hamlet_sdk` is what a plugin is
built with, `plugins/` holds the plugins, and `hamlet_plugins` lists which are
built in.

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
| `convo` | `ChiefName`, the name of the window's chief. `Me`, the chat's session and the agent it is with. `AgentSelected`, whether the world has an agent selected. `of_agent`, a page about the agent the chat is with |
| `asking` | The card of the questions an agent asks the chief, which Needs You and the chat both show, so an answer begun in one stands in the other |
| `change` | `DrawChange`, set by the window to the function that draws the edit an approval holds as the chat draws it, so a plugin shows an edit the same way |
| `waits` | `Waits`, how many things wait for the chief, for the status bar and any plugin |
| `papers` | `Papers`, a read-model of the selected agent's notes, character and skills. Empty while no agent is selected |
| `confirm` | `Confirm`, an observer event that asks the chief a question before a command is run |
| `command` | `Command`, the one action; `RunCommand`, which runs one by its id, on a `Subject` where it has one; `Handler`, `HandlerWith` and `Opens`, what a built-in one does; `Forward`, a command the plugin carries out itself; `Notice`; and `Commands`, a read-model of every command |
| `menu` | `Menu`, `MenuEntry`, a line a plugin adds to a menu of the top bar, and `Menus`, a read-model of the lines that may show |
| `context` | `ContextItem`, an item a plugin adds to the menu of a thing in the world, `ContextWhen`, and `ContextItems`, a read-model of the items that may show |
| `inspector` | `InspectorSection`, a tab a plugin adds to the inspector's page of an agent, and `InspectorSections`, a read-model of the tabs that may show |
| `settings` | `SettingsPage`, a door of the Settings panel to a page of a plugin's, and `SettingsPages`, a read-model of the pages that may show |
| `page` | `Native` and `ShownWhen`, for a page a built-in plugin draws itself, and `content`, what fills the page of a contribution |
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
  out itself, as an installed plugin does), or `command_with` (a function
  that is given the `Subject` it is run on); a line of a menu, with
  `menu_item`; an item of the menu of a thing in the world, with
  `context_item` and `context_item_if`; a tab of the inspector, with
  `inspector_section` and `inspector_section_with`; a door of the Settings
  panel, with `settings_page` and `settings_page_with`; and a shortcut, with
  `key`. A new kind of contribution is a
  variant of `Declared`, a method of `Entry` and an arm of `Entry::spawn`, all
  in `plugin/entry.rs`. The WASM host makes an `Entry` of what an installed
  plugin declared and spawns it.
- Nothing is a list that is added to and removed from. What a plugin
  declares is an entity, and a host (the menu bar, the key handler, the
  menu of the world, the inspector, the Settings panel, the Plugins page)
  reads those entities with an ordinary system that keeps a small
  `PartialEq` resource (`Menus`, `Keys`, `Commands`, `ContextItems`,
  `InspectorSections`, `SettingsPages`) only when it differs.
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
window runs a command with `commands.trigger(RunCommand::of(id))`, or with
`RunCommand::on(id, subject)` for a command that is run on a thing of the
world; an observer finds it:

1. A plugin that is not running (`Switched off`, `stopped`, `not started
   yet`) or not there runs nothing. The reason is logged and put in the open
   chat as a notice (`Notice`), such as ``probe/wave did not run: Probe is
   switched off``.
2. A command with a `Handler`, a `fn(&mut Commands)`, runs it. This is a
   built-in plugin's command. One with a `HandlerWith`, a `fn(&mut Commands,
   &Subject)`, runs it with the subject of the `RunCommand`, and is refused
   with a notice where there is none. One with `Opens` brings the panel of
   that id forward through `dock::open`, adding it to the dock first if it is
   closed.
3. Any other command is the plugin's own to carry out, and `Forward` is
   triggered, with the subject written as `args`. `hamlet_wasm` puts it in
   the inbox of an installed plugin as `command` with the sender `chief` (see
   [`wasm.md`](wasm.md)); the chief needs no `plugin.use` for its own
   commands.

`Commands`, a `PartialEq` resource, lists every command with its id, title,
owner and whether the owner is running, for the views that show them.

### A subject

`Subject { kind, id, name }` is a thing of the world that a command is run
on: what a menu was opened on. `kind` is one of `command::kind`:

| Kind | The thing | Of the realm's `Thing` |
|---|---|---|
| `agent` | An agent besides the mayor, as a robot | `Resident` |
| `house` | An agent's house | `ResidentHouse` |
| `mayor` | The mayor, or its house | `Keeper`, `Cottage` |
| `ruin` | The remains of the house of an agent that left | `Ruin` |
| `hamlet` | A hamlet, by its common, its workbench or its clockwork | `Common`, `Workbench`, `Clockwork` |
| `chief` | The chief's mascot | `Chief` |
| `hearth` | The chief's Hearth | `ChiefPlace` |
| `library` | The library every agent shares | `Library` |
| `ground` | The bare ground | none |

`id` names the thing among those of its kind while it is there, and is of no use
after that. `name` is what the window calls it. The realm does not know the SDK:
`hamlet_ui::world_menu` makes a `Subject` of a `Thing`, and a built-in command
reads the entity back from the `id`. A plugin's command is given the subject
written with `postcard` (`Subject::encode`, and `Subject::decode`), as `args`.

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

### Context items

`ContextItem { target, section, label, command, icon }` is an entity
`ContributedBy` a plugin: an item of the menu a right click opens on a thing of
the kind `target`, which runs the plugin's command `command` on the thing. The
`label` may hold `{name}`, the thing's name. `ContextItems` is the read-model of
the items that may show, and `hamlet_ui::world_menu` builds a menu from the
items of the kind of the thing, ordered by `section` and then by label, with
the lines of its own that depend on a second agent that is chosen (a road to
it, merging the two) and on the roads an agent has. The menu has no line
between sections.
A click triggers `RunCommand::on` for the item's command and the thing as a
`Subject`, and closes the menu unless the command opened another popup.

An item of a built-in plugin may have `ContextWhen`, a function of the world and
the subject that says whether it is there (`Entry::context_item_if`): Stop is
only there for an agent at work. The window's own items use the sections 10 to
120, in `hamlet_ui::world_menu::contribute`, so that an item a plugin adds with
a higher section comes after them. A plugin that is not built in shows its items
only while the chief allows it `ui.context`, as for menu lines.

### Inspector tabs

`InspectorSection { target, title, panel }` is an entity `ContributedBy` a
plugin: a tab on the inspector's page of the things of the kind `target`, which
is `agent` for now (the mayor's page too). `panel` is the id in the dock of the
plugin panel that fills the page, and the tab takes that panel's icon. The tabs
come after those of the inspector, built in plugins' first, then by plugin and
title. `InspectorSections` is the read-model of the tabs that may show, and the
page is built again when it changes. A plugin learns which agent is selected
from the host event `selection-changed` and draws its panel again.

A built-in plugin may draw the page itself, with `Entry::inspector_section_with`,
which takes a view (`Native`) and when the tab is there (`ShownWhen`). The Skills
plugin adds the tab SKILLS this way, only for an agent that has skills of its
own. A plugin that is not built in shows its tabs only while the chief allows
it `ui.inspector`.

### Settings pages

`SettingsPage { title, icon, holds, panel }` is an entity `ContributedBy` a
plugin: a door of the Settings panel's front page, under the pages of the
panel itself, to a page that is the plugin panel `panel`. `holds` is a line
under the title of the door, and may be empty. `SettingsPages` is the read-model
of the pages that may show: built in plugins' first, then by plugin and title.
The Plugins page is one: the plugin `settings` adds it with
`Entry::settings_page_with`, which takes the view of the page. A plugin that is
not built in shows its pages only while the chief allows it `ui.settings`.

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
entities, and no menu line or key points at them; the window's lines and the
items of its menus run commands of the `window` plugin, which act through the
world. The world's
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
| `plugin_needs` | The Needs You panel, titled PETITIONS: everything that waits for the chief, oldest first, with the answers each takes and a field for the reason or reply. It reads the chief's `ChiefName` and, where the window has set it, `DrawChange`, so it runs with no window |
| `plugin_settings` | The Settings panel: its front page and the pages of it (endpoints, models, agents, charter, realm, senses) and the Plugins page. It stands on `hamlet_chat` (markdown, links, the deeds that ask first), `hamlet_limits` (the models and the rules of the realm) and `hamlet_realm` (the sky and the sound it sets) |
| `plugin_skills` | The Skills panel: the selected agent's own skills over the library, a VIEW line that opens it, and a SKILLS tab of the inspector's page of an agent that has skills of its own. TAKE OUT asks through `Confirm` and then runs `/unshelve` as the chief |

Settings has a Plugins page that lists every `PluginInfo` with a switch
that sends `Enable` or `Disable`, and shows what each asks for, what it adds
and what the chief chose ([`permissions.md`](permissions.md)).
