# The engine

`engine/` holds `hamlet_engine`, the core that runs plugins and gives
them services. It stands apart from the app in `crates/`: no engine crate
depends on a crate there, and the vendored fynix is the only code they
share.

```sh
cargo run -p hamlet_engine
cargo run -p hamlet_engine --features hot-reload
```

## The crates

Each depends only on those above it, and each adds one Bevy plugin,
`plugin`. The binary only adds them.

| Folder | Crate | What it is |
|---|---|---|
| `engine/api` | `hamlet_engine_api` | The names the engine and plugins share. serde only, no Bevy |
| `engine/core` | `hamlet_engine_core` | The tuning files, the home, the log, the budgets |
| `engine/shell` | `hamlet_engine_shell` | The window, its theme, the dock and its panels, workspaces, commands, shortcuts, the menu bar and the status line |
| `engine/plugins` | `hamlet_engine_plugins` | Plugins as entities, what they add, switching them on and off, the plugin list |
| `engine/app` | `hamlet_engine` | The binary, and a demo plugin built into it |

`engine/assets/` holds the engine's tuning files. The typefaces are read
from the repository's `assets/fonts/`, where their licences are.

## The vocabulary

`hamlet_engine_api` is plain data, so a plugin and the engine build the
same types.

- **Ids.** A name is lowercase words of letters, digits and `_`, joined
  by `.`: `hamlet.demo`. A plugin's id is a name. What a plugin adds is
  named within it, and its full id joins the two with `/`:
  `hamlet.demo/notes` for a `PanelId`, `hamlet.demo/hello` for a
  `CommandId`. The owner `engine` is the engine's own and no plugin may
  take it.
- **The manifest** (`plugin.toml`): id, name, version, the API version it
  was built against (`API_VERSION`, 0.1.0), and its asks. `check` refuses
  one built against another API, with an empty or unreadable text, or
  asking for the same thing twice.
- **Capabilities.** An ask names a capability, its target and a reason in
  the plugin's words: `net.fetch` a host, `assets.read` a type name,
  `services.call` another plugin's service. Powers that are never granted
  have no name, so no manifest can ask for them.
- **Assets.** An `AssetId` the store gives, and a `TypeName` such as
  `weather.forecast`. The engine never reads inside an asset.
- **The widget tree** a plugin describes a panel with: text, heading,
  icon, button, toggle, field, list, row, column, scroll, divider. It is
  flat, in pre-order, each node with its child count and an id the plugin
  keeps from one tree to the next. `check` refuses counts that do not
  make one tree, a leaf with children, two nodes of one id, and a tree
  past its `TreeLimits`.
- **The wire format** is postcard, through `encode` and `decode`.
  `decode` refuses bytes left over after the value.

## The core

- **Tuning.** `engine/assets/engine.toml` holds the sizes, timings and
  budgets, and `theme.toml` the shell's look. Both are built into the
  binary. A module reads one section of a file as a resource: it
  implements `Tuning` (the file and the section) and adds
  `TuningPlugin<T>`. With the `hot-reload` feature the files in the
  source tree are looked at every `[tuning] every` seconds, and a saved
  change replaces each section that differs. A file that cannot be read
  is told in the log, and the last good values stay.
- **The home.** The engine keeps its own home, apart from the app's, so
  running it never touches the app's data. `HAMLET_ENGINE_HOME` names it;
  unset, it is `~/.hamlet_engine`. Both names are constants in
  `home.rs` and nowhere else. Tests put in a `Home` of their own.
- **The log.** A `LogLine` message, from a part of the engine or a
  plugin, is passed to the terminal and kept in `Log`, the last
  `[log] keep` lines, for the console.
- **Budgets.** `[budgets]` in `engine.toml`, as the `Budgets` resource:
  every cap on what one plugin may spend.

## The shell

- **The theme.** `EngineTheme` is `theme.toml`, dark only: the colours by
  role, the text sizes, the spacing and the timings, and the token
  traits fynix's views read. A saved theme builds the views again.
- **The window.** `window_plugin` sets Bevy's window from
  `[window]`. Everything shown is built under one root.
- **Fonts.** Inter is the default face. A label tagged `DisplayText` is
  drawn in Cinzel, tracked, for titles.
- **The dock and panels.** The dock is fynix's: split areas of tabs,
  dragged, split and closed by hand, and a "+" on each tab bar that opens
  any panel there is. A panel is an entity with a `Panel`: its id, its
  title, and a function that builds its content given the entity. While
  the entity is there the dock can show it; when it goes, its tabs go.
  `panel(view)` gives content the panel's padding and fill.
- **The console** (`engine/console`) lists the log, newest first.
- **Settings pages** are entities with a `SettingsPage`, as plugins add
  them. No panel shows them yet.

## Workspaces

A workspace is a named set of panels and their layout in the dock. A
person has one or more, and one is shown at a time.

- **The layout** is `Layout` in the vocabulary, so a plugin can suggest
  one: an area of tabs with the one in front, or a split of two layouts
  along an axis with the share of the first.
- **Shown.** The shown workspace's layout is laid out in the dock
  (`workspace/arrange.rs`). Panels the dock does not have, such as those
  of a plugin that is off, are left out of the dock but kept in the
  workspace, and the dock is laid out again when panels come or go, so
  they come back with their plugin. A workspace with no panels shows one
  empty area, whose "+" adds one.
- **Kept.** What the person changes in the dock by hand (a tab moved,
  closed or added, a split dragged) is kept in the shown workspace.
- **Saved** in `workspaces.ron` in the home, once the workspaces have
  been still for `[workspaces] save_after` seconds, and as the engine
  closes. It is written to `workspaces_saving.ron` first and moved over
  the old file, so a save is whole or not at all. RON, since a layout is
  a tree of enums, which RON writes as it is and TOML only as a table
  per level.
- **Restored** at start. A new home starts with one workspace, "Main",
  holding the plugin list and the console. A file that cannot be read is
  moved aside to `workspaces_broken.ron` (or `_2`, `_3`, never over an
  earlier one), the console says so and where, and the workspaces start
  afresh. If it cannot even be moved, nothing is saved, so the file stays
  as it is.
- **The switcher** (`switcher.rs`) is a tab per workspace in the bar at
  the top; a press shows that workspace. "+" adds an empty one, named
  "Workspace" and a number, and shows it. A right-click on a tab offers
  Rename, a field under the tab (Enter keeps the name; Escape or a press
  outside leaves it), and Delete, while there is more than one. A name is
  trimmed, at most 40 characters, and no other workspace's, whatever the
  case.
- **Commands.** New, rename and delete the shown workspace, and next and
  previous, are in the Workspace menu. Ctrl+Tab shows the next workspace
  and Ctrl+Shift+Tab the one before, going round; Cmd+1 to Cmd+9 (Ctrl
  off macOS) show the first nine.
- **Suggested.** A `SuggestedWorkspace`, as a plugin adds one, puts
  "Take workspace" and its name in the Workspace menu while it is there.
  Taking it adds a copy, numbered if the name is taken, and shows it. The
  copy is the person's, to change or delete, and stays when the plugin
  goes.

## Commands, menus and the status line

- **A command** is an entity with a `Command`: its id (`engine/quit`,
  `hamlet.demo/hello`) and its label. Its owner observes `RunCommand` on
  it. `command::run(world, id)` runs one by id; `spawn_command` spawns
  one with its menu entry and keys as its children.
- **A shortcut** is an entity with a `Shortcut`: a chord and the id of
  the command it runs. A key press runs the command of the first
  shortcut with its chord, unless a bare key goes to a text field being
  typed in. `parse_chord` reads keys written as `Primary+Shift+H`:
  Primary is Command on macOS and Control elsewhere.
- **A menu entry** is an entity with a `MenuEntry`: the title of its
  menu and the id of its command. The bar gathers the entries into
  menus, the engine's own first (Hamlet, Workspace, View) and then the
  others in the order they came; an entry shows its command's label and
  first key. An entry whose command is not there is left out.
- **The engine's own.** Quit (Cmd+Q, Ctrl+Q off macOS) in the Hamlet
  menu, the workspace commands, and for each panel a command in the View
  menu that brings it to the front, or opens it in the first area.
- **The status line** at the foot of the window shows the workspace
  shown and the log's last line, an error in red.

## Plugins

`hamlet_engine_plugins` keeps each plugin as an entity.

- **A plugin** is an entity with a `PluginInfo` (id, name, version, and
  whether it is built in) and an `EntryPoint`. Two plugins of one id are
  not kept: the second is turned away, and the log says so.
- **The entry point** is one function, given an `Entry` to declare what
  the plugin adds: panels, commands, menu entries, shortcuts, settings
  pages and suggested workspaces. It never reaches the app or the world.
  A name in it is the plugin's own; a command is named by its name, or
  by its full id for another's.
- **What it adds** is an entity each, linked to the plugin by
  `ContributedBy`; the plugin's `Contributions` lists them, and goes with
  it (`linked_spawn`). `[budgets] panels` and `contributions` cap how
  much one plugin adds; what is past them, or not well named, is left
  out and told in the log.
- **On and off.** Triggering `Enable` on a plugin runs its entry point
  and adds what it declares, and marks it `Enabled`. `Disable` despawns
  everything it added. Its panels leave the dock but stay in the
  workspaces, and come back when it is switched on again.
- **Built in.** `add_builtin(commands, manifest, entry)` adds a plugin
  linked into the binary and switches it on. A manifest that `check`
  refuses is told in the log and not added.
- **The plugin list** (`engine/plugins`) is the engine's own panel: each
  plugin, its id and version, whether it is on and how much it adds, and
  a button that switches it.
- **The demo** in `engine/app/src/demo.rs` is built into the binary. It
  adds a panel, Hello; a command, Say hello, in a Demo menu and on
  Primary+Shift+H, which writes to the log; and a workspace to take,
  Hello with the console.
