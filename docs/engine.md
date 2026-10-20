# The engine

`engine/` holds `hamlet_engine`, the core that runs plugins and gives
them services. It stands apart from the app in `crates/`: no engine crate
depends on a crate there, and the vendored fynix is the only code they
share.

```sh
cargo run -p hamlet_engine
cargo run --release -p hamlet_engine --features hot-reload
HAMLET_ENGINE_HOME=/tmp/scratch cargo run -p hamlet_engine
cargo run -p hamlet_engine_harness --bin plugin_check -- engine/samples/board
```

## The crates

Each depends only on those above it, and each adds one Bevy plugin,
`plugin`. The binary only adds them.

| Folder | Crate | What it is |
|---|---|---|
| `engine/api` | `hamlet_engine_api` | The names the engine and plugins share. serde only, no Bevy |
| `engine/core` | `hamlet_engine_core` | The tuning files, the home, the log, the budgets |
| `engine/shell` | `hamlet_engine_shell` | The window, its theme, the dock and its panels, workspaces, commands, shortcuts, the menu bar and the status line |
| `engine/plugins` | `hamlet_engine_plugins` | Plugins as entities, what they add, switching them on and off, the plugin list, and the mail between plugins and services |
| `engine/grants` | `hamlet_engine_grants` | What the person lets plugins do: petitions, the grants file, the Permissions page |
| `engine/assets` | `hamlet_engine_assets` | The asset store, the type registry, references and change notification |
| `engine/bus` | `hamlet_engine_bus` | Services and events between plugins, whatever host runs them |
| `engine/net` | `hamlet_engine_net` | The network service: fetches to granted hosts at public addresses |
| `engine/wasm` | `hamlet_engine_wasm` | The host of WASM plugins: wasmtime, a store per plugin, its budgets, and the glue from the WIT to the services. The only crate that knows wasmtime |
| `engine/harness` | `hamlet_engine_harness` | The engine without a window, for tests, and `plugin_check` |
| `engine/app` | `hamlet_engine` | The binary, and a demo plugin built into it |
| `engine/plugin` | `hamlet_plugin` | What a plugin is written with, built for `wasm32-wasip2` |

`engine/tuning/` holds the engine's tuning files. The typefaces are read
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
  was built against (`API_VERSION`, 0.1.0), its asks, and what it shares:
  `[assets]` the types it `makes` and `uses`, `[services]` the services
  it `exports` and `imports`, `[events]` the events it `sends` and
  `hears`. `check` refuses one built against another API, with an empty
  or unreadable text, asking for the same thing twice, naming a type,
  service or event twice, or using a type or importing a service it
  does not ask for.
- **Capabilities.** An ask names a capability, its target and a reason in
  the plugin's words: `net.fetch` a host, `assets.read` a type name,
  `services.call` another plugin's service. Powers that are never granted
  have no name, so no manifest can ask for them.
- **Assets.** An `AssetId` the store gives, a `TypeName` such as
  `weather.forecast` with a version (`AssetType`), and an `AssetInfo`:
  id, type, owner, name and version. The engine never reads inside an
  asset.
- **Services and events** are named by their plugin and their name,
  `hamlet.notes/words` for a `ServiceId` and `hamlet.notes/saved` for an
  `EventId`. A service names the types of its argument and result, an
  event the type of its payload.
- **Requests and messages.** A `Request` is what a plugin asks of the
  engine: an asset made, read, changed, listed, deleted, held or let go,
  a service called, an event subscribed to or sent, a fetch. It is given
  a `Ticket`, and its answer comes later as a `Message::Reply` with the
  ticket: a `Reply`, or a `Failure` (not found, no implementer, refused,
  mismatch, out of budget, failed). The other messages are `Changed`
  and `Gone` for an asset held, and `Event`.
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
  (a panel or settings page by name), and its requests, each giving a
  ticket: `create-asset`, `read-asset`, `update-asset`, `delete-asset`,
  `list-assets`, `hold-asset`, `release-asset`, `call`, `subscribe`,
  `emit` and `fetch`. A ticket of 0 is a request not taken: one call
  made more than `[budgets] requests_per_call`.
- **The guest** exports `declare` (its `Contribution`s), `draw` (the
  `WidgetTree` of a panel or settings page), `event` (a `WidgetEvent`),
  `run` (a command by name), `save` and `restore` (its state, as bytes
  of its own, handed from one version to the next on a reload),
  `receive` (a batch of `Message`s) and `serve` (a call of one of its
  services, by the caller's id, with the result or why it failed).
- **Requests are answered on a later frame.** What a plugin asks in a
  call is passed to the services as the call ends, and their messages
  reach it in one `receive` on the next frame it is served. A plugin's
  stores never meet: the engine copies what passes between them.

## Writing a plugin

`hamlet_plugin` is the guest crate. A plugin is a type with `Default`
that implements `Plugin`, exported with `hamlet_plugin::export!`:
`declare` fills an `Entry` with what it adds, `draw` fills a `Ui` with a
panel's widgets, and `event`, `run`, `save`, `restore`, `receive` and
`serve` have defaults that do nothing. `info`, `warn`, `error` and
`redraw` reach the host; `assets::create`, `read`, `update`, `delete`,
`list`, `hold` and `release`, `call`, `subscribe`, `emit` and `fetch`
make requests, and `encode` and `decode` turn the payloads it shares
into bytes. Its panels are drawn again after each batch it receives.

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

`engine/samples/notes` and `engine/samples/board` share. Notes makes
the type `notes.note` (its bytes are the text) and keeps one note of
it, edited in a field and saved with Save; it exports the service
`words`, which counts the words of a text, and sends `saved` with the
note's id and version. Board uses `notes.note`, holds the note by
reference and shows each version as it is saved, calls `words` for its
count, and sends `seen` with the version it shows, which Notes hears
and shows. Board asks for `assets.read notes.note` and `services.call
hamlet.notes/words`.

## Testing a plugin

`hamlet_engine_harness` runs the engine without a window, on a home of
its own made empty first, with fetches never sent. `Harness` installs
built plugins before the first frame, answers for the person (`allow`,
`answer_use`), runs frames (`frame`, `frames`, `until`), presses
widgets (`click`, `write`, `toggle`), runs commands, reads a panel as a
text outline (`outline`), reads assets, replies to fetches (`reply`),
and reads the log. What it waits for it waits `[harness] patience`
frames at most, `pause` milliseconds apart.

A plugin's `steps.toml` is a script for it: `with` lists other built
plugins it needs, from its folder, and each `[[step]]` does one thing:
`allow`, `admit` (with an answer for every ask), `answer` (a first
use), `frames`, `expect` and `lacks` (text in a panel's outline),
`click`, `write`, `toggle`, `run`, `expect_asset`, `expect_log`, and
`reply` or `fail` for a fetch. A step that expects waits for it; the
script stops at the first that fails.

```sh
cargo run -p hamlet_engine_harness --bin plugin_check -- <folder> [<folder>...] [--steps <file>]
```

checks the built plugins in the folders (each a `plugin.toml` with its
`plugin.wasm` or its crate's release build), with the first folder's
`steps.toml` unless `--steps` names one, and prints each step and, on a
failure, the log. Without steps it lets each plugin in and prints its
panels. An outline is a line for each widget, indented under its
holder: its kind, `#` and its id, and what it shows.

The test `the_notes_and_board_samples_pass_their_steps` (ignored, as it
builds the samples) runs both samples' scripts.

## The core

- **Tuning.** `engine/tuning/engine.toml` holds the sizes, timings and
  budgets, and `theme.toml` the shell's theme. Both are built into the
  binary. A module reads one section of a file as a resource: it
  implements `Tuning` (the file and the section) and adds
  `TuningPlugin<T>`. In a debug build the files in the source tree are
  checked every `[tuning] every` seconds, and a saved change replaces
  each section that differs. A release build is run away from its
  sources, so it watches them only with the `hot-reload` feature. A
  file that cannot be read is told in the log, and the last good values
  stay.
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

- **The window.** `window_plugin` sets Bevy's window from
  `[window]`. Everything shown is built under one root.
- **Fonts.** Inter is the default face. A label tagged `DisplayText`,
  and the name on a dock tab, is drawn in Cinzel, tracked, for titles.
- **The dock and panels.** The dock is fynix's: split areas of tabs,
  dragged, split and closed by hand, and a "+" on each tab bar that opens
  any panel there is. A panel is an entity with a `Panel`: its id, its
  title, and a function that builds its content given the entity. While
  the entity is there the dock can show it; when it goes, its tabs go.
  Its tab shows its title in capitals, after its `PanelIcon` if it has
  one. `panel(view)` gives content the panel's padding and its fill,
  shaded toward the bottom.
- **The console** (`engine/console`) lists the log, newest first.
- **Settings pages** are entities with a `SettingsPage`, as plugins add
  them. The Settings panel (`engine/settings`, opened from the View
  menu) shows a link row to each by title; the page gone through is shown
  under a head whose SETTINGS leads back.

### The theme and styles

The shell looks as the app in `crates/` does: its palette, text sizes,
buttons, cards, headings, notes, icons and chrome. Dark only.

- **The theme file.** `engine/tuning/theme.toml` holds every value
  that decides how the shell looks, read as `EngineTheme`: `[colors]`,
  `[text]` sizes, `[space]`, the `[shadow]` under what floats, `[glow]`
  and `[motion]`. What each value means is beside its field in
  `engine/shell/src/theme.rs`. An unknown or missing key is refused, and
  the last good theme stays. A saved change builds the views again: in a
  debug build always, in a release build with `hot-reload`, since a
  release build is run away from its sources.
- **Colours by role**, never by what they look like: `background` and
  `background_bottom` it shades to, `surface`, `surface_raised`,
  `surface_sunken`, `fill`, `hover`, `accent_hover`, `separator`,
  `border`, `border_strong`, `text`, `text_muted`, `text_accent`,
  `accent`, `accent_soft`, `active` with `active_strong`, `active_low`
  and `active_high`, `selection`, `info`, `critical` with
  `critical_glow`, `critical_strong`, `critical_muted`, `critical_hover`
  and `critical_surface`, `success` with `success_surface`, and
  `shadow`. fynix's tones and surfaces are read from them.
- **Styles** (`style.rs`) build every widget from the theme. A button
  has a `ButtonStyle`: `Plain`, `Primary` (the one action a panel is
  for), `Critical` (what refuses or stops), `Quiet` (lettering only, as
  a way back), `Row` (a row that leads somewhere), `Title` (a menu's
  title) and `Tab`. `action` and `marked` letter one in the display face,
  `marked` after an icon. Besides: `field`, `toggle`, `choices`,
  `heading`, `section`, `caption`, `note`, `card`, `list`, `link_row`
  and `link_rows`, `page_head`, `divider`, `buttons` (a row that wraps),
  `scroll_area`, `title_button` and `tab_button`. A `Shade` surface,
  as a panel or the bar, shades toward `background_bottom`.
- **Glows** are Bevy's `BoxShadow` and `BackgroundGradient` and
  nothing else: no shader of the shell's own. A control glows by what
  it is for (`Glow`): eased toward the pointer, a press or the caret,
  covered while held, pulsing once its work is done, or shaking if it
  refused. The glow is two shadows, a halo and a brighter core, set a
  little below the control (`drop`); a `Quiet` button glows about its
  lettering. The primary action and a field being typed in are also
  lit inside, by a gradient from the active colour at the bottom to
  nothing at the top (`inner`). A title or a tab is lit the same way
  (`BottomLight`), under the pointer and while its menu is open or it is
  shown. A scroll area cuts what it holds as far past its edges as a
  glow reaches (`GlowMargin`), and a panel cuts at the outer edge of
  its padding, so a glow at the edge shows whole. What floats has
  `drop_shadow`.
- **Plugin panels** are drawn by `tree_view` with the same styles.
- **Icons** (`icons.rs`) are one family of line drawings made in code
  at start, so there is no file or font to load: a `Glyph` each, drawn
  at 12, 14, 16 and 20 pixels and tinted by a tone. A plugin names one
  by `Glyph::name`.
- **Stills.** `cargo run -p hamlet_engine --example stills -- --out
  <dir>` draws the widgets in each state, the icons, and the shell with
  a menu open or Settings shown, into PNGs with no window; `--scene
  <name>` draws one.

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
  the top, its name in capitals, the one shown lit and underlined; a
  press shows that workspace. "+" adds an empty one, named
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
  others in the order they came. A menu's title, in capitals, opens it
  under itself and stays lit while it is open; a press on another title
  opens that one instead. An entry shows its command's label and first
  key. An entry whose command is not there is left out.
- **The engine's own.** Quit (Cmd+Q, Ctrl+Q off macOS) in the Hamlet
  menu, the workspace commands, and for each panel a command in the View
  menu that brings it to the front, or opens it in the first area.
- **The status line** at the bottom of the window shows the workspace
  shown and the log's last line, each behind an icon, a warning in the
  accent and an error in red.

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
  why it was stopped, and SWITCH ON or SWITCH OFF; an installed one has
  UNINSTALL too (`Uninstall`). ADD PLUGIN... above them
  triggers `AddPlugin`, for a host to answer.
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
  child of the plugin's entity, so they go with it. The Petitions panel
  (`engine/petitions`) says how many wait over a card for each, and is
  opened in the dock when one comes; the log says so too.
- **Installing.** A plugin that comes with no answers is asked about:
  its name and version, and each ask with its reason and a choice of
  the three answers, Ask next time to start with. Allow lets it in with
  those answers, marks it `Admitted` and switches it on; Don't allow
  keeps it off, and switching it on asks again.
- **An update that asks for more** is asked about again, for what it
  asks besides, and stays off until the person answers. One that asks
  for nothing new is let in as before.
- **First use.** A service that finds a use answered Ask next time
  (`Grants::verdict` gives `Asking`) sends a `FirstUse`; the person is
  asked, and `Decided` says whether the use may go on. Allow and Don't
  allow are kept; Ask next time lets this use go on and asks again next
  time. `[budgets] petitions_per_minute` of these a plugin may raise in
  a minute; the rest are refused. The asset store, the bus and the
  network keep the request meanwhile in a `Waiting`, and answer it once
  the person did: reading another's type, calling a service and
  fetching from a host are each asked about this way.
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
- **Watched.** The plugins folder is checked every `[wasm] watch`
  seconds. A folder that appeared is added once its `plugin.wasm` and
  `plugin.toml` stayed the same from one check to the next, so one still
  being copied in is not read, and the person is asked about it; one
  that cannot be is told of once, and again when it changes. A plugin
  whose folder is gone goes with it.
- **ADD PLUGIN...** on the Plugins panel asks for a plugin to be added
  (`AddPlugin`). The host opens the system's dialog on the main thread
  and waits for it off the frame. The folder chosen, or one given by
  `AddFrom` as a test does, is checked: a `plugin.toml` the engine can
  take and a `plugin.wasm`, each a plain file within its size, not a
  link, and the folder neither in the home nor holding it. A folder of
  sources is refused; plugins are not built here. The two files are
  copied off the frame into `<home>/plugins/.adding_<id>`, read again,
  and the folder is moved into place: `<home>/plugins/<id>`, or the
  folder of the installed plugin of that id, which it replaces. The
  person is asked about it at once. An id of a plugin built in is
  refused.
- **Uninstall** on the Plugins panel (`Uninstall`) takes an installed
  plugin away with its folder, and the grants forget what the person
  answered for it.
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
- **Requests and mail.** What a plugin asked in a call is written as
  an `Asked` for the services once the call ends, `depth` one more than
  the work it was serving. A `Deliver` for it goes into its inbox, and
  the messages at the front of its inbox are handed over together in
  one `receive`, as many as fit in `bytes_per_call`; a reply too large
  to fit at all is handed over as a failure. A call of one of its
  services (a `Serve`) goes into its inbox too, and `serve` answers it
  with a `Served`. Its tickets go on counting across a reload.

### Hot reload

- **Noticed.** Every `[wasm] watch` seconds each installed plugin's
  `plugin.wasm` and `plugin.toml` are checked, and a change is taken
  once a file stayed the same from one check to the next.
- **A new `plugin.wasm`** of a running plugin is compiled off the frame
  and swapped in by one system in the frame's update, never in the
  middle of a call: the running version's `save` gives its state, the
  new one is instantiated in a store of its own (the pool keeps room
  for it), given the state by `restore`, and asked what it adds. Its
  inbox and its second's spending carry over. When it adds the same,
  its panels are drawn again; when not, what it added is taken away and
  added anew. The log says it was reloaded.
- **A build that fails**, or a new version at fault, leaves the old one
  running, and the log says why; the same file is not tried again until
  it changes. A running version at fault while it saves is stopped.
- **A plugin that is not running** forgets its compiled component, so
  it is compiled anew when it starts.
- **A new `plugin.toml`** that changes the name, the version or the
  asks adds the plugin again, as if it had just come: one that asks for
  more is asked about again. Its state is not kept.

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
- **The inbox** holds widget events, commands, messages and calls to
  serve, `events_per_second` at most; more are left out, and the log
  says how many. A call left out answers its caller out of budget.
- **A service's call is paid by its caller.** The fuel and time its
  plugin spent serving it are charged to the caller's
  `service_fuel_per_second` and `service_time_per_second`, not to its
  own second; its own counts the call.
- **The log.** A plugin's lines of each level, info, warn and error,
  are `log_lines_per_second` a second at most; the rest are left out,
  and the log says how many when the second ends.

### Panels

- **Drawn from a tree.** A WASM plugin's panels and settings pages are
  the same `Panel` and `SettingsPage` as a built-in plugin's, and carry
  a `Drawn`: their name within the plugin and what they show. The
  plugin's `draw` gives a `WidgetTree`, checked against the budgets'
  `TreeLimits`, and the shell's `tree_view` draws it with its own
  widgets. An icon is one of the engine's set by its name (`plus`,
  `check`, `bell`, ... as `Glyph::name` has them); a name the set does
  not have is shown as it is.
- **Events back.** A click, a toggle switched, or a field's text (on
  Enter, or as the field is left) goes into the plugin's inbox as a
  `WidgetEvent`, and the plugin's `event` is called with it on the next
  frame.
- **Drawn again** only when a panel comes, when the plugin asks with
  `redraw`, or after the plugin was given an event, ran a command,
  received messages or served a call; then each of its
  panels is drawn again, and one whose tree did not change is left as
  it is.
- **Stopped.** A plugin at fault is stopped: its store goes, it is
  marked `Stopped` with why, the log says so, and its panels show that
  it was stopped and why. Switching it off and on starts it afresh.

## Assets

`hamlet_engine_assets` keeps the asset store and answers what plugins
ask of it, in `PostUpdate`.

- **The store** is `<home>/assets/`: the bytes of each asset's version
  in `<id>_<version>.bin`, and `index.toml` with each asset's
  `AssetInfo` and the next id. Each file is written beside its place
  (`saving_<name>`) and moved over it, the bytes before the index, so
  either is whole or not at all; the bytes of the version before are
  taken away after. An index that cannot be read is left as it is: the
  log says so, nothing is written until the engine starts with one
  that can be, and every write fails saying why.
- **The type registry** (`TypeRegistry`) is each type a plugin there is
  makes, with its version. A type belongs to the plugin that makes it;
  one that two make is the one whose id comes first, and the log tells
  the other.
- **Who may do what.** A plugin makes assets only of the types it
  makes, and changes and deletes only its own. It reads, lists and
  holds its own types, and another's that its manifest uses at the
  version its maker makes, once the person allowed `assets.read` of
  that type. A type of another version is a mismatch; one no plugin
  makes is refused.
- **Caps** (`[budgets]`): `asset_bytes` for one asset, less than
  `bytes_per_call` so a read one can be handed over; `assets` it may
  own, and as many it may hold; `asset_writes_per_second` made,
  changed or deleted.
- **References.** A plugin that holds an asset (`Holding` on its
  entity) is sent `Changed` with the version each time it changes, and
  `Gone` when it is deleted. What it holds is let go when it is
  switched off.

## Services

`hamlet_engine_bus` sits between plugins, and knows nothing of how they
run.

- **Declared.** A plugin exports a service by its name and the names
  of its argument's and result's types; a caller imports it by its full
  id with the types it expects, and asks for `services.call` on it.
- **A call** is checked in this order: the caller imports it, does not
  call itself, the chain is `[budgets] chain_depth` plugins deep at
  most, the caller is within its `service_calls_per_second`,
  `service_fuel_per_second` and `service_time_per_second`, the person
  allowed it (or is asked first), a running plugin exports it (else no
  implementer), and both sides name the same types (else a mismatch).
  The bus then hands the plugin a `Serve` and remembers the call
  (`Calls`).
- **Answered.** The result of `serve`, or why it failed, comes back as
  `Answered` or a failure; a plugin out of its fuel or time for the
  call leaves the caller out of budget and is stopped, as any plugin
  at fault is. A call whose plugin stops before it serves it fails.
- **Asynchronous.** Each plugin has a store of its own, and a host
  call cannot reach another store while its own is in use, so a call
  is not made within the caller's call: it goes through the callee's
  inbox, served on the next frame, and its answer through the caller's
  on the frame after. The depth a request is made at follows the chain
  of messages, so calls that call back and forth stop at
  `chain_depth`.

## Events

- **Declared.** A plugin sends events its manifest names with the type
  of their payload, and hears another's by its full id, with the type
  it expects.
- **Subscribed** by name at run time (`subscribe`), to an event its
  manifest hears; `Subscriptions` on its entity, let go when it is
  switched off.
- **Sent** with `emit`, `[budgets] events_sent_per_second` a second at
  most and no deeper than `chain_depth`; a refusal is replied to, a
  sent event is not. Events need no grant: a plugin hears only what it
  declares, from the plugin that names it.
- **Delivered** together: the events of a frame go to each subscriber
  that names the payload's type as the sender does, and reach it in
  one batch on the next frame.

## The network

`hamlet_engine_net` makes fetches for plugins, off the frame, on a
runtime of its own.

- **Asked for by host.** A plugin asks for `net.fetch` on each host it
  needs (`api.example.org`, or `host:port` for a port other than 443),
  and the person answers for each. A fetch goes only to an `https`
  address with no user on such a host, once allowed; the first use of
  one answered Ask next time asks the person.
- **Public addresses only.** The engine resolves the host, and the
  fetch is pinned to the public addresses of the answer: loopback,
  private, link-local, unique-local, shared, multicast, broadcast and
  unspecified addresses are refused, as is a host that is one of them.
  No redirect is followed and no proxy is used, so it cannot go
  elsewhere.
- **Caps** (`[budgets]`): `fetches_per_minute`, `fetches_waiting` at
  once, `fetch_bytes` of an answer, `fetch_time` seconds for the whole.
- **A fetch** is a `Fetch` on a child of its plugin, so it goes with
  it. With `Offline` in the app, as the harness has it, fetches are
  never sent and wait for `answer`.

## Where the budgets are

Every cap is a field of `Budgets`, `[budgets]` in
`engine/tuning/engine.toml`, its meaning beside its field in
`engine/core/src/budget.rs`. The host's set-up is `[wasm]`, and the
harness's waiting `[harness]`.
