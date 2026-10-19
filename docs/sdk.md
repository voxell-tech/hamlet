# The SDK and the plugins

A plugin is a unit of function added to the window: a panel in the dock, and
commands that menu lines, keys and the menu of a thing in the world run, a tab
of the inspector and a page of the settings. `hamlet_sdk` is what a plugin is
built with, `plugins/` holds the plugins, and `hamlet_plugins` lists which are
built in.

```
hamlet (binary) -> hamlet_plugins -> plugins/<name> -> hamlet_limits, hamlet_chat -> hamlet_agents_api -> hamlet_sdk -> hamlet_base, hamlet_assets
                 |                                 \-> hamlet_realm_api ------------------------------> hamlet_core, hamlet_rules
                 |                                      ^
                 \-> hamlet_ui (the frame) ------------/   hamlet_realm -> hamlet_realm_api
```

A plugin crate depends on `hamlet_sdk`, `hamlet_agents_api` where it reads
the agents, `hamlet_realm_api` where it reads the world or asks something of
it, the engine crates and the two crates that hold what plugins share,
`hamlet_chat` and `hamlet_limits` (below), never on `hamlet_ui` or on
another plugin. A plugin reads the core through `hamlet_agents_api`, which
gives the core's types, events and functions a plugin uses; only the layers
`plugin_agents` and `plugin_hamlets` depend on `hamlet_core`. `plugin_needs` and
`plugin_skills` need nothing but the SDK, the agents API and, for `needs`,
the rules, and run with no window and no realm. Only
`plugin_realm` depends on `hamlet_realm` itself: it holds the layer that
adds the realm and the realm's view (`view`, the plugin `world-view`),
which mounts the realm's camera in a panel, sets its stage and its sky,
and hangs the realm's labels off the panel. No other crate of the window does: the chat,
the frame and every other plugin see the world only through
`hamlet_realm_api`. A test of `hamlet_plugins` (`layering.rs`) reads each
plugin's `Cargo.toml` and fails on a dependency that breaks these rules.

## hamlet_realm_api

The plain types and events of the realm, which the realm keeps and answers.
A panel that runs with no realm reads the read models as they were left,
and its asks go unanswered.

| Module | What it holds |
|---|---|
| `pick` | `Thing`, `Selected`, `Also`, `Hovered`, `Asked`, `AskedGround`, `Asks`, `Taken`, `Following`, `Steer`, `Pressable`; `select` and `ask_about`; `Focus` and `focus_on`, which the camera answers; `PointAt`, a click on a thing a plugin names (`Named`, `Gesture`), which the realm answers |
| `chief` | `Chief`, `Beside`, `named`, and `look`: a chief's `Look` and its file |
| `palette` | The world's `Palette`, and `RobotPaint`, the paint of an agent's robot, which the chat draws a speaker in |
| `hand` | `Hold`, the hand's `Grip` for the cursor, and `Proud` |
| `sky` | `Quality` and `TimeOfDay`, the chief's choices of how the world is drawn |
| `sound` | `PanelCue` and `PanelSound`, the panels' own sounds; `SoundChoice`, what the chief chose to hear |
| `orders` | `Decree` and `Tell`, which the chat says; `decree`, `restore`, `part` and `offers`; `Introduce`, `Give`, `Carry` and `Switched`, which the realm answers |
| `overlay` | `Overlays`, the marks plugins draw over the world, which the host of the plugins keeps and the realm's view draws |
| `survey` | `Survey`, the realm by name for a plugin that holds `read.world`: the hamlets, the agents and where each stands, the thing selected and the hour |

## What plugins share

| Crate | What it holds |
|---|---|
| `hamlet_chat` | The chat with an agent (`chat`), the markdown in it (`markdown`), selecting words (`select`), links (`links`), pictures (`pictures`), who the chat is with and how it is pointed at another (`me`), and the deeds that ask the chief first (`deed`: `Deed`, `ask`, `stop`). A second plugin needs it: the Inspector draws the chat, the Settings draws markdown and asks before it removes a charter, and the window reads the chat for its sounds, its status bar and its mascot |
| `hamlet_limits` | The panes of the limits: the home's endpoints and catalogue (`models`) and each hamlet's budget, votes, models and the mayor's leave (`rules`). The Settings shows the home's, the Inspector each hamlet's |

Each is also a built-in plugin, `chat` (`ChatPlugin`) and `limits`
(`LimitsPlugin`), both essential, which `Builtin` adds after the frame. A
plugin crate depends on them for their types, and adds neither.

A plugin asks another only through the SDK or an API crate: an event one
observes (`deed::Ask`, `settings::SetUpAgain`, `settings::AddEndpoint`,
`confirm::Confirm`), a command it runs (`menus/found`), or a resource one
keeps (`Bar`, `Roll`, `AgentSelected`, `PointerWait`, `Animating`).

## hamlet_sdk

| Module | What it holds |
|---|---|
| `theme`, `looks`, `icons`, `platform` | How the panels look: the theme, the buttons and fields, the icons, the typefaces and the keyboard focus |
| `feel` | `Mood`, `Lit` and `FootLight`, the marks a view puts on a node for the feel in `hamlet_ui` to read |
| `cards` | The card of a list row and the heading over a part of a panel |
| `tabs` | A strip of tabs that folds when it is narrow, and the system that marks the one shown |
| `checks` | The checks a signal reads again by: `moved`, when a resource changed, and `either` |
| `bar` | `Bar`, the words of the status bar, which a panel may quote |
| `papers` | `Papers`, a read-model of the selected agent's notes, character and skills. Empty while no agent is selected. `hamlet_agents_api` keeps it |
| `confirm` | `Confirm`, an observer event that asks the chief a question before a command is run |
| `command` | `Command`, the one action; `RunCommand`, which runs one by its id, on a `Subject` where it has one; `Handler`, `HandlerWith` and `Opens`, what a built-in one does; `Forward`, a command the plugin carries out itself; `Notice`; and `Commands`, a read-model of every command |
| `menu` | `Menu`, `MenuEntry`, a line a plugin adds to a menu of the top bar, and `Menus`, a read-model of the lines that may show |
| `context` | `ContextItem`, an item a plugin adds to the menu of a thing in the world, `ContextWhen`, and `ContextItems`, a read-model of the items that may show |
| `inspector` | `InspectorSection`, a tab a plugin adds to the inspector's page of an agent, and `InspectorSections`, a read-model of the tabs that may show |
| `settings` | `SettingsPage`, a door of the Settings panel to a page of a plugin's, and `SettingsPages`, a read-model of the pages that may show. `ShowModels`, `SetUpAgain` and `AddEndpoint`, events between the Settings and the window's first-run screen |
| `page` | `Native` and `ShownWhen`, for a page a built-in plugin draws itself, and `content`, what fills the page of a contribution |
| `keys` | `Key`, a shortcut a plugin adds, `chord` (reading and writing `Cmd+Shift+N`), and `Keys`, a read-model of the shortcuts with whether each runs |
| `dock` | `Panel`, `add_panel`, `panel` and `lit_panel`, `show_tab`, `open` and `TabShown` |
| `frame` | `FramePart` and `Place`, a part a built-in plugin adds to the window's frame, and `parts` and `views`; the window's states `Showing` and `WorldShown`; `Mounting`, the set the frame is mounted in; `Animating` and `PointerWait`, what the frame reads of the plugins |
| `capability` | `Capability`, the names a plugin can ask for and `NEVER`, the ones no plugin can; `Asks`, what a manifest asks for; `Choice` and `Choices`, what the chief chose; `Granted`, the verdict for each capability the manifest names; `Petition`, `Answer` and `Answered`, a question put to the chief and the answer back. See [`permissions.md`](permissions.md) |
| `agents` | `AgentList`, a read-model of the living agents and their hamlets, and `SelectedAgent`, the name of the agent selected in the world. `hamlet_agents_api` keeps both, in the set `Kept` |
| `events` | The catalogue of the host's events a plugin may subscribe to: `selection-changed` and `agents-changed`, each behind `read.agents`. See [`wasm.md`](wasm.md) |
| `tree` | The tree of a plugin's panel: `Node`, the `PluginTrees` read-model of what each panel shows, the view built from it with the looks of the panels, and the `Interaction` event of a click or typing in it |
| `plugin` | The plugin types below |

`Cx`, `AnyView`, `View` and `with_cx` are the view types of the window, and
`HomeDir` the home the app opened.
`SdkPlugin` is the plugin to add before any other: it keeps fynix's
`DockRegistry` in step with the `Panel` entities.

## hamlet_agents_api

What a plugin reads and does about the agents. It stands on the SDK and the
core, and the layers add its plugins (below).

| Module | What it holds |
|---|---|
| `agents` | The systems that keep the SDK's `AgentList` and `SelectedAgent`; the core's marks on an agent, a hamlet and a road (`Citizen`, `Mayor`, `Busy`, `Left`, `Settlement`, `Treasury`, `Road` and the rest), and its `name_of`, `mayor` and `register` |
| `session` | The core's `Entry`, `Transcript`, `Status`, `Usage`, `SessionKey`, `Seat`, `UserMessage` and `JobFinished`; `house_at`, `log_of` and `prompt_of`; and `archive`, the past sessions |
| `oversight` | The core's `Waiting`, `Need`, `Asks`, `Answer`, `Kind`, `Danger`, `Veto`, `Revoke`, `Grants` and the tags of a petition |
| `model` | The core's `Settings`, `Model` and `Link`, signing in, and the events that save, test, list and try the models |
| `library` | The core's `Skills`, the library's skills as last read, and `RefreshLists`, which reads them again |
| `papers` | The systems that read the SDK's `Papers` from the selected agent's house, and `character_file` |
| `convo` | `ChiefName`, the name of the window's chief. `Me`, the chat's session and the agent it is with (`Me::of` makes one). `AgentSelected`, whether the world has an agent selected. `of_agent`, a page about the agent the chat is with. `send_as_chief`. It puts each `Notice` in the open chat |
| `roll` | `Roll`, the count of changes to the agents and the hamlets, and the checks over it (`rolled`, `agents_moved`, `hamlets_moved`) |
| `words` | The words for an agent's step (`now`), a hamlet's state (`standing`) and a purpose cut short (`short`) |
| `asking` | The card of the questions an agent asks the chief, which Needs You and the chat both show, so an answer begun in one stands in the other |
| `change` | The core's `Change`, and `DrawChange`, set by the window to the function that draws the edit an approval holds as the chat draws it, so a plugin shows an edit the same way |
| `waits` | `Waits`, how many things wait for the chief, for the status bar and any plugin |
| `petition` | Puts the SDK's `Petition` in the core's queue and passes the chief's answer back as `Answered` |
| `tools` | The core's tools a plugin gives the agents, the sandbox they run in (`Sandboxing`) and the turns a plugin is told of, for a host of plugins |

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
  panel, with `settings_page` and `settings_page_with`; a shortcut, with
  `key`; and a part of the window's frame, with `frame_part` (below). A new
  kind of contribution is a
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
`plugin_menus::world_menu` makes a `Subject` of a `Thing`, and a built-in command
reads the entity back from the `id`. A plugin's command is given the subject
written with `postcard` (`Subject::encode`, and `Subject::decode`), as `args`.

### Menu lines

`MenuEntry { menu, section, label, command, icon }` is an entity `ContributedBy`
a plugin; `command` is the name of one of that plugin's own commands, and a line
whose command is not there does not show. `menu` is `Menu::App` (HAMLET) or
`Menu::View`. A menu lists its lines by `section`, then by label, then by plugin
id, with a hairline between sections; the icon is the entry's, or the command's.
`Menus` is the read-model of the lines that may show, and `plugin_menus::menu`
draws it when a menu is open. A click triggers `RunCommand` for the line's
command.

A plugin that is not built in shows its lines only while the chief allows it
`ui.menu` (`capability::allowed`). A line held back is still an entity, so
Allow shows it and Don't allow or Ask next time takes it off at once.

The window's own lines are the plugin `window` (`hamlet_ui::commands`): Run
in Background and Quit in HAMLET. Quit is in section 100 so that it ends the
menu whatever plugins add. The `menus` plugin adds Found a Hamlet to HAMLET,
`world-view` the camera and the sky to VIEW, and Skills a VIEW line that
opens its panel.

### Context items

`ContextItem { target, section, label, command, icon }` is an entity
`ContributedBy` a plugin: an item of the menu a right click opens on a thing of
the kind `target`, which runs the plugin's command `command` on the thing. The
`label` may hold `{name}`, the thing's name. `ContextItems` is the read-model of
the items that may show, and `plugin_menus::world_menu` builds a menu from the
items of the kind of the thing, ordered by `section` and then by label, with
the lines of its own that depend on a second agent that is chosen (a road to
it, merging the two) and on the roads an agent has. The menu has no line
between sections.
A click triggers `RunCommand::on` for the item's command and the thing as a
`Subject`, and closes the menu unless the command opened another popup.

An item of a built-in plugin may have `ContextWhen`, a function of the world and
the subject that says whether it is there (`Entry::context_item_if`): Stop is
only there for an agent at work. The items of the `menus` plugin use the sections 10
to 120, in `plugin_menus::world_menu::contribute`, so that an item a plugin adds with
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
items of its menus run commands of the `window`, `menus` and `world-view`
plugins, which act through the world. The world's
camera keys (F to come in close, Escape to let go) and the commit keys of a
text area are handled where they apply and are not chords.

## plugins/

One crate each, named `plugin_<name>`, with `pub fn plugin(app: &mut App)`
that adds its `HamletPlugin`. `plugins/all` is `hamlet_plugins`: each plugin
is an optional dependency behind a feature of its name, all on by default,
listed in these `PluginGroup`s:

| Group | What it lists |
|---|---|
| `Base` | `agents` and `hamlets`, which run with no window. A headless app adds it after `hamlet_sdk::plugin::plugin` |
| `Frame` | The plugins that fill the window's frame: `world-view`, `menus`, `status` and `welcome` |
| `Panels` | The plugins that add a panel. The gallery adds it after `SdkPlugin` and the read models of `hamlet_agents_api` |
| `Builtin` | `Base`, the frame (`hamlet_ui::UiPlugin`, under the `window` feature), `chat`, `limits`, `realm`, `Frame` and `Panels`, in that order. The binary adds it after Bevy's default plugins |

### The layers

Three plugins are the layers the others stand on. Each is `essential`: its
manifest says so (`Manifest::essential`, `PluginInfo::essential`), `Disable`
leaves it running, and the Plugins page shows it as Essential with no switch,
because the app cannot run without it. A layer needs the one below it, and
never depends on one above. `settings` and `needs` are essential too, so the
chief cannot switch off the Plugins page or the petitions, the two places a
plugin is put right, and so are `window`, which owns Quit, and `menus`,
which shows it: the close button sends the app to the background, so Quit
is the only way out. `chat` is essential as the place the chief talks to
the agents, and `limits` as the panes the first-run screen and the
Settings choose the models with. The inspector, the world's panel,
the status bar and the first-run screen may be switched off; a panel closed
for good is opened again from a tab bar's plus.

| Layer | What it adds |
|---|---|
| `plugin_agents` | The resources the core starts from, made of `Launch`, a resource the binary inserts and the plugin takes out: the runtime, the home, the sandbox, the model (`Model::new`, which answers from a recorded session where there is one) and the settings. Then `hamlet_core`'s modules `core` (the models and the library's skills), `agents` (the hamlets, agents and roads as entities), `catalogue`, `session` and `oversight` (what waits for a chief and the chief's answers, the gate on every tool call), and the read models of `hamlet_agents_api` over them (`convo`, `agents`, `papers`, `roll`). The gate is here, not in `hamlets`, so that no build runs an agent's tools without it |
| `plugin_hamlets` | `waits` and `petition` of `hamlet_agents_api`: the count of what waits for the chief, and the petitions of plugins put in the chief's queue. Needs `agents` |
| `plugin_realm` | `world`, `hamlet_realm`'s modules in order: the scene (settings, palette, play, sound, debug, sky, stage, keeper, stock), the generator, what the chief does in it (pick, needs, bubble, hand, orders, chief, library, moving, works, lantern), and the core's entities drawn (`hamlet`, `survey`). Needs `hamlets` |

A layer owns its wiring: which of the engine's modules make it up, in which
order, and what they start from. An engine crate has no `Plugin` of its own
and is a library: its types, systems and functions, and for each module a
`plugin` function that adds that module's own systems, observers and
resources beside them. An app that wants part of an engine adds the
modules it needs: the core's tests add `core`, `agents`, `catalogue` and
`session` with the resources they start from, the style test the realm's
scene, and `far_lands` and `chief_sheet` add `plugin_realm::world`.

The wiring stays per module, not in the layer's crate, for three reasons.
Most of what a module adds is private to it (a channel and the system that
drains it, the observer that queues a deed), and moving the lines that add
them would make every one of them public. The engine's own tests build an
app from its modules, and the engine cannot depend on the plugin crate
above it. And a module's plugin stands first in its module, beside what it
adds, so a change to a system and to where it runs is one edit. Moving the
engines themselves into the plugin crates was not cheap either:
`hamlet_core` is under `hamlet_agents_api`, `hamlet_chat`, `hamlet_limits`,
`hamlet_realm_api`, `hamlet_realm`, `hamlet_ui`, `hamlet_wasm` and
`hamlet_desktop`, which would then stand on a plugin.

A layer's `HamletPlugin` declares nothing in `entry`: neither engine knows
the SDK, so no command, menu line or other contribution lives in them. The
items of the menu of an agent, a hamlet or the ground are the `menus`
plugin's, and the camera's and the sky's lines are `world-view`'s.
`plugin_realm` also holds the realm's view, which reaches into the realm's
camera, stage, sky, labels and the chief's mascot: `view::plugin` is a
plugin of its own, `world-view`, which `Frame` adds and the chief may
switch off.

| Plugin | What it adds |
|---|---|
| `plugin_realm::view` | The world's panel, titled REALM: the realm's camera drawn into it at the window's pixel scale and never larger than the window, the pointer passed on to the camera, the labels of the world and the marks plugins draw over it (`overlay`) hung off it; the stage set from the mayor's session, the sky held at day or night, `Animating` while something moves; the VIEW lines Reset Camera and the sky's three; and the chief's mascot brought into the world, kept beside whoever the chat is with and its look kept between launches |
| `plugin_menus` | HAMLET and VIEW in the top bar (a part of the frame) and the open menu; the menu a right click opens on a thing of the world or the ground, with the items and commands of its own (Talk, Focus, Follow, Stop, Give a Skill, Undo, Make Mayor, Remove, Restore, Move, Close Down, the chief's and the library's, Found a Hamlet Here) and the roads and merges of two that are chosen; the form a hamlet is founded with, with its HAMLET line; the question before a `Deed` and a plugin's `Confirm`; and the sounds of those popups. Essential |
| `plugin_status` | The status bar at the foot of the window: the sandbox, the model, the connection, what the session and the hamlet spent, the link under the pointer, what waits and whether an agent works. It keeps `Bar` |
| `plugin_welcome` | The first-run screen over the dock, on a home whose model was never chosen: a provider's key, signing in, a server on this computer, and the models of whoever answered. Settings opens it again with `SetUpAgain` and `AddEndpoint` |
| `plugin_inspector` | The Inspector: whoever is selected in the world, or every hamlet when none is. An agent's page (its chat, notes, character and past sessions, and the tabs plugins add), a hamlet's overview and rules, a chief's page and look, the page of an agent that is gone and the library's. It brings itself forward when something is selected, and adds the PROMPT item to the menu of an agent, its house and the mayor |
| `plugin_needs` | The Needs You panel, titled PETITIONS: everything that waits for the chief, oldest first, with the answers each takes and a field for the reason or reply. It reads the chief's `ChiefName` and, where the window has set it, `DrawChange`, so it runs with no window |
| `plugin_settings` | The Settings panel: its front page and the pages of it (endpoints, models, agents, charter, realm, senses) and the Plugins page. It stands on `hamlet_chat` (markdown, links, the deeds that ask first), `hamlet_limits` (the models and the rules of the realm) and `hamlet_realm_api` (the quality and the `SoundChoice` it sets) |
| `plugin_skills` | The Skills panel: the selected agent's own skills over the library, a VIEW line that opens it, and a SKILLS tab of the inspector's page of an agent that has skills of its own. TAKE OUT asks through `Confirm` and then runs `/unshelve` as the chief |

Settings has a Plugins page that lists every `PluginInfo` with a switch
that sends `Enable` or `Disable`, and shows what each asks for, what it adds
and what the chief chose ([`permissions.md`](permissions.md)). Every plugin
but the essential ones can be switched off from it; the panel or the part of
the frame of one that is off is gone until it is switched on or the app
starts again.

### The frame

A built-in or native plugin puts a view in the window around the dock with
`Entry::frame_part(place, order, view)`: a `FramePart` entity with the view as
its `Native`, gone with the plugin. `Place` is the top bar (`Bar`, before the
room the window is held by), over the dock (`Dock`), the foot (`Foot`) or over
everything (`Over`), and the parts of a place are drawn by `order`. The root
is built once the theme is ready, in the set `Mounting`, and again when a
part comes or goes, as for a new theme. The menus are the `Bar` part and two
`Over` parts (0 and 10), the status bar the `Foot` part and the first-run
screen the `Dock` part. A native plugin reaches `Entry` through
`hamlet_native::declare` and may add parts too, as it runs with full trust.
A WASM plugin adds none: a view is a function in the app, and the host
hands a WASM plugin only the panels drawn from its tree.

### What stays in the window

`hamlet_ui` is the frame, and keeps only what belongs to the window and to
no one subject:

| Part | Why it is the frame's |
|---|---|
| `root` | The root the dock and the parts are mounted in, built again for a theme or a part that came or went. It also tells the world whether any popup is open (`over` of `Taken`), whoever opened it, so that Escape closes it and the hover card hides |
| `dock`, `remember` | The dock is the host every panel is in, and its layout, the chosen sky, sound and quality and the selection are saved together in one file of the window |
| `window`, `chrome`, `commands` | The window's states (`Showing`, `WorldShown`), its size, its buttons and grip, and the pace it is drawn at from `Animating`; Run in Background and Quit are what the window does to itself |
| `cursor` | One pointer for the whole window, chosen from what every part reports: the panels' cursors, `Hold`, `Steer`, `Hovered`, `PointerWait` |
| `hover` | The card that follows the pointer over any thing of the world, over every panel and under the menus. It reads the realm only through `hamlet_realm_api` |
| `sounds`, `widgets` | How every control feels and sounds, and the sounds no single plugin makes: the dock's tabs and drags, a fold, a card, a message sent or failed |

The frame also adds the question card of `hamlet_agents_api`, which
several plugins share. The chat, a plugin of its own, bridges the
world's selection to who the chat is with (`hamlet_chat::me`), so that a
plugin sees the selection as `AgentSelected`, `SelectedAgent` and `Me`, and
`plugin_realm::view` keeps the mascot beside it.

### The SDK and the core

`hamlet_sdk` does not depend on `hamlet_core` or `hamlet_rules`. What it shares
with the core is in `hamlet_base`: the home the app opened (`HomeDir`), where the
capability store is kept, and the rules of a plugin's names. Its read models of
the agents (`AgentList`, `SelectedAgent`, `Papers`) are plain data, and a petition
is its own `Petition` entity with its own `Answer`s; `hamlet_agents_api` fills
the first from the core and carries the second to the core's queue and back.
`hamlet_wasm` reaches the core only through `hamlet_agents_api`.
