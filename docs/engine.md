# The engine

`engine/` holds `hamlet_engine`, the core that runs plugins and gives
them services. It stands apart from the app in `crates/`: no engine crate
depends on a crate there, and the vendored fynix is the only code they
share.

```sh
scripts/build_plugins.sh
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
| `engine/core` | `hamlet_engine_core` | The tuning files, the home, the bundle's folder, the log, the budgets |
| `engine/shell` | `hamlet_engine_shell` | The window, its theme, the dock and its panels, workspaces, commands, shortcuts, the menu bar and the status line |
| `engine/plugins` | `hamlet_engine_plugins` | Plugins as entities, what they add, switching them on and off, the plugin list, and the mail between plugins and services |
| `engine/grants` | `hamlet_engine_grants` | What the person lets plugins do: petitions, the grants file, the Permissions page |
| `engine/assets` | `hamlet_engine_assets` | The asset store, the type registry, references and change notification |
| `engine/bus` | `hamlet_engine_bus` | Services and events between plugins, whatever host runs them |
| `engine/net` | `hamlet_engine_net` | The network service: fetches to granted hosts at public addresses |
| `engine/agents` | `hamlet_engine_agents` | The agents service: the agent core run in the engine, behind a protocol |
| `engine/desktop` | `hamlet_engine_desktop` | The desktop service: the machine kept awake while an agent works, addresses and files opened for plugins, the address under the pointer, one engine per home |
| `engine/render` | `hamlet_engine_render` | What plugins draw: scenes kept entity by entity, their views in a viewport or behind the dock, picking, and plugins' own WGSL checked with naga, their compute and passes, held to their GPU time |
| `engine/wasm` | `hamlet_engine_wasm` | The host of WASM plugins: wasmtime, a store per plugin, its budgets, and the glue from the WIT to the services. The only crate that knows wasmtime |
| `engine/harness` | `hamlet_engine_harness` | The engine without a window, for tests, and `plugin_check` |
| `engine/app` | `hamlet_engine` | The binary, and a demo plugin built into it |
| `engine/sdk` | `hamlet_engine_sdk` | What a plugin is written with, built for `wasm32-wasip2` |

The agent core is in `engine/` too, under its old crate names, so the
engine runs it without reaching into `crates/`; the app in `crates/`
depends on it from here.

| Folder | Crate | What it is |
|---|---|---|
| `engine/sandbox` | `hamlet_sandbox` | Per-platform process sandboxes |
| `engine/model` | `hamlet_model` | The model client, its settings, the catalogue of models, the session log |
| `engine/home` | `hamlet_home` | A home on disk: its layout, charter and prompt, the chief's limits, the starter skills |
| `engine/rules` | `hamlet_rules` | The hamlet's record and the rules over it, no Bevy |
| `engine/agent_core` | `hamlet_core` | The agents' runtime as Bevy plugins: agents, sessions, tools and oversight |
| `engine/testing` | `hamlet_testing` | A scratch folder for their tests |

`engine/tuning/` holds the engine's tuning files. The typefaces are read
from the repository's `assets/fonts/`, where their licences are.
`engine/wit/hamlet_engine.wit` is the interface between the engine and
a plugin. `engine/samples/` holds sample plugins, each a workspace of
its own. `plugins/` at the repository root holds the plugins that come
with Hamlet (see [Bundled plugins](#bundled-plugins)).

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
  `hears`, and `[limits]` the limits it needs in place of the
  engine's (see [Limits](#limits)). `check` refuses one built against
  another API, with an empty or unreadable text, asking for the same
  thing twice, naming a type, service or event twice, using a type or
  importing a service it does not ask for, or asking for a limit of
  zero or below.
- **Capabilities.** An ask names a capability, its target and a reason in
  the plugin's words: `net.fetch` a host, `assets.read` a type name,
  `services.call` another plugin's service, and `agents.read`,
  `agents.chief`, `agents.settings` and `agents.keys` the target `local`
  (see [The agents](#the-agents)), `desktop.open` the target `https` or
  a folder, and `desktop.settings` the target `local` (see
  [The desktop](#the-desktop)), and `render.scene`, `render.shader`
  and `render.compute` the target `gpu` (see
  [Scenes and GPU work](#scenes-and-gpu-work)). Powers that are never
  granted have no name, so no manifest can ask for them.
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
  a service called, an event subscribed to or sent, a fetch, a request
  of the agents or of the desktop. It is given
  a `Ticket`, and its answer comes later as a `Message::Reply` with the
  ticket: a `Reply`, or a `Failure` (not found, no implementer, refused,
  mismatch, out of budget, failed). The other messages are `Changed`
  and `Gone` for an asset held, and `Event`.
- **What a plugin adds** is a `Contribution` each: a panel, a command,
  a menu entry, a shortcut, a settings page (with its icon and a line
  on what it holds), a settings footer, a suggested workspace, a
  status item (one that grows or not) or an overlay.
- **The wire format** is postcard, through `encode` and `decode`.
  `decode` refuses bytes left over after the value.

## The interface

`engine/wit/hamlet_engine.wit` is the package `hamlet:engine@0.1.0`.
Values that are `list<u8>` are wire-format bytes of the vocabulary's
types, so the WIT names nothing twice.

- **The host** gives a plugin `log` (a level and a line), and its
  requests, each giving a ticket: `create-asset`, `read-asset`,
  `update-asset`, `delete-asset`, `list-assets`, `hold-asset`,
  `release-asset`, `call`, `subscribe`, `emit`, `fetch`, `agents` and
  `desktop`. A ticket of 0 is a request not taken: one call made more than
  `[limits] requests_per_call`.
- **The guest** exports `declare` (its `Contribution`s), `run` (a
  command by name), `save` and `restore` (its state, as bytes of its
  own, handed from one version to the next on a reload), `receive` (a
  batch of `Message`s) and `serve` (a call of one of its services, by
  the caller's id, with the result or why it failed).
- **Requests are answered on a later frame.** What a plugin asks in a
  call is passed to the services as the call ends, and their messages
  reach it in one `receive` on the next frame it is served. A plugin's
  stores never meet: the engine copies what passes between them.
- **Panels, settings pages, settings footers and overlays** are the
  world `ui-plugin`: `plugin` with the interface `ui`, whose resource
  `node` the plugin makes,
  last under a parent that holds nodes, with a key of its own;
  `set`s a property of (at once, or over a `motion` where it can
  move); `reorder`s; `watch`es for hover and press; and drops to take
  it away with what is under it. And the export `ui-guest`: `mount` (a
  panel, settings page, footer or overlay to build under a root node
  the engine gives), `unmount`, `input` (what the person did, by key)
  and `frame`, each answering whether something still moves.
- **Besides the nodes**, `ui` has `set-status` (what one of the
  plugin's status items shows: an icon by name, words, a tone and
  words under the pointer), `show-overlay` (one of its overlays shown
  or hidden) and `open` (one of its panels brought to the front, or
  one of its settings pages shown in Settings; or, by a full id
  `plugin.id/name`, a panel of any plugin brought to the front, which
  needs no capability since it changes only the dock). Each counts
  as a
  change to a node towards the budgets.
- **The kinds of node**: `frame` (a column or a row), `label`,
  `button` (showing its text, or holding an icon and a label as an
  icon button does), `icon` (one of the engine's set, by name),
  `toggle`, `field` (text to edit), `list` (rows with a divider
  between each), `scroll` (a column that scrolls), `divider` and
  `dropdown` (one of its options shown, opening a list to pick
  another). A frame, a button, a list and a scroll hold nodes.
- **The properties**, each of the kinds it names: `text`, `tone`,
  `size`, `bold`, `face` (the body's or the display face), `opacity`,
  `offset`, `tip` and `menu` (of every kind; a tip is words shown
  near the pointer while it rests on the node, and a menu its items,
  opened at the pointer on a right-click and, when it says so, on a
  click, an item with a `group` under a row of that name that opens
  them beside it), `style` and `folder` (a button's; a button with a
  folder's title opens the system's dialog to choose a folder when
  it is pressed, in place of a click),
  `direction`, `align` (its children at its start or its end),
  `fill` (none, a card, the accent's or a disc, by the theme's
  roles; a filled frame is padded and no wider than what holds it)
  and
  `space` (a frame's), `placeholder` and `lines` (a field's: the most
  lines it grows to, one to start with), `on` (a toggle's), and
  `options` and `selected` (a dropdown's). A field of more than one
  line is for a message: Enter sends what is typed and empties it,
  Shift+Enter adds a line, and Escape keeps what is typed and leaves
  it. A scroll's `stick` keeps it to its end as it grows, until the
  person scrolls up, and again once they are back at its end; what
  is put above it after it reached its top leaves the rows shown
  where they were. A tone, a size and a
  space are roles, filled in from the theme as it is now. A tone,
  size, opacity, space or offset can move. Setting a property a
  node's kind does not have, or making a node under one that holds
  none, fails the call.
- **What the person does**: a click, the pointer over a node or off
  it, a press or a release, a toggle switched (`toggled`, with the
  state asked for), a field's text as it was left (`committed`), a
  dropdown's option picked (`picked`, by its place), what Enter
  sent from a field of several lines (`submitted`), and a scroll that
  sticks scrolled to its top, or holding too little to scroll, once
  for each height of what it holds (`reached-top`), and an item of a
  node's menu chosen (`chose`, by its place), and the folder chosen
  in the dialog a button opened (`chosen`, by its path).

## Writing a plugin

`hamlet_engine_sdk` is the guest crate. A plugin is a type with
`Default` that implements `Plugin`, exported with
`hamlet_engine_sdk::export!`: `declare` fills an `Entry` with what it
adds, `mount` gives the view of a panel or settings page, and `run`,
`save`, `restore`, `receive` and `serve` have defaults that do
nothing. Each is given the plugin's `World`, so a command, a message
or a call served can change what its views show. `info`, `warn` and
`error` reach the host; `assets::create`, `read`, `update`, `delete`,
`list`, `hold` and `release`, `call`, `subscribe`, `emit`, `fetch`,
`agents` and `desktop` make requests, and `encode` and `decode` turn the
payloads it shares into bytes. `ENGINE_VERSION` is the version of
the engine the SDK comes with.

- **Scenes** are `hamlet_engine_sdk::scene`: the handles of the
  `scene` interface, `Transform::looking_at`, `Pbr::colored`, the
  default `CameraSettings`, `copies` for instances, `animate`, the
  view `viewport`, and `scene::gpu` for shaders, buffers, textures,
  compute and passes. What the person does in a view comes to
  `Plugin::scene`, and each frame while it animates to `Plugin::tick`
  (see [Scenes and GPU work](#scenes-and-gpu-work)).
- **Views** are written with fynix (`hamlet_engine_sdk::ui`). `Hamlet`
  is fynix's backend: a node is made in the engine as fynix spawns
  it, of the kind of its element, and a property still at its first
  value is not sent. The elements are `label` (and `heading`),
  `button`, `icon_button` (an icon and a label in a button), `icon`,
  `toggle`, `field`, `dropdown`, `divider`, and the holders `column`,
  `row`, `list` and `scroll`; `keyed` builds a view again when its
  key changes, and `each` a view per item of a list. `on_click`,
  `on_toggle`, `on_commit`, `on_submit`, `on_pick` and `on_top` run a
  handler
  with the world, a node holding as many as it is given, and `when::<Hovered, _>` and `when::<Pressed, _>` hold rules
  while the engine says so. `tip` gives a view words shown near the
  pointer while it rests on it, and `menu` (on a right-click) and
  `menu_on_click` (on a click too) a menu of `menu_item`s, each
  running a handler when it is chosen, `under` a group or `critical`.
  A button's `folder` and `on_folder` have the person choose a
  folder in the system's dialog. `leads_to` gives a view an address that
  the plugin tells the desktop service of (`Hover`) while the pointer
  is over it, so the status line shows it. `set_status`,
  `show_overlay` and `open` change what the plugin added outside its
  views; `Entry::growing_status_item` adds a status item that takes
  the room left on the line.
  `transition` hands each change of a tone, size, opacity, space or
  offset to the engine with its motion, once, and the engine moves it
  over its own theme's curve; the plugin is not called each frame for
  it.
- **The `World`** holds the plugin's resources, which `resource`
  signals read: `insert`, `init_resource`, `resource`,
  `resource_mut`, `resource_or_init` and `get`. What a call changes
  in it is shown once the call ends. `mount` builds a view each time
  the panel or page is shown, from the world as it is.
- **The system.** A plugin is given WASI 0.2 that grants nothing but
  the wall and monotonic clocks and randomness, so the standard
  library's `Instant`, `SystemTime` and `HashMap` work. What it prints
  goes to its log, stdout as info and stderr as warnings. It has no
  folders, no environment or arguments, an empty stdin, and no sockets
  or lookups; the network is `fetch`.
- **A built plugin** is a folder with its `plugin.toml` and its
  component, `plugin.wasm`.

`engine/samples/hello` is a panel with a button and a count of its
presses, a resource of its world, and a command in its own menu and
on Primary+Shift+R that sets the count back. It keeps its count
through `save` and `restore`. Its crate's library is named `plugin`,
so in its folder

```sh
cargo build --release --target wasm32-wasip2
```

builds `target/wasm32-wasip2/release/plugin.wasm`, to go beside its
`plugin.toml`.

`engine/samples/fynix_counter` is a panel of about fifty
nodes: a count and a button that adds one, a button that is primary
under the pointer, and a label the engine fades and dims. `cargo run
--release -p hamlet_engine_harness --example retained_costs -- <home>
engine/samples/fynix_counter` prints what it costs the host, frame by
frame.

`engine/samples/notes` and `engine/samples/board` share, each
keeping what it shows as a resource of its world and changing it as
messages come. Notes makes the type `notes.note` (its bytes are the
text) and keeps one note of it, edited in a field and saved with
Save, an icon button; it exports the service `words`, which counts
the words of a text, and sends `saved` with the note's id and
version. Board uses `notes.note`, holds the note by reference and
shows each version as it is saved in a list, calls `words` for its
count, and sends `seen` with the version it shows, which Notes hears
and shows. Board asks for `assets.read notes.note` and
`services.call hamlet.notes/words`.

## Testing a plugin

`hamlet_engine_harness` runs the engine without a window, on a home of
its own made empty first, with fetches never sent. `Harness` installs
built plugins before the first frame, answers for the person (`allow`,
`answer_use`), runs frames (`frame`, `frames`, `until`), acts on
widgets (`click`, `write`, `toggle`, `hover`), runs commands, reads a
panel as a
text outline (`outline`), reads assets, replies to fetches (`reply`),
and reads the log. What it waits for it waits `[harness] patience`
frames at most, `pause` milliseconds apart.

A plugin's `steps.toml` is a script for it: `with` lists other built
plugins it needs, from its folder, and each `[[step]]` does one thing:
`allow`, `admit` (with an answer for every ask), `answer` (a first
use), `frames`, `expect` and `lacks` (text in a panel's outline),
`click`, `write`, `toggle`, `run`, `expect_asset`, `expect_log`,
`reply` or `fail` for a fetch, `expect_scene` (how many entities,
meshes, materials and cameras a plugin's scene holds) and `pick` (a
click on the entity of a key, and the copy of instances). A step that
expects waits for it; the script stops at the first that fails. The
harness has no GPU: a scene is kept and counted, never drawn.

```sh
cargo run -p hamlet_engine_harness --bin plugin_check -- <folder> [<folder>...] [--steps <file>]
```

checks the built plugins in the folders (each a `plugin.toml` with its
`plugin.wasm` or its crate's release build), with the first folder's
`steps.toml` unless `--steps` names one, and prints each step and, on a
failure, the log. Without steps it lets each plugin in and prints its
panels.

A panel, settings page, footer or overlay is built as the dock,
Settings or the window builds it when it shows it, an overlay whether
it is shown or not. Its outline is a line for each node, `#` and its
key, indented under the node that holds it, and a line for each text
it shows. A widget is named by text it shows: `click` presses the
button whose text it is, `write` leaves text in the field whose
placeholder it is, `toggle` switches the toggle nearest it, and
`hover` puts the pointer over a watched node by its text.

The test `the_samples_pass_their_steps` (ignored, as it builds the
samples) runs the scripts of hello, notes and board.

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
  `home.rs` and nowhere else. Tests put in a `Home` of their own. It
  holds `workspaces.ron`, `grants.toml`, `switched_off.toml`,
  `limits.toml`, `desktop.toml`, the installed plugins in `plugins/`
  and the asset
  store in `assets/`, and
  is the agents' home too: their settings, records and sessions.
- **Writing a file whole.** `write_whole` writes a file beside its
  place as `saving_<name>` and moves it over the old one, so a stop in
  the middle leaves the old file or the new one. The workspaces, the
  grants, the plugins switched off, the person's limits and the asset
  store are written with it.
- **The log.** A `LogLine` message, from a part of the engine or a
  plugin, is passed to the terminal and kept in `Log`, the last
  `[log] keep` lines, for the console.
- **Budgets and limits.** `[budgets]` in `engine.toml`, as the
  `Budgets` resource, holds the caps on all plugins together: the
  frame's share, the strikes and the chain's depth. `[limits]`, as
  `DefaultLimits`, holds every limit of one plugin as the engine has
  it; each plugin's own are `PluginBudgets` on its entity (see
  [Limits](#limits)).

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
  one. `view.in_panel()` gives content the panel's padding and its
  fill, shaded toward the bottom. `tree.open_panel(id)` (`DockTreeExt`)
  brings a panel to the front.
- **The console** (`engine/console`) lists the log, newest first.
- **Settings pages** are entities with a `SettingsPage`, as plugins add
  them: a title, an icon and a line on what it holds. The Settings
  panel (`engine/settings`, opened from the View menu) shows a link
  row to each in the order they came, and under them each
  `SettingsFooter`'s content; the page gone through is shown under a
  head whose SETTINGS leads back. `world.open_settings_page(page)`
  (`ShellWorldExt`) shows one and brings Settings to the front.
- **Overlays** (`overlay.rs`) are entities with an `Overlay`, as
  plugins add them: a card over the dock, at most
  `[space] overlay_width` wide, on a filled backdrop that takes every
  press, shown while it carries `OverlayShown`. One is shown at a
  time, the first by its entity.

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
  for), `Critical` (what refuses or stops), `Quiet` (text only, as
  a way back), `Row` (a row that leads somewhere), `Title` (a menu's
  title) and `Tab`. `action` and `icon_button` show text in the display
  face, `icon_button` after an icon; `content_button` is a button
  padded as `icon_button` is, showing any view, and `icon_row` and
  `raised` set out an icon and its text as `icon_button` does.
  Besides: `field`, `toggle`, `dropdown`, `choices`, `heading`,
  `section`,
  `caption`, `note`, `card`, `list` (its rows each a `list_row`),
  `link_row` and `link_rows`, `page_head`, `divider`, `buttons` (a row
  that wraps), `scroll_area` and `title_button`. `StyleExt` wraps any
  view:
  `styled(kind)` as a button, `with_icon(glyph)`, `in_tab(shown)` and
  `in_panel()`. A `BottomGradient` surface, as a panel or the bar,
  shades toward `background_bottom`.
- **Glows** are Bevy's `BoxShadow` and `BackgroundGradient` and
  nothing else: no shader of the shell's own. A control glows by what
  it is for (`Glow`): eased toward the pointer, a press or the caret,
  covered while held, pulsing once its work is done, or shaking if it
  refused. The glow is two shadows, a halo and a brighter core, set a
  little below the control (`drop`); a `Quiet` button glows about its
  text. The primary action and a field being typed in are also
  lit inside, by a gradient from the active colour at the bottom to
  nothing at the top (`inner`). A title or a tab is lit the same way
  (`BottomLight`), under the pointer and while its menu is open or it is
  shown. A scroll area cuts what it holds as far past its edges as a
  glow reaches (`GlowMargin`), and a panel cuts at the outer edge of
  its padding, so a glow at the edge shows whole. What floats has
  `drop_shadow`.
- **Plugin panels** and settings pages are built node by node from
  the same widgets and styles.
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
  closes, whole or not at all. RON, since a layout is a tree of enums,
  which RON writes as it is and TOML only as a table per level.
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
  it. `world.run_command(id)` (`ShellWorldExt`) runs one by id;
  `commands.spawn_command(..)` (`ShellCommandsExt`) spawns one with its
  menu entry and keys as its children.
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
- **The engine's own.** Run in Background and Quit Hamlet (Cmd+Q,
  Ctrl+Q off macOS) in the Hamlet menu, the workspace commands, and for each panel a command in
  the View menu that brings it to the front, or opens it in the first
  area (`world.open_panel(id)`).
- **The status line** at the bottom of the window shows the
  `StatusItem`s plugins add, in the order they came, each an icon and
  words in a tone with words under the pointer, hidden while it has no
  words; one that grows takes the room left, words or none, and clips
  its words. Then the workspace shown and the log's last line, each
  behind an icon, a warning in the accent and an error in red.
- **In the background.** Run in Background, or the window's close
  button, hides the window and the engine runs on, a frame every
  `[window] hidden_wait` seconds. `ShowWindow` shows it again and
  gives it the focus.

## Plugins

`hamlet_engine_plugins` keeps each plugin as an entity.

- **A plugin** is an entity with a `PluginInfo`: id, name, version,
  and its `PluginKind`, built in (Rust, in the binary), bundled (WASM
  that comes with Hamlet) or installed (WASM the person added). A
  plugin built in has an `EntryPoint` too. Two plugins of one id are
  not kept: the second is turned away, and the log says so.
- **The entry point** is one function, given an `Entry` to declare what
  the plugin adds: panels, commands, menu entries, shortcuts, settings
  pages and footers, suggested workspaces, status items and overlays. It never reaches the app or the world.
  A name in it is the plugin's own; a command is named by its name, or
  by its full id for another's.
- **What it adds** is an entity each, linked to the plugin by
  `ContributedBy`; the plugin's `Contributions` lists them, and goes with
  it (`linked_spawn`). `[limits] panels` and `contributions` cap how
  much one plugin adds; what is past them, or not well named, is left
  out and told in the log.
- **On and off.** Triggering `Enable` on a plugin runs its entry point
  and adds what it declares, and marks it `Enabled`. `Disable` despawns
  everything it added. Its panels leave the dock but stay in the
  workspaces, and come back when it is switched on again.
- **Stopped.** A plugin the engine stopped while it was on carries
  `Stopped` with why; what it added stays until it is switched off.
- **Built in.** `commands.add_builtin(manifest, entry)`
  (`PluginCommandsExt`) adds a plugin linked into the binary and
  switches it on, unless it is kept off. A manifest that `check`
  refuses is told in the log and not added.
- **The plugin list** (`engine/plugins`) is the engine's own panel:
  each plugin's name, with its id and version, whether it is on and
  how much it adds, or why it was stopped, under it, and the switch
  that turns it on or off at the right of its name; an installed one
  has UNINSTALL too (`Uninstall`), and a bundled one says it comes
  with Hamlet. LIMITS folds out its limits (see [Limits](#limits)).
  ADD PLUGIN... above them triggers `AddPlugin`, for a host to
  answer.
- **Kept off.** `SwitchedOff` holds the ids the person switched off on
  the plugin list, kept in `switched_off.toml` in the home
  (`plugins = ["hamlet.demo"]`). Only the switch there changes it;
  the engine stopping a plugin, or a hot reload, does not. At
  start a plugin on it is left off, of any kind. Letting an install
  in, and uninstalling, take its id off. A file that cannot be
  read is left as it is: the log says so, and no plugin is kept off.
- **The demo** in `engine/app/src/demo.rs` is built into the binary. It
  adds a panel, Hello; a command, Say hello, in a Demo menu and on
  Primary+Shift+H, which writes to the log; and a workspace to take,
  Hello with the console.
- **`commands.switch_on(..)`** adds what an `Entry` declares and marks
  the plugin on.
  A built-in plugin's entry point runs through it, and so does a WASM
  plugin once it has said what it adds.
- **A command's function** is given the world and the command's
  entity, whose `Command` and `ContributedBy` say which it is and
  whose.

### Limits

- **What one plugin is held to.** Each limit is a field of
  `PluginLimits` (`engine/api/src/limits.rs`): its memory, its fuel
  and time a call and a second, the bytes and requests of a call, its
  calls and events a second, its system resources, its assets and
  their writes, the services it calls and what they spend, the events
  it sends, its fetches, its panels and what else it adds, its log
  lines, its petitions, its nodes, and its scenes and GPU work. The
  engine's value of each is `[limits]` in `engine.toml`
  (`DefaultLimits`).
- **The plugin's ask.** A manifest's `[limits]` names only those it
  needs other than the engine's, by the same names:

  ```toml
  [limits]
  fuel_per_call = 2_000_000_000
  call_time = 2_000.0
  ```

  What it asks is used as it is, for a bundled plugin and an
  installed one alike: nobody is asked about it.
- **On the plugin list**, LIMITS under a plugin folds out each of its
  limits: its name, where the value in use comes from (the engine's,
  asked for by the plugin, or yours), and the value in a field. A
  value typed there becomes the person's, and RESET takes it away
  again; one that is not a number above zero, whole where the limit
  is, is refused and said so over the limits.
- **The person's value** of any limit of any plugin is kept in
  `limits.toml` in the home, by plugin, and outlasts reloads, new
  versions and restarts; uninstalling a plugin forgets it. Any value
  above zero is taken, with no ceiling. The file is written whole or
  not at all; one that cannot be read is left as it is, the log says
  so, and the values are kept only until the engine closes.

  ```toml
  [plugins."hamlet.realm"]
  fuel_per_call = 3_000_000_000
  ```

- **Resolved** for each limit in this order: the person's value, else
  the plugin's ask, else the engine's. The result is `PluginBudgets`
  on the plugin's entity, given as the plugin comes or its manifest
  changes, and again when `[limits]` or the person's values change.
  Every part of the engine that holds a plugin to a limit reads it
  there.
- **When a change applies.** At once, at the plugin's next call or
  request, for every limit but two: `panels` and `contributions` are
  counted as what it adds is added, so a change to them applies when
  it is next switched on. A `plugin.toml` whose limits alone changed
  is read again while the plugin runs.

## Permissions

`hamlet_engine_grants` keeps what the person lets each installed plugin
do. Plugins built in are not asked about, and bundled ones are allowed
all they ask (see [Bundled plugins](#bundled-plugins)).

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
  time. `[limits] petitions_per_minute` of these a plugin may raise in
  a minute; the rest are refused. The asset store, the bus and the
  network keep the request meanwhile in a `Waiting`, and answer it once
  the person did: reading another's type, calling a service and
  fetching from a host are each asked about this way.
- **The grants file** is `grants.toml` in the home: for each plugin,
  whether it was let in, the version answered for, and each answer. It
  is written whole or not at all. A file that cannot be read is left as
  it is: the log says so, and answers are kept only until the engine
  closes.
- **The Permissions page** of Settings shows the answers by plugin or
  by capability, and each can be changed there, but for a bundled
  plugin's, which are shown as they are.

## The WASM host

`hamlet_engine_wasm` runs plugins built as WASM components, with
wasmtime. `[wasm]` in `engine.toml` sets it up; it holds a plugin
to the plugin's own limits (see [Limits](#limits)), written here by
their names in `[limits]`.

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
  and waits for it off the frame (`dialog.rs`, which a button of a
  plugin's with a `folder` opens too, one dialog at a time). The
  folder chosen, or one given by
  `AddFrom` as a test does, is checked: a `plugin.toml` the engine can
  take and a `plugin.wasm`, each a plain file within its size, not a
  link, and the folder neither in the home nor holding it. A folder of
  sources is refused; plugins are not built here. The two files are
  copied off the frame into `<home>/plugins/.adding_<id>`, read again,
  and the folder is moved into place: `<home>/plugins/<id>`, or the
  folder of the installed plugin of that id, which it replaces. The
  person is asked about it at once. An id of a plugin built in or
  bundled is refused.
- **Uninstall** on the Plugins panel (`Uninstall`) takes an installed
  plugin away with its folder, and the grants forget what the person
  answered for it.
- **Switched on**, only when it is `Admitted`, a plugin's component is
  compiled off the frame, on
  the async compute pool, and kept, so switching it on again needs no
  compile. It is then instantiated in a store of its own and asked what
  it adds. A plugin that imports anything but the host's interface and
  the system's (`wasi:`) is not loaded, and one built against another
  version of the interface is told so.
- **The system** (`wasi.rs`) is WASI 0.2 from wasmtime, with a context
  of its own in each store that grants nothing but the clocks and
  randomness. Its calls are held to the same budgets as the host's: a
  list it hands over counts towards `bytes_per_call`, random bytes are
  `bytes_per_call` at most, a wait ends at the call's `call_time`, and
  it holds `[limits] wasi_resources` resources at most. Each write to
  stdout or stderr is kept up to 4,096 bytes, `log_lines_per_second`
  writes a call; the rest are left out without the plugin knowing.
  What is kept is cut into lines, each cut at `log_line_bytes`, for
  the log.
- **What it adds** comes back from `declare` as `Contribution`s and is
  made into the same `Entry` a built-in plugin fills, so it is added,
  capped and taken away as theirs is: its panels, settings pages,
  footers and overlays are built node by node as it says, and its
  commands put their name in its inbox, to run it on the next frame.
  Its menu entries, shortcuts, suggested workspaces and status items
  are plain data. What a call sets with `set-status`, `show-overlay`
  and `open` is made once the call ends (`surfaces.rs`), on what the
  plugin added under that name.
- **Switched off**, its store goes, and with it its state.
- **The engine** is one for all plugins, with fuel, an epoch and a pool.
  A thread ticks the epoch every `[wasm] tick` milliseconds. The pool
  has room for `[wasm] plugins` plugins and one more, for a version
  built beside a running one; each plugin's share of it is capped: its
  core instances, memories and tables, and each table's elements.
- **Every call** into a plugin is given `[limits] fuel_per_call` fuel,
  an epoch deadline of `call_time` milliseconds, and `bytes_per_call`
  bytes it may hand over. A string or a list from the plugin is checked
  against its memory, and its length taken from those bytes, before
  wasmtime copies any of it. What the host hands a plugin is checked
  against the same bytes before the call.
- **Memory** is counted over all of a plugin's memories together, held
  to `[limits] memory`. The pool leaves each memory all a 32-bit
  memory can address, so the limit holds whatever it is set to.
- **Limits change as it runs.** Each call is armed with the plugin's
  limits as they are then, so a new value reaches a running plugin at
  its next call.
- **A fault** is a call over its fuel, its time, its memory or its
  bytes, a trap, or what cannot be read; a plugin cannot be called
  again after one.
- **The log.** What a plugin logs or prints goes to the engine's log
  under its id, each line cut at `log_line_bytes`.
- **Requests and mail.** What a plugin asked in a call is written as
  an `Asked` for the services once the call ends, `depth` one more than
  the work it was serving. A `Deliver` for it goes into its inbox, and
  the messages at the front of its inbox are handed over together in
  one `receive`, as many as fit in `bytes_per_call`; a reply too large
  to fit at all is handed over as a failure. A call of one of its
  services (a `Serve`) goes into its inbox too, and `serve` answers it
  with a `Served`. Its tickets go on counting across a reload.

### Hot reload

- **Noticed.** Every `[wasm] watch` seconds each WASM plugin's
  `plugin.wasm` and `plugin.toml`, installed or bundled, are checked, and a change is taken
  once a file stayed the same from one check to the next.
- **A new `plugin.wasm`** of a running plugin is compiled off the frame
  and swapped in by one system in the frame's update, never in the
  middle of a call: the running version's `save` gives its state, the
  new one is instantiated in a store of its own (the pool keeps room
  for it), given the state by `restore`, and asked what it adds. Its
  inbox and its second's spending carry over. Its nodes are taken
  away and each panel or settings page shown is mounted again; when it
  adds something else, what it added is taken away and added anew.
  The log says it was reloaded.
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
  its inbox stays for the next second.
- **Stopped when it keeps going over.** A plugin that went past a
  second's budget `strikes` seconds in a row is stopped, and the log
  says why. The first second it goes over, the log says it waits.
- **The frame's share.** The calls of all the plugins in one frame
  share `frame_share` milliseconds. The plugin served longest ago goes
  first, and what is left when the share is spent waits for the next
  frame, when those left go first.
- **The inbox** holds what the person did to its nodes, commands,
  messages, calls to serve, and panels to mount and unmount,
  `events_per_second` at most; more are left out, and the log
  says how many. A call left out answers its caller out of budget.
- **A service's call is paid by its caller.** The fuel and time its
  plugin spent serving it are charged to the caller's
  `service_fuel_per_second` and `service_time_per_second`, not to its
  own second; its own counts the call.
- **The log.** A plugin's lines of each level, info, warn and error,
  are `log_lines_per_second` a second at most; the rest are left out,
  and the log says how many when the second ends.

### Panels

- **Built node by node** (`retained.rs`). A WASM plugin's panels and
  settings pages are the same `Panel` and `SettingsPage` as a built-in
  plugin's, and carry a `Surface`: their name within the plugin, and
  why it was stopped if it was. When the dock shows a panel, or
  Settings a page, a root is built for it, inside a panel's padding,
  and the plugin is asked to `mount` it there; when the root goes, it
  is told to `unmount` it. A plugin that does not export `ui-guest`
  leaves them empty.
- **Holders.** What a call did to its nodes is kept (`nodes.rs`) and
  made, as the call ends, into a holder for each node: an entity with
  its properties, and, for a node that holds others, them in order.
  Its widget reads the holder, so a `set` changes one value and only
  the widget reading it; what a node holds is a fynix `each` over that
  list, built under the rules of the widget it is in, so the label of
  a button takes the button's tone as the shell's own do.
- **The shell's own widgets.** A frame is a `column` or `row`; a label
  a `label` (`DisplayText`, not wrapping, in the display face), its
  tone, size and boldness left to the rules it is under unless set; a
  button `action` in its `ButtonStyle`, or, holding an icon and a
  label, a `content_button` setting them out as `icon_button` does; an
  icon one of the engine's set by its name (`plus`, `check`, `bell`,
  ... as `Glyph::name` has them), a name the set does not have shown
  as it is; a toggle `toggle`, a field `field`, or `message_field`
  with more than one line (`message.rs`), a list `list_row`s, a
  scroll `scroll_area`, with `StickToEnd` when it sticks
  (`stick.rs`), a divider `divider`, and a dropdown
  `dropdown`. A node with a tip has fynix's tooltip, a `note` of it,
  and one with a menu fynix's context menu, a `submenu` for each
  group; a click opens it as a right-click does.
  A widget is built again when what it is built as changes, such as
  a button's style, a dropdown's options, whether it has a tip, or
  its menu.
- **Moved by the engine.** A tone, size, opacity, space or offset set
  over a motion is moved from where it is shown over the theme's curve
  for it, a tone as its colour. A role is filled in from the theme as
  it is now, while it moves and again whenever the theme changes.
- **Inputs back.** A click, the pointer over a watched node or off it,
  a button down on it or up, a toggle switched and a field's text (on
  Enter, or as the field is left, and sent from a field of several
  lines) go into the inbox, and are handed
  over together in one `input`; the texts together are cut to
  `bytes_per_call`. A plugin that answers that something still moves
  is given a `frame` each frame until it answers that nothing does.
  Any other call that changed its nodes is given a frame after it too.
- **Budgets.** Makes, sets, orders and watches are
  `[limits] node_changes_per_call` a call, past which the call fails,
  and `node_changes_per_second` a second, past which the plugin waits.
  It holds `nodes` nodes at most, none deeper than `node_depth` under
  its root; a call that makes more, or deeper, fails.
- **Costs.** `NodeStats` counts the calls into plugins for their
  views, the calls they made on nodes, and the time of both.
- **Stopped.** A plugin at fault is stopped: its store goes, it is
  marked `Stopped` with why, the log says so, and its panels and
  settings pages show that it was stopped and why. Switching it off
  and on starts it afresh.

## Bundled plugins

Hamlet's own features are WASM plugins that come with the engine. They
are a kind of their own, apart from the plugins built into the binary
in Rust, such as the demo, and from those the person installs.

- **The folder.** `plugins/` at the repository root is a cargo
  workspace of its own, left out of the root's, with one release
  profile for small builds. Each plugin is a crate `plugins/<name>/`,
  named as its folder, with its `plugin.toml`, depending on
  `hamlet_engine_sdk` by path.
- **The script.** `scripts/build_plugins.sh` checks that the
  `wasm32-wasip2` target is installed, and says how to add it if not;
  builds the workspace with `cargo build --release --target
  wasm32-wasip2`; and lays each plugin out as `plugins/dist/<id>/`,
  its `plugin.toml` and `plugin.wasm`, each file written beside its
  place and moved over it. `target/` and `dist/` are not kept in git.
- **Found at start.** `HAMLET_ENGINE_BUNDLE` when it is set; else a
  `plugins` folder beside the executable; else, in a checkout, the
  repository's `plugins/dist`, found from the binary crate's
  `CARGO_MANIFEST_DIR` as it was built. The names are constants in
  `engine/core/src/bundle.rs` and nowhere else, and the binary puts
  in the `Bundle`; with none, as in the harness, no folder is read. A
  bundle that is not there is told once in the log, and the engine
  runs without it.
- **Read in place.** Each folder of the bundle is read with the same
  checks as an installed plugin's, where it is, never copied into the
  home, and its `PluginInfo` is marked `Bundled`.
- **Trusted, since they come with Hamlet.** Nobody is asked: a bundled
  plugin is let in with every capability its manifest asks for
  allowed, recorded in the grants file as `bundled` so the
  Permissions page shows it. An update asks nothing either. What was
  recorded for a bundled plugin is no answer for an installed plugin
  of the same id.
- **Off, never away.** A bundled plugin can be switched off, and
  `switched_off.toml` keeps it off, but it is not uninstalled: the
  Plugins page shows no UNINSTALL for it and says it comes with
  Hamlet.
- **Its id is its own.** An installed plugin with a bundled plugin's
  id is refused, at start, when it appears and when it is added.
- **Hot reload.** The bundle is watched as the plugins folder is, so
  running the script again while the engine runs reloads each plugin
  that changed and adds a new one.

`plugins/about` is a panel, About, showing the engine's name and
version (`ENGINE_VERSION`) and the interface's. Built with the
script, it is checked by its `steps.toml`:

```sh
cargo run -p hamlet_engine_harness --bin plugin_check -- plugins/dist/hamlet.about --steps plugins/about/steps.toml
```

Hamlet's own shell is three plugins over the agents service. None
holds a key: a key typed in Welcome goes to the service with
`Connect` and is forgotten.

- **`plugins/settings`** (`hamlet.settings`, asking `agents.read`,
  `agents.settings`, `agents.chief`, `desktop.settings` and
  `desktop.open https`) adds five pages to Settings
  and a footer. Endpoints: each endpoint with whether it has a key or
  is the ChatGPT plan, MANAGE USAGE for the plan, REMOVE, and ADD A
  PLACE FOR MODELS, which begins a setup that adds one. Models: the
  default model as a dropdown of the model run now and those allowed,
  LIST MODELS, FIND, what each endpoint answered, and the first 40
  models found, each with its allowed toggle, its prices in and out
  (a field where the price is unknown or the person's own), where
  its price comes from and what it holds. Agents: reflect every N
  turns (0 to 1000), the tools' network access, Keep awake (from the
  desktop service), and the lasting permissions with REVOKE. Charter: the charter as words, EDIT, and
  BACK TO THE DEFAULT for a home's own charter, asked about first.
  Realm: every limit of the realm with its bounds. The footer, SET UP
  AGAIN, begins a setup from the start. Numbers are typed in fields
  and checked against their bounds; a change the plugin or the
  service refuses is shown in red on its page. When a setup that adds
  a place ends, the Models page is shown.
- **`plugins/welcome`** (`hamlet.welcome`, asking `agents.read`,
  `agents.settings` and `agents.keys`) is the first-run card, an
  overlay. It begins a setup at start when no model was chosen, and is
  shown while a setup is under way (`Setup::begun`), whoever began it.
  Its steps: the ways in (sign in with OpenRouter, continue with
  ChatGPT, a key, a model on this computer, or leave), the key with
  its provider or OTHER and an address, signing in, looking for a
  local server (LOOK AGAIN), and the models found, narrowed with FIND,
  of which the one picked is tried. The setup itself, its problems in
  plain words included, runs in the service.
- **`plugins/status`** (`hamlet.status`, asking `agents.read`) adds
  seven status items, from the service's `Status`: the sandbox, the
  model, the link (red when not connected), what the open session
  used, the first hamlet's spend, how many things wait for the chief,
  and whether the open session works. Between the spend and what
  waits, a growing item shows the address under the pointer in the
  accent, as the desktop service tells its watchers.

Settings and Welcome have a `steps.toml`; the harness runs no agents
service, so they check the pages and the card's steps on their own.
The status items are not read by the harness.

`plugins/scene_demo` (`hamlet.scene_demo`, asking `render.scene`,
`render.shader` and `render.compute`) is the panel Scene: a viewport
of a lit scene, under a sun with shadows, of a ground, a sphere, a
ring in a material of its own WGSL (`ring.wgsl`), a six-sided pyramid
of its own mesh with a colour at each vertex, and a field of 400
cubes as copies of one, lifted in waves by compute (`waves.wgsl`)
through the material that draws them (`field.wgsl`), with a pass that
darkens the view's edges (`edges.wgsl`). Dragging turns the camera
about the middle and the wheel brings it nearer; the shape or cube
under the pointer lights up, and a click names what it hit over the
view. Its `steps.toml` counts the scene and clicks a shape and a cube.

`plugins/realm` (`hamlet.realm`, asking `agents.read`, `agents.chief`,
`render.scene` and `render.shader`) is the realm, drawn behind the
dock from the agents service's roster, as `crates/hamlet_realm` draws
it: the endless land of [`endless-world.md`](endless-world.md) in
tiles about the camera with its rivers and trees, a town for every
hamlet with a house, a yard and a robot for every agent, the roads
between agents with their bridges and lit grooves, and the chief's
Hearth with its mascot, which goes by a Wickstep to the agent the
chief talks to. The sky follows the chief's clock (`Focus::offset`).
Its tuning is `assets/world.toml` and `assets/palette.toml`, built in.
- **Within its budgets.** Everything is built a step at a time on
  `tick`, nearest the camera first: `upload.rs` holds each call to a
  share of the bytes it may hand over and of a second's time, and a
  mesh with no room left waits for the next call.
- **The roster** is followed in `bridge.rs` as `main` follows the
  core's entities: a hamlet founds its town at its spot or the first
  free site, its agents take their plots in the order they came, one
  that leaves is a ruin or a wing of the house it merged into, and the
  roads and what each agent does follow theirs.
- **The camera** (`camera.rs`) frames every hamlet; the left button
  slides the ground, the right turns and tilts it, and the wheel comes
  nearer. Reset Camera, Focus on Selection (F), Follow Selection
  (Shift+F) and the sky held to day or night are in the View menu.
- **The pointer.** A robot, a house, a common, the Hearth or the
  mascot under it has a ring of light, and a click selects its agent,
  hamlet or the Hearth in the window's focus and brings the Inspector
  forward; a click on the bare ground selects nothing. A hamlet or the
  Hearth selected elsewhere is brought into view. Houses, commons and
  the Hearth have signs while the camera is near, and a robot or the
  mascot a tag under the pointer.
- **Moving a town.** It hears the Inspector's `carry`: the town is in
  hand, its footprint follows the pointer over the ground, gold where
  it fits and red where it does not, a click sends `/move-hamlet` to
  its hamlet as a decree, and the right button puts it back.

The agents' panels are four more, each following the focus the
agents service keeps, so what one opens or selects the others show.
They are built with `plugins/agents_ui`, a library and no plugin: the
service as the panels read it (`start`, `ask` and `hear`, keeping the
focus, the roster, the needs and the library's skills as resources),
a message, a change, the card of an agent's questions, Markdown as
labels can set it (headings, paragraphs, items, quotes, code and
rules; a link is its words, and each `https` link is a button under
the text that opens it and leads to its address), and a confirmation
on an overlay, the
verb beside CANCEL with a warning for agents it cuts short.

- **`plugins/chat`** (`hamlet.chat`, asking `agents.read`,
  `agents.chief` and `desktop.open https`) is the panel Chat, apart from the Inspector. Its
  head names the house and everyone who speaks, with New session,
  asked about first. Its line of time is the open conversation and
  the sessions `Related` gives, in the order said, each run of one
  speaker to one listener named once; a turn's steps fold into one
  row whose heading says what it did and opens it, its edits shown
  even shut. It keeps to its end until the chief scrolls up, and
  another conversation is shown from its end. The last 24 rows are
  built, and 24 more each time it is scrolled to its top, the rows
  read staying where they were.
  The chief's bubbles sit at the right on the accent's fill, and
  anyone else's at the left on a card, by a disc with the speaker's
  picture at the end of its run.
  A bubble shows its words plainly, quotes dim, or as Markdown, its
  hour on the chief's clock, and under the pointer Reply and, for the
  chief's own words while the agent rests, Rewind to here. The bar
  holds the card of questions the agent waits on, what the next
  message replies to, and the field, "Message", which grows to eight
  lines: Enter sends, Shift+Enter adds a line and Escape keeps the
  draft. A line that begins with `/`
  is not sent: the chat says why, in its own line of time. The chat
  comes to the front when the open conversation moves to another, or
  a session resumed is read again.
- **`plugins/inspector`** (`hamlet.inspector`, asking `agents.read`,
  `agents.chief`, `agents.settings` and `desktop.open https`) shows
  what is selected. An agent: its hamlet as
  a way back, its state, STOP while it works, More (`more.rs`: a
  menu of talking to it, its prompt, stopping it, a road to another
  agent of its hamlet, a skill another has, merging, undoing the last
  change, making it mayor and removing it, each a decree to its
  hamlet, asked about first where `main` asks), its purpose, its
  model and use, its spend, the Model dropdown (`RunModel`), and NOTES
  (REFLECT NOW), CHARACTER (EDIT), ARCHIVE and PROMPT. The archive
  lists the past sessions with a search and counts, each read on a
  click, and read, resumed (bringing the chat to the front) or
  deleted from its menu, and reads one
  forty messages at a time. One that is gone, with RESTORE, DELETE FOR
  GOOD and ITS HAMLET. The library. Every hamlet, searched, each with
  Close Down in its menu, with FOUND A HAMLET, those closed down, and
  YOUR HEARTH, which selects the chief's place for the realm to bring
  into view.
  FOUND A HAMLET, and Found a Hamlet... in the Hamlet menu, show the
  form a hamlet is founded with on an overlay (`founding.rs`): its
  name, its mayor's and what it is for, all three needed, sent as
  `/found` to the open conversation; CANCEL keeps what was typed.
  A hamlet, with RENAME, MOVE (the event `carry`, with the hamlet's
  name, for the realm to take its town in hand), CLOSE DOWN, the
  folder it works on, CHOOSE
  A FOLDER (or CHOOSE ANOTHER) in the system's dialog, which gives it
  `/folder` with the path, and its mode, and AGENTS, BYLAWS
  (`bylaws.rs`: its own limits as `main`'s rules panel sets them, on
  one page: the budget's period and cap with START IT AGAIN NOW, a
  task's cost and messages, a road's price, the models it may run
  and its default, what its mayor may do without asking, and its
  most agents and roads, each written with `SetBylaw`) and RECORD.
  The realm's page and the bylaws share the controls of a limit,
  `agents_ui`'s `whole`, `switch` and `setting`.
- **`plugins/skills`** (`hamlet.skills`, asking `agents.read` and
  `agents.chief`) is the agent's own skills over the library's, each
  with TAKE OUT, and OPEN CHAT, which brings the chat to the front,
  while the library is empty.
- **`plugins/needs`** (`hamlet.needs`, asking `agents.read` and
  `agents.chief`) is what the agents wait on the chief for, oldest
  first, with the reason field, STOP ALL, and the answers each takes;
  a question's card is the one the chat shows. It comes to the front
  when more wait. It is apart from the engine's Petitions, which ask
  what a plugin may do before it does it: those are the grants'
  questions about plugins, these the agents' about their work.

Each has a `steps.toml` checking what shows before the agents
answer.

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
- **Caps** (`[limits]`): `asset_bytes` for one asset, less than
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
- **Sent** with `emit`, `[limits] events_sent_per_second` a second at
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
- **Caps** (`[limits]`): `fetches_per_minute`, `fetches_waiting` at
  once, `fetch_bytes` of an answer, `fetch_time` seconds for the whole.
- **A fetch** is a `Fetch` on a child of its plugin, so it goes with
  it. With `Offline` in the app, as the harness has it, fetches are
  never sent and wait for `world.answer_fetch(..)` (`NetWorldExt`).

## The agents

`hamlet_engine_agents` runs the agent core (`hamlet_core`: sessions and
their workers, oversight, the catalogue, the treasury) in the engine's
world, as the app's core plugin does, on a Tokio runtime of its own. The
agents' home is the engine's home. The chief is `HAMLET_USER`, else
`USER`, and their sessions are kept under the channel `app`. When the
agents cannot run (no sandbox, settings that cannot be read), the log
says why and every request fails saying so.

- **The protocol** is `hamlet_engine_api::agents`: a plugin asks with
  `Request::Agents(AgentsRequest)`, `agents` in the WIT, and is
  answered with `Reply::Agents(AgentsReply)`. Plain serde types, so a
  remote harness can stand behind the same protocol later.
- **Watching.** `Watch` sends each `AgentsEvent` from then on as
  `AgentsReply::Event`, with the ticket of the `Watch`: the settings,
  the link, the listing, the catalogue changed, a trial, the setup, the
  charter, the realm's limits, the lasting permissions, the status,
  sessions that came or went, what a working session says or does
  (words streamed in a frame go in one event) and whether it works.
- **Settings.** `Settings` gives the model, `reflect_every`, `allow_net`
  and the endpoints by name and address, each with only whether it has
  a key or is the ChatGPT plan, with the link and the listing. Each of
  `SetModel`, `SetReflectEvery`, `SetAllowNet` and `RemoveEndpoint`
  saves the settings; removing an endpoint forgets its key and signs
  out of ChatGPT. `SetKey` and `ForgetKey` give or forget an endpoint's
  key by name. `OpenUsage` opens the plan's usage page.
- **Models.** `TestConnection`, `ListModels`, `Catalogue`,
  `AllowModel`, `PriceModel` (only a model with no price, or the
  person's own) and `TryModel`.
- **Setting up a place for models**, as Welcome does: `Providers`,
  `BeginSetup` (adding or not), `Connect` with a provider or an address
  and a key, `SignIn` to OpenRouter or ChatGPT, `FindLocal`,
  `PickModel` and `EndSetup`, followed in `Setup`: whether one is
  under way (`begun`, from `BeginSetup` until it ends), its stage, who
  answered and with which models, and the problem in plain words. The
  endpoint being set up, and a key a sign-in gave, stay in the service.
- **The charter**: `Charter`, `EditCharter` (the home's own, opened in
  the system's editor) and `ResetCharter`.
- **The realm's limits**: `Limits`, and `SetLimit` of one key of
  `votes`, `hamlets` or `road.grows_over_days`, failing with the
  problem when the file cannot take it.
- **Lasting permissions**: `Grants` and `Revoke`.
- **Status**: the sandbox, the model, the link, the open session's use,
  the first hamlet's spend, how many things wait for the chief, and
  whether the open session works.
- **The chief**: `Send` a line, a message or a slash command, to an
  agent or a hamlet's mayor, and `Veto`.
- **Sessions**: `Sessions`, `Open` (the chief's session with an agent
  or a mayor, which the status then tells of) and `Messages`. A
  session says when it began and whether it is being answered; a
  message whether it is another agent's work or a reply to a letter;
  a tool call its command, its exit and the change an edit asks for.
- **The focus** is the window's, kept in the service so every panel
  follows the same (`focus.rs`): the open conversation (`Open`), who
  it is with, and the thing selected (`Select`: nothing, an agent, a
  hamlet, the library or the chief's Hearth). Selecting a living agent talks to it, and
  opening a conversation selects its agent. When the agent of the
  open conversation is gone, it falls back to the first mayor.
  `Focus` reads it and each change is an `AgentsEvent::Focus`.
- **The chat** (`conversation.rs`): `Related` is the sessions an
  agent's chat shows beside the open one, each with its part (with
  the agent, or one it wrote to) and who writes in it. Each message
  that comes or changes is told as `Said`, or `Said` with `again`
  when a session was read afresh. `Rewind` takes a session back to
  before a message and `Rewound` gives its words back. `Decree` gives
  a chief's command to a hamlet as the app does, the answers told in
  the open conversation.
- **Papers and skills**: `Papers` (the notes, the character and the
  own skills of the open conversation's agent, the first two cut at
  6,000 characters, and the skills of the other agents of its
  hamlet it could be given), `Prompt`, `EditCharacter` (made empty if there
  is none, opened in the system's editor), and `Skills`, the
  library's, told again as `AgentsEvent::Skills` when they change.
- **The archive** (`archive.rs`): `Pasts` with a search, `ReadPast`
  (its messages without the steps between them, the last 400) and
  `DeletePast` (an archived session only), each by the name of its
  log in the house of the open conversation's agent.
- **The roster** (`roster.rs`): `Roster` is every agent and hamlet
  with its state in words, its account and spend, the hamlets closed
  down, and what the realm is drawn from: when each agent came and
  left and the step it is on, the spot a hamlet's chief set it down
  on, and the roads between agents, each end by its hamlet and
  number, with their use; it is told again as `AgentsEvent::Roster` when it
  changes. `Record` is a hamlet's budget, votes, comings and goings
  and limits file. `Bylaws` is what a hamlet's own limits file holds
  for it alone, and `SetBylaw` writes one key of that file, refusing
  a key of the realm's and failing with the problem when the file
  cannot take it. `RunModel` has an agent run a model from now on,
  written as the app does: the mayor's pinned in its hamlet's limits,
  another's chosen as its mayor would, its hamlet let run it.
- **What waits for the chief** (`needs.rs`): `Needs`, told again as
  `AgentsEvent::Needs` when it changes and every ten seconds for the
  time waited. `Answer` gives a choice with a reason or a reply. The
  answers to an agent's questions are picked in the service (`Pick`,
  `Turn`, `Submit`), so a card shown in two panels shows the same
  sheet.
- **Capabilities**, each on the target `local`: `agents.read` for what
  is read and watched, and the focus, `agents.settings` for what
  changes the settings, the models, the setup, the charter, the
  realm's limits and a hamlet's bylaws, `agents.keys` for giving or forgetting a key and signing
  in, and `agents.chief` for sending, vetoing, revoking, rewinding,
  decrees, the character, deleting a past session and answering what
  waits. A use answered Ask next time asks the
  person first, as the other services do.
- **Secrets stay in the service.** No reply holds a key, a ChatGPT
  token or a sign-in; keys are kept in the system's keychain or a
  private file of the home, as the app keeps them, but under the
  keychain service `hamlet_engine` (`KEYCHAIN_SERVICE` in
  `engine/core`), so the engine never reads or deletes the app's keys.

## The desktop

`hamlet_engine_desktop` is the machine the engine runs on.

- **The protocol** is `hamlet_engine_api::desktop`: a plugin asks with
  `Request::Desktop(DesktopRequest)`, `desktop` in the WIT, and is
  answered with `Reply::Desktop(DesktopReply)`. `Watch` sends each
  `DesktopEvent` from then on with the ticket of the `Watch`: the
  settings changed, or the address under the pointer.
- **Keep awake.** While any agent is busy and `KeepAwake` is on, the
  system's own helper holds the machine awake (`caffeinate` on macOS,
  `systemd-inhibit` on Linux, none on Windows), and it lets go when
  the engine dies. `Settings` gives it and `SetKeepAwake` sets it,
  kept in `desktop.toml` in the home, on when the file is not there.
- **Opening.** `Open` opens an `https` address in the browser, or a
  file or folder in the program the system keeps for it, a file the
  system would run shown in its folder instead. An address needs
  `desktop.open` on the target `https`, with a host and no user; a
  path, whole or from `~/`, needs it on a folder that holds the path
  with every link on the way followed. The program is given it as one
  argument that no shell reads. A use answered Ask next time asks the
  person first, as the other services do.
- **The address under the pointer.** `Hover` tells of it, or that
  there is none; only the plugin that told of one takes it away, and
  it goes with that plugin. A view given `leads_to` tells of its own.
  Neither this nor `Watch` and `Settings` need a capability.
- **One engine per home.** The binary claims the home before it
  starts (`claim`, the single-instance lock of `hamlet_home`). A later
  launch on the same home asks the running engine to show its window
  (`ShowWindow`), on every system, and ends.

## Scenes and GPU work

`hamlet_engine_render` draws what plugins build; the WASM host turns
their calls into its changes (`engine/wasm/src/scene.rs`). The world
`render-plugin` is `ui-plugin` with `scene-view`: the interfaces
`scene` and `gpu`, and the export `scene-guest`. A plugin that uses
none of them imports none of them.

### Layer one: scenes

- **The scene** is a subset of Bevy's ECS, kept by handle: `mesh` (a
  shape of the engine's, or `mesh-data` of the plugin's: positions,
  normals made when left out, uvs, a colour at each vertex and the
  triangles, checked whole and uploaded once; `update` replaces it),
  `material` (`pbr`: base colour, emissive, roughness, metallic, alpha
  mode and cutoff, unlit, both faces, fog; a mesh's vertex colours
  tint it), `entity` (under a parent or the scene's root, with a key
  of the plugin's: its transform, visibility, parent, mesh and
  material, light, shadows, words over it and what it watches),
  `instances` (copies of one mesh in one material, ten floats a copy,
  copy `i` tagged `i` for a shader; Bevy draws them together) and
  `camera` (at an entity, along its forward). Dropping a handle takes
  its thing away; dropping an entity takes away what is under it.
- **Lights** are directional (with shadows), point and spot, each a
  light of an entity; ambient light is a camera's.
- **A camera's settings**: a perspective or orthographic projection,
  what it clears to, high range, tonemapping, exposure, distance fog,
  bloom, ambient light and samples a pixel.
- **Only changes cross.** Each call is one change, numbered by the
  host and applied as the frame's calls end; nothing is sent again
  each frame. `animate` asks for `tick` each frame, for a plugin that
  moves things itself.
- **A viewport** is a frame of the plugin's panel that `show-in` gives
  a camera of target `viewport`: its widget shows the image the camera
  draws into, sized to the room the layout gives it (the frames above
  it grow, `[render] grow_levels` at most), and the camera draws only
  while one shows it. In the SDK, `scene::viewport` is that frame.
- **Behind the dock.** A camera of target `background` draws into the
  window under the interface. One plugin holds the window's background
  at a time (the shell's `Backdrop`); another's make is refused. While
  one does, the shell's root has no fill and lets the pointer through,
  and the interface is drawn over the scene without clearing it, so
  the scene shows where the dock has no panel. The plugin is told the
  window's size as `resized`, as a viewport's is told its own.
- **The pointer.** Over a view, its moves, buttons and wheel are told
  to the plugin with the key of the camera's entity, in the
  interface's pixels; behind the dock, only where no UI node is under
  it. A press begun over a view is let go anywhere. A ray from the
  camera through the pointer finds the nearest watched entity of the
  plugin's (Bevy's mesh ray casting): `over` and `out` as it changes,
  `click` on a press let go within `[render] click_slop`, each with the
  point hit and its normal, and the copy of instances.
- **Words over the world** (`set-label`) are texts of the interface
  kept where the camera sees the entity: over each viewport, or behind
  the dock on a layer of their own under every root of the interface
  (`label.rs`), so the dock's panels cover them.

### Layer two: the plugin's own GPU work

- **WGSL, checked first.** `shader.compile` takes plain WGSL of one
  kind, without `#`, of at most `[limits] shader_bytes`. The engine's
  bindings come before it and its entry points after it, so the source
  declares no bindings, no entry point but a compute shader's `main`
  and no `override`; naga parses and validates the whole, and a
  refusal names the line of the source. A material gives
  `fn shade(surface: Surface) -> vec4<f32>` and may give
  `fn displace(vertex: Vertex) -> vec3<f32>`; a pass
  `fn post(pass: Pass) -> vec4<f32>`; compute
  `@compute @workgroup_size(..) fn main`, of at most the device's
  invocations a workgroup. The fixed names: `params` (16 `vec4<f32>`,
  the plugin's 64 floats), and `picture`, `picture_sampler` and
  `data` (a read-only storage buffer) of a material; `screen`,
  `screen_sampler` and `frame` (size and time) of a pass; `data` and
  `extra` (read-write storage buffers) and `frame` (time, delta and a
  count) of compute.
- **Bounded.** The shader Bevy is given has wgpu's runtime checks on
  (`ValidateShader::Enabled`): every index is bounds-checked and every
  loop is bounded by naga's force-loop-bounding. Where the backend has
  no loop bounding (not Vulkan, Metal or DX12), a source with a loop
  is refused.
- **Materials** (`gpu.shaded-material`) are Bevy's `Material` with the
  plugin's shader in place of its own, lit by the scene's lights
  through Bevy's PBR functions or shown as `shade` gives, fogged and
  tonemapped as Bevy's own; `set-params` changes their params. Their
  vertices may move, so they have no prepass and cast no shadows.
- **Compute** (`gpu.compute`) runs over the plugin's buffers
  (`gpu.buffer`, of 16-byte items; a write uploads the whole buffer as
  the plugin last wrote it), once or every frame, before the cameras
  draw. The workgroups of one plugin's dispatches in a frame are
  `[limits] workgroups_per_frame` at most; one dispatch past them is
  refused, and what is past them in a frame waits for the next.
- **Passes** (`gpu.set-pass`) run over the image of one of the
  plugin's cameras in Bevy's 3D schedule, after it draws and before
  its tonemapping.
- **Textures** (`gpu.texture`) are sRGB images a material samples.
- **GPU time.** Where the device has timestamp queries, each compute
  and pass is timed (`[render] timed_slots` at once), read back each
  frame, and its plugin's milliseconds summed over a second; a plugin
  past `[limits] gpu_time_per_second` has its compute and passes wait
  for the next second, and the log says so. A material is drawn inside
  Bevy's own passes, among every other mesh, so it is not timed apart.
- **A failing device.** Bevy's `RenderErrorHandler` is the engine's:
  when the device is lost or wgpu reports an error while plugins have
  GPU work of their own, those plugins are stopped (`GpuLost`) and the
  renderer is built again (`RenderErrorPolicy::Recover`). With no such
  work the engine stops, as Bevy's own handler has it.

### Capabilities and budgets

- `render.scene` makes meshes, materials, entities, instances and
  cameras; `render.shader` compiles material and pass shaders, makes
  shaded materials and sets passes; `render.compute` compiles compute
  and dispatches it. A buffer or texture needs either of the last two.
  Each is asked for on the target `gpu`. A make without its capability
  fails saying so; one answered Ask next time fails saying it waits,
  and the person is asked once.
- **Held at once**: `scene_entities` (each copy counted),
  `scene_vertices`, `scene_materials`, `scene_lights`,
  `scene_cameras`, `gpu_objects` (shaders, buffers, textures, compute
  and passes) and `gpu_memory` (meshes, buffers and textures, as the
  engine counts their bytes). A make past one fails with why.
- **Changes**: `scene_changes_per_call`, past which the call fails and
  the plugin is stopped, as with nodes; `scene_changes_per_second`,
  past which it waits. A drop is never refused.
- **Misuse** (a handle of another kind, a number that is not finite
  where it moves the scene) fails the call.

### What is not possible, and why

- **Raw pipelines and render graphs.** A plugin gives functions the
  engine wraps; it does not build pipelines, bind groups or its own
  passes in Bevy's schedules. Bevy 0.19 runs a camera's passes as
  systems of its schedule, which a plugin cannot add at run time, and
  a pipeline's layout must be the engine's to keep its bindings safe.
- **GPU time of a material.** It is drawn in Bevy's passes with other
  meshes; only its source's size and the runtime checks bound it.
- **Stopping a runaway shader mid-frame.** A bounded loop can still
  take long; the GPU's own watchdog loses the device, which is then
  rebuilt with the plugin stopped. Whether every Bevy asset comes back
  after a rebuilt device is Bevy's to do, and is not checked here.
- **Particles** are instances moved by compute through a material's
  `displace`, as the field of the demo is; Bevy's own particle crates
  are not offered.
- **Skinned and animated meshes** of glTF are not offered: a plugin
  moves its entities' transforms itself, with `tick`.

## Where the budgets are

A cap on all plugins together is a field of `Budgets`, `[budgets]` in
`engine/tuning/engine.toml`, its meaning beside its field in
`engine/core/src/budget.rs`. A limit of one plugin is a field of
`PluginLimits`, its meaning beside it in `engine/api/src/limits.rs`,
and the engine's value of it is in `[limits]`. The host's set-up is
`[wasm]`, how scenes are shown `[render]`, and the harness's waiting
`[harness]`.
