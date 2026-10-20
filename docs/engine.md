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
| `engine/grants` | `hamlet_engine_grants` | What the person lets plugins do: petitions, the grants file, the Permissions page |
| `engine/wasm` | `hamlet_engine_wasm` | The host of WASM plugins: wasmtime, a store per plugin, its budgets. The only crate that knows wasmtime |
| `engine/app` | `hamlet_engine` | The binary, and a demo plugin built into it |
| `engine/plugin` | `hamlet_plugin` | What a plugin is written with, built for `wasm32-wasip2` |

`engine/assets/` holds the engine's tuning files. The typefaces are read
from the repository's `assets/fonts/`, where their licences are.
`engine/wit/hamlet_engine.wit` is the interface between the engine and
a plugin. `engine/samples/` holds sample plugins, each a workspace of
its own.

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
- **What a plugin adds** is a `Contribution` each: a panel, a command,
  a menu entry, a shortcut, a settings page or a suggested workspace.
- **A widget event** is what the person did to a widget of a plugin's
  panel: a click, a toggle switched, or a field's text.
- **The wire format** is postcard, through `encode` and `decode`.
  `decode` refuses bytes left over after the value.

## The interface

`engine/wit/hamlet_engine.wit` is the package `hamlet:engine@0.1.0`.
Values that are `list<u8>` are wire-format bytes of the vocabulary's
types, so the WIT names nothing twice.

- **The host** gives a plugin `log` (a level and a line) and `redraw`
  (a panel or settings page by name).
- **The guest** exports `declare` (its `Contribution`s), `draw` (the
  `WidgetTree` of a panel or settings page), `event` (a `WidgetEvent`),
  `run` (a command by name), and `save` and `restore` (its state, as
  bytes of its own, handed from one version to the next on a reload).

## Writing a plugin

`hamlet_plugin` is the guest crate. A plugin is a type with `Default`
that implements `Plugin`, exported with `hamlet_plugin::export!`:
`declare` fills an `Entry` with what it adds, `draw` fills a `Ui` with a
panel's widgets, and `event`, `run`, `save` and `restore` have defaults
that do nothing. `info`, `warn`, `error` and `redraw` reach the host.

- **The `Ui`** adds widgets in order, under a column the engine is given
  with the empty id. Each widget has an id of the plugin's, unique in
  its panel. `list`, `row`, `column` and `scroll` take a closure that
  adds what they hold.
- **The system.** The standard library's calls to the system, such as
  the clock, randomness or printing, stop the plugin. A `HashMap` needs
  randomness, so a plugin keeps a `BTreeMap`.
- **A built plugin** is a folder with its `plugin.toml` and its
  component, `plugin.wasm`.

`engine/samples/hello` is a panel with a button and a count of its
presses, and a command in its own menu and on Primary+Shift+R that
sets the count back. It
keeps its count through `save` and `restore`. Its crate's library is
named `plugin`, so in its folder

```sh
cargo build --release --target wasm32-wasip2
```

builds `target/wasm32-wasip2/release/plugin.wasm`, to go beside its
`plugin.toml`.

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
  them. The Settings panel (`engine/settings`, opened from the View
  menu) lists them by title beside the page chosen.

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
- **Stopped.** A plugin the engine stopped while it was on carries
  `Stopped` with why; what it added stays until it is switched off.
- **Built in.** `add_builtin(commands, manifest, entry)` adds a plugin
  linked into the binary and switches it on. A manifest that `check`
  refuses is told in the log and not added.
- **The plugin list** (`engine/plugins`) is the engine's own panel: each
  plugin, its id and version, whether it is on and how much it adds, or
  why it was stopped, and a button that switches it.
- **The demo** in `engine/app/src/demo.rs` is built into the binary. It
  adds a panel, Hello; a command, Say hello, in a Demo menu and on
  Primary+Shift+H, which writes to the log; and a workspace to take,
  Hello with the console.
- **`switch_on`** adds what an `Entry` declares and marks the plugin on.
  A built-in plugin's entry point runs through it, and so does a WASM
  plugin once it has said what it adds.
- **A command's function** is given the world and the command's
  entity, whose `Command` and `ContributedBy` say which it is and
  whose.

## Permissions

`hamlet_engine_grants` keeps what the person lets each installed plugin
do. Plugins built in are not asked about.

- **Asks.** A plugin's manifest names each capability it asks for, its
  target and a reason in its own words, shown with it. Each is answered
  Allow, Ask next time or Don't allow. Powers that are never granted
  (keys, secrets, answering for the person, the raw home, running
  processes, turning the sandbox off) are refused when a manifest
  asking for one is read: `never_granted` names them by the first word
  of a capability.
- **Petitions** are questions for the person, each a `Petition` on a
  child of the plugin's entity, so they go with it. The Needs you panel
  (`engine/needs_you`) lists them, and is opened in the dock when one
  comes; the log says so too.
- **Installing.** A plugin that comes with no answers is asked about:
  its name and version, and each ask with its reason and a choice of
  the three answers, Ask next time to start with. Allow lets it in with
  those answers, marks it `Admitted` and switches it on; Don't allow
  keeps it off, and switching it on asks again.
- **An update that asks for more** is asked about again, for what it
  asks besides, and stays off until the person answers. One that asks
  for nothing new is let in as before.
- **First use.** A host that finds a use answered Ask next time
  (`Grants::verdict` gives `Asking`) sends a `FirstUse`; the person is
  asked, and `Decided` says whether the use may go on. Allow and Don't
  allow are kept; Ask next time lets this use go on and asks again next
  time. `[budgets] petitions_per_minute` of these a plugin may raise in
  a minute; the rest are refused.
- **The grants file** is `grants.toml` in the home: for each plugin,
  whether it was let in, the version answered for, and each answer. It
  is written to `grants_saving.toml` and moved over the old file, so it
  is whole or not at all. A file that cannot be read is left as it is:
  the log says so, and answers are kept only until the engine closes.
- **The Permissions page** of Settings shows the answers by plugin or
  by capability, and each can be changed there.

## The WASM host

`hamlet_engine_wasm` runs plugins built as WASM components, with
wasmtime. `[wasm]` in `engine.toml` sets it up; the budgets it holds a
plugin to are `[budgets]`.

- **Installed plugins** are folders in `<home>/plugins/`, each with its
  `plugin.toml` and `plugin.wasm`. At start each is read and added as a
  plugin, with a `Package` naming its folder; the grants switch it on
  once the person let it in. A folder whose name starts with a dot, or
  that is a link, is passed over. A file is read only when it is a
  plain file and not a link, and its size is checked before a byte of
  it is read: `[wasm] manifest_bytes` and `wasm_bytes`. A manifest that
  asks for a power that is never granted is refused, and says which.
- **Switched on**, only when it is `Admitted`, a plugin's component is
  compiled off the frame, on
  the async compute pool, and kept, so switching it on again needs no
  compile. It is then instantiated in a store of its own and asked what
  it adds. A plugin that imports anything but the host's interface and
  the system's (`wasi:`) is not loaded, and one built against another
  version of the interface is told so. The system's functions trap.
- **What it adds** comes back from `declare` as `Contribution`s and is
  made into the same `Entry` a built-in plugin fills, so it is added,
  capped and taken away as theirs is: its panels and settings pages are
  drawn from its trees, and its commands put their name in its inbox,
  to run it on the next frame. Its menu entries, shortcuts and suggested
  workspaces are plain data.
- **Switched off**, its store goes, and with it its state.
- **The engine** is one for all plugins, with fuel, an epoch and a pool.
  A thread ticks the epoch every `[wasm] tick` milliseconds. The pool
  has room for `[wasm] plugins` plugins and one more, for a version
  built beside a running one; each plugin's share of it is capped: its
  core instances, memories and tables, and each table's elements.
- **Every call** into a plugin is given `[budgets] fuel_per_call` fuel,
  an epoch deadline of `call_time` milliseconds, and `bytes_per_call`
  bytes it may hand over. A string or a list from the plugin is checked
  against its memory, and its length taken from those bytes, before
  wasmtime copies any of it. What the host hands a plugin is checked
  against the same bytes before the call.
- **Memory** is counted over all of a plugin's memories together, held
  to `[budgets] memory`.
- **A fault** is a call over its fuel, its time, its memory or its
  bytes, a trap, or what cannot be read; a plugin cannot be called
  again after one.
- **The log.** What a plugin logs goes to the engine's log under its
  id, each line cut at `log_line_bytes`.

### Budgets

- **A second's budgets.** Every call into a running plugin, of any
  kind, counts towards what it spends in a second: `time_per_second`
  milliseconds, `fuel_per_second` fuel, `calls_per_second` calls, of
  which `events_per_second` events. A plugin past one waits: what is in
  its inbox and its panels to draw stay for the next second.
- **Stopped when it keeps going over.** A plugin that went past a
  second's budget `strikes` seconds in a row is stopped, and the log
  says why. The first second it goes over, the log says it waits.
- **The frame's share.** The calls of all the plugins in one frame
  share `frame_share` milliseconds. The plugin served longest ago goes
  first, and what is left when the share is spent waits for the next
  frame, when those left go first. One panel is drawn each frame
  whatever the share, so a slow plugin is drawn in time.
- **Events** wait in a plugin's inbox, a second's worth at most; more
  are left out, and the log says how many.
- **The log.** A plugin's lines of each level, info, warn and error,
  are `log_lines_per_second` a second at most; the rest are left out,
  and the log says how many when the second ends.

### Panels

- **Drawn from a tree.** A WASM plugin's panels and settings pages are
  the same `Panel` and `SettingsPage` as a built-in plugin's, and carry
  a `Drawn`: their name within the plugin and what they show. The
  plugin's `draw` gives a `WidgetTree`, checked against the budgets'
  `TreeLimits`, and the shell's `tree_view` draws it with its own
  widgets. The engine has no set of icons yet: an icon shows its name.
  A field's placeholder is written above it.
- **Events back.** A click, a toggle switched, or a field's text (on
  Enter, or as the field is left) goes into the plugin's inbox as a
  `WidgetEvent`, and the plugin's `event` is called with it on the next
  frame.
- **Drawn again** only when a panel comes, when the plugin asks with
  `redraw`, or after the plugin was given an event or ran a command;
  then each of its
  panels is drawn again, and one whose tree did not change is left as
  it is.
- **Stopped.** A plugin at fault is stopped: its store goes, it is
  marked `Stopped` with why, the log says so, and its panels show that
  it was stopped and why. Switching it off and on starts it afresh.
