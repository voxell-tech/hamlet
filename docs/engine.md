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
| `engine/core` | `hamlet_engine_core` | The tuning files, the home, the bundle's folder, the log, the guards and the limits a plugin is held to |
| `engine/audio` | `hamlet_engine_audio` | Sound on Bevy's audio: clips handed over as samples, whole or in parts as they play, played unplaced or placed, by category, scaled by the person's volumes |
| `engine/shell` | `hamlet_engine_shell` | The window, its theme, the dock and its panels, workspaces, commands, shortcuts and the menu bar |
| `engine/plugins` | `hamlet_engine_plugins` | Plugins as entities, what they add, switching them on and off, the Plugins panel, and the mail between plugins and services |
| `engine/grants` | `hamlet_engine_grants` | What the person lets plugins do: petitions, the grants file, the Permissions page |
| `engine/assets` | `hamlet_engine_assets` | The asset store, the type registry, references and change notification |
| `engine/bus` | `hamlet_engine_bus` | Services and events between plugins, whatever host runs them |
| `engine/net` | `hamlet_engine_net` | The network service: fetches to granted hosts at public addresses |
| `engine/agents` | `hamlet_engine_agents` | The agents service: the agent core run in the engine, behind a protocol |
| `engine/desktop` | `hamlet_engine_desktop` | The desktop service: the machine kept awake while an agent works, addresses and files opened for plugins, the address under the pointer, one engine per home |
| `engine/render` | `hamlet_engine_render` | What plugins draw: scenes kept entity by entity, their views in a viewport or behind the dock, picking, and plugins' own WGSL checked with naga, their compute and passes, held to their GPU time |
| `engine/wasm` | `hamlet_engine_wasm` | The host of WASM plugins: wasmtime, a store per plugin, the limits the person switches on, and the glue from the WIT to the services. The only crate that knows wasmtime |
| `engine/harness` | `hamlet_engine_harness` | The engine without a window, for tests, and `plugin_check` |
| `engine/app` | `hamlet_engine` | The binary |
| `engine/bevy_sdk` | `hamlet_engine_bevy` | What a plugin is written with, as a Bevy plugin, built for `wasm32-wasip2` |

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
  `hears`, and `[dependencies]` the plugins it needs (see
  [Dependencies](#dependencies)). `check` refuses one built against
  another API, with an empty or unreadable text, asking for the same
  thing twice, naming a type, service or event twice, using a type or
  importing a service it does not ask for, naming a dependency that is
  not a plugin id with a semver requirement, or needing itself. A
  plugin cannot ask for limits: a `plugin.toml` with `[limits]` is
  refused as it is read, saying only the person sets them (see
  [Limits](#limits)).
- **Capabilities.** An ask names a capability, its target and a reason in
  the plugin's words: `net.fetch` a host, `assets.read` a type name,
  `services.call` another plugin's service, and `agents.read`,
  `agents.chief` and `agents.settings` the target `local`
  (see [The agents](#the-agents)), `desktop.open` the target `https` or
  a folder, `desktop.drop` the target `files` (the files dropped on
  its views), and `desktop.settings` the target `local` (see
  [The desktop](#the-desktop)), `render.scene`, `render.shader`
  and `render.compute` the target `gpu` (see
  [Scenes and GPU work](#scenes-and-gpu-work)), and `audio.play` the
  target `output` (see [Sound](#sound)). Powers that are never
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
- **What a plugin adds** is a `Contribution` each: a panel (with an
  icon for its tab or none, `panel_icon` after it in the SDK, and a
  `PanelPadding` in place of the theme's, a role of the theme's
  `Space` or pixels, `panel_padding(Space::None)` or
  `panel_padding_pixels(..)` after it; the engine refuses a panel
  whose pixels are below zero or not finite), a command,
  a menu entry, a shortcut, a settings page (with its icon and a line
  on what it holds), a settings footer, a suggested workspace, a
  status item (one that grows or not), an overlay or a panel section
  (a part of an engine panel that takes them, so far Petitions).
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
  `desktop`. A ticket of 0 is a request not taken: a `fetch`,
  `agents` or `desktop` request that cannot be read.
- **The guest** exports `declare` (its `Contribution`s), `run` (a
  command by name), `save` and `restore` (its state, as bytes of its
  own, handed from one version to the next on a reload), `receive` (a
  batch of `Message`s) and `serve` (a call of one of its services, by
  the caller's id, with the result or why it failed).
- **Requests are answered on a later frame.** What a plugin asks in a
  call is passed to the services as the call ends, and their messages
  reach it in one `receive` on the next frame it is served. A plugin's
  stores never meet: the engine copies what passes between them.
- **Panels, settings pages, settings footers, overlays and panel
  sections** are the
  world `ui-plugin`: `plugin` with the interface `ui`, whose resource
  `node` the plugin makes,
  last under a parent that holds nodes, with a key of its own;
  `set`s a property of (at once, or over a `motion` where it can
  move); `reorder`s; `watch`es for hover and press; and drops to take
  it away with what is under it. And the export `ui-guest`: `mount` (a
  panel, settings page, footer, overlay or section to build under a root node
  the engine gives), `unmount`, `input` (what the person did, by key)
  and `frame`, each answering whether something still moves.
- **Besides the nodes**, `ui` has `set-status` (what one of the
  plugin's status items shows: an icon by name, words, a tone and
  words under the pointer), `show-overlay` (one of its overlays shown
  or hidden), `set-section` (how many things one of its panel sections
  holds; one holding none is hidden) and `open` (one of its panels brought to the front, or
  one of its settings pages shown in Settings; or, by a full id
  `plugin.id/name`, a panel of any plugin brought to the front, which
  needs no capability since it changes only the dock). Each counts
  as a change to a node in `NodeStats`.
- **The kinds of node**: `frame` (a column or a row), `label`,
  `button` (showing its text, or holding an icon and a label as an
  icon button does), `icon` (one of the engine's set, by name),
  `toggle` (a switch, with its text beside it), `field` (text to
  edit), `list` (rows with a divider
  between each), `scroll` (a column that scrolls), `divider` and
  `dropdown` (one of its options shown, opening a list to pick
  another). A frame, a button, a list and a scroll hold nodes.
- **The properties**, each of the kinds it names: `text` (a
  label's, a button's, an icon's, a toggle's and a field's), `tone`,
  `size`, `bold`, `face` (the body's or the display face), `opacity`,
  `offset`, `tip`, `menu` and `pointer` (of every kind; a tip is
  words shown near the pointer while it rests on the node, a menu its
  items, opened at the pointer on a right-click and, when it says
  so, on a click, an item with a `group` under a row of that name
  that opens them beside it, and a pointer one of the engine's set
  shown over the node, see [Pointers](#pointers)), `style` and
  `folder` (a button's: plain, primary,
  critical, quiet or row, a row of a list; a button with a
  folder's title opens the system's dialog to choose a folder when
  it is pressed, in place of a click),
  `direction`, `align` (its children at its start or its end),
  `fill` (none, a card, the accent's or a disc, by the theme's
  roles; a filled frame is padded and no wider than what holds it),
  `padding` (a frame's room about what it holds, by the same roles as
  `space`, past what its fill pads it by), `grow` (a frame's share of
  the room left along what holds it, none at 0) and
  `space` (a frame's), `placeholder` and `lines` (a field's: the most
  lines it grows to, one to start with), `on` (a toggle's), and
  `options` and `selected` (a dropdown's). A field of more than one
  line is for a message: Enter sends what is typed and empties it,
  Shift+Enter adds a line, and Escape keeps what is typed and leaves
  it. A scroll in a panel takes the height left there and scrolls
  what does not fit, however deep in frames it is. A scroll's `stick`
  keeps it to its end as it grows, until the
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
  in the dialog a button opened (`chosen`, by its path), and a menu
  opened at the pointer or a card placed `here` closed with nothing
  chosen (`closed`).
- **Scenes and sounds** are the worlds `scene-view` (see
  [Scenes and GPU work](#scenes-and-gpu-work)) and `sounds` (see
  [Sound](#sound)), which `render-plugin` includes with `ui-plugin`.

## Writing a plugin

`hamlet_engine_bevy` (`engine/bevy_sdk`) is the guest crate, and a
plugin depends on it alone; there is no other way to write one. A
plugin is an ordinary Bevy `Plugin`
with `Default`, exported with `hamlet_engine_bevy::export!`. The
macro runs one Bevy `App` in the plugin's sandbox, with `bevy_app`,
`bevy_ecs`, `bevy_math`, `bevy_time` and `bevy_transform`, no
rendering and the single-threaded executor, and drives it through the
WIT. The prelude names `bevy_ecs` for the derives of `Component`,
`Resource`, `Event` and `Message`, so a module that derives them uses
`hamlet_engine_bevy::prelude::*`. `info`, `warn` and `error` reach the
host, `encode` and `decode` turn the payloads it shares into bytes,
and `ENGINE_VERSION` is the version of the engine the SDK comes with.

```rust
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{button, column, label, resource, UiExt};

#[derive(Default)]
struct Hello;

#[derive(Resource, Default, serde::Serialize, serde::Deserialize)]
struct Count(u32);

impl Plugin for Hello {
    fn build(&self, app: &mut App) {
        app.init_saved_resource::<Count>()
            .add_panel("hello", "Hello", hello)
            .panel_icon("star")
            .add_command("reset", "Reset", |mut count: ResMut<Count>| {
                count.0 = 0;
            })
            .add_menu_entry("Hello", "reset");
    }
}

fn hello() -> AnyView {
    column((
        label(resource::<Count, _>(|count| count.0.to_string())),
        button("Add one").on_click(|world| world.resource_mut::<Count>().0 += 1),
    ))
    .boxed()
}

hamlet_engine_bevy::export!(Hello);
```

- **The app** is built by `build` on the plugin's first call.
  `Declare` is a schedule run once as the engine starts the plugin and
  asks what it adds, before it is on: for the requests it makes as it
  starts, such as a watch or a subscription. Its scenes cannot be
  made there. `Startup` runs on the first call after it. Every call
  then runs `app.update()` once: `run` after `RunCommand` is
  triggered for the command and its system ran, `receive` after its
  messages are written, `serve` after the service's system, the
  panels' `input` after their handlers, and `mount` before and after
  it builds a view. `tick` moves `Time` on by its micros, and so does
  `frame` while the plugin does not `Animate`.
- **What it adds** is declared on the `App` with `HamletAppExt`:
  `add_panel` (then `panel_icon`, and `panel_padding` or
  `panel_padding_pixels`, for that panel), `add_settings_page`,
  `add_settings_footer`, `add_overlay` and `add_panel_section`, each
  with a system that returns the view; `add_command` with the system
  it runs; `add_menu_entry`, `add_shortcut`, `add_workspace`,
  `add_status_item` and `add_growing_status_item` (one that takes the
  room left on the line); `add_service` with a system given an
  `In<ServiceCall>`; and `add_subscription` to an event of another
  plugin as it starts. `declare` and `add_view` add any contribution.
- **Requests** are made with `HostExt` on `Commands` or the `World`:
  `create_asset`, `read_asset`, `update_asset`, `delete_asset`,
  `list_assets`, `hold_asset` and `release_asset`, `call`,
  `subscribe`, `emit`, `fetch`, `agents` and `desktop`, each giving a
  ticket. What comes back is a message: `Replied` with the ticket,
  `AgentsEventReceived` and `DesktopEventReceived` for a watch,
  `AssetChanged`, `AssetGone` and `EventReceived`. Each kind is read
  on its own, so what came in one call is not in the order it came
  across kinds.
- **Scenes** are components of the plugin's entities. An entity with
  a `Transform` is an entity of the scene; `Mesh3d`,
  `MeshMaterial3d`, `ForwardDecal` (laying its `MeshMaterial3d`, a
  decal's, from `Material::decal`, `add_decal_material` or
  `add_shaded_decal`),
  `SceneLight`, `Shadows`, `SceneLabel`, `Pickable`, `SceneCamera`,
  `PostPass` and `Visibility` (hidden with what is under it) on it,
  `ChildOf` for its parent, and
  `Instances` on an entity of its own are sent in `PostUpdate` as they
  change, and only then. `SceneKey` names an entity for the engine;
  several may share one, and a message names the last made that is
  still there.
  Meshes, materials, shaders, buffers, textures and compute are made
  with the WIT's own makes and kept in `SceneAssets` while a clone of
  their `Handle` is held; after the last is dropped the asset is let
  go in `Last`, and the engine takes it away once nothing draws with
  it. Despawning an entity takes it away in the engine. What the
  person does over a view comes as `SceneOver`, `SceneOut`,
  `SceneClick` (its `Hit` with the button and `Modifiers`),
  `SceneMove`, `ScenePress`, `SceneScroll`, `SceneResize` and
  `SceneKeyInput`, and each again as a `SceneInput` in the order it
  came, and a file dropped as `SceneDrop`; `viewport(camera)` shows a
  camera in a panel (see [Scenes and GPU work](#scenes-and-gpu-work)).
  `SceneLabel` with a `LabelStyle`, `SceneBalloon` and the
  `PointerWords` resource show words and balloons over the scene. A
  `Card` (a view built given its entity) is shown at its `CardPlace`,
  `Pointer`, `Here` or `Over` an entity, a `CardClosed` when one
  placed here is closed; `open_menu` (`MenuExt` on the `World` and
  `Commands`) opens a menu of `menu_item`s at the pointer, and
  `MenuShown` holds while it is open (`hamlet_engine_bevy::float`).
- **Sounds** are Bevy's audio as the SDK has it
  (`hamlet_engine_bevy::audio`): `Sound::new(samples, channels,
  rate)` makes a clip in the engine, kept in `SoundAssets` while a
  clone of its `Handle` is held, and `AudioPlayer` plays one as its
  `PlaybackSettings` say (`ONCE`, `LOOP`, `DESPAWN` or `REMOVE`, with
  a volume, a speed, a `Category` and whether it is spatial), in
  `PostUpdate`. A spatial one plays at its entity, an entity of the
  scene, following its `Transform`, heard from the
  `SpatialListener`, else from the first `SceneCamera`. Changing its
  settings changes its volume and speed, a `FadeTo` moves its volume
  evenly over seconds in the engine, and taking its player away or
  despawning it stops it; one that does not loop ends when the
  engine says it ended. A sound's samples are handed over in parts
  of a quarter of a MiB in `PostUpdate`, so a sound of any length is
  made at once and may play while it is still handed over.
  `Sound::open(channels, rate)` is filled with `SoundAssets::append`
  until `SoundAssets::close`; `Sound::stream(channels, rate)` is
  filled the same way and played once, its samples let go of in the
  engine as they are heard, for music (see [Sound](#sound)).
- **Views** are written with fynix (`hamlet_engine_bevy::ui`) over the
  plugin's Bevy `World`. `Hamlet` is fynix's backend: a node is made
  in the engine as fynix spawns it, of the kind of its element, and a
  property still at its first value is not sent. A view is built each
  time its panel or page is shown, from the world as it is, and
  `resource` and `component` signals read the world by Bevy's change
  ticks. The elements are `label` (and `heading`),
  `button`, `icon_button` (an icon and a label in a button), `icon`,
  `toggle` (and `switch`, one with words beside it), `field`,
  `dropdown`, `divider`, and the holders `column`,
  `row`, `list` and `scroll`; a column or row's `grow` has it take
  the room left; `keyed` builds a view again when its
  key changes, and `each` a view per item of a list. `on_click`,
  `on_toggle`, `on_commit`, `on_submit`, `on_pick` and `on_top` run a
  handler with the whole world, so it can change resources, make
  requests or trigger observers; a node holds as many as it is given.
  `when::<Hovered, _>` and `when::<Pressed, _>` hold rules while the
  engine says so. `description(text)`, or
  `.rules(description_rules)` on any view, is the description style
  of the shell's, run in the plugin as a set rule on its labels: the
  `Small` size, not bold, the `Dim` tone and the body face, which
  wraps. A call site's value still wins over it. `tip` gives a view
  words shown near the pointer while it rests on it, and `menu` (on a right-click) and
  `menu_on_click` (on a click too) a menu of `menu_item`s, each
  running a handler when it is chosen, `under` a group or `critical`.
  `pointer` has a view ask for a `NodePointer` of the engine's set,
  a value or a signal (see [Pointers](#pointers)).
  A button's `folder` and `on_folder` have the person choose a
  folder in the system's dialog. `leads_to` gives a view an address that
  the plugin tells the desktop service of (`Hover`) while the pointer
  is over it, so the status line shows it. `set_status`,
  `show_overlay`, `set_section` and `open` change what the plugin
  added outside its views. `transition` hands each change of a tone,
  size, opacity, space or offset to the engine with its motion, once,
  and the engine moves it over its own theme's curve; the plugin is
  not called each frame for it. What a call changes is shown once the
  call ends.
- **Saved across a reload**: each resource added with
  `init_saved_resource`, a serde type, is saved by the running version
  and restored into the next before its `Startup`. A plugin derives
  serde's traits with serde as a dependency of its own, or with the
  SDK's `hamlet_engine_bevy::serde` and
  `#[serde(crate = "hamlet_engine_bevy::serde")]`.
- **The system.** A plugin is given WASI 0.2 that grants nothing but
  the wall and monotonic clocks and randomness, so the standard
  library's `Instant`, `SystemTime` and `HashMap` work. What it prints
  goes to its log, stdout as info and stderr as warnings. It has no
  folders, no environment or arguments, an empty stdin, and no sockets
  or lookups; the network is `fetch`.
- **A built plugin** is a folder with its `plugin.toml` and its
  component, `plugin.wasm`.

### The samples

The samples under `engine/samples` are each a workspace of its own,
written with `hamlet_engine_bevy` as the bundled plugins are.

`engine/samples/hello` is a panel with a button and a count of its
presses, a resource of its world, and a command in its own menu and
on Primary+Shift+R that sets the count back. Its count is a saved
resource, kept across a reload. Its crate's library is named `plugin`,
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
messages come. Notes keeps its note across a reload. Notes makes the type `notes.note` (its bytes are the
text) and keeps one note of it, edited in a field and saved with
Save, an icon button; it exports the service `words`, which counts
the words of a text, and sends `saved` with the note's id and
version. Board uses `notes.note`, holds the note by reference and
shows each version as it is saved in a list, calls `words` for its
count, and sends `seen` with the version it shows, which Notes hears
and shows. Board asks for `assets.read notes.note` and
`services.call hamlet.notes/words`, and needs `hamlet.notes ^0.1`.

## Testing a plugin

`hamlet_engine_harness` runs the engine without a window, on a home of
its own made empty first, with fetches never sent. `Harness` installs
built plugins before the first frame, answers for the person (`allow`,
`answer_use`), runs frames (`frame`, `frames`, `until`), acts on
widgets (`click`, `write`, `toggle`, `hover`), runs commands, reads a
panel as a
text outline (`outline`), reads assets, replies to fetches (`reply`),
and reads the log. What it waits for it waits `[harness] patience`
frames at most, `pause` milliseconds apart, not counting the frames
while a plugin compiles or waits its turn to start.

A plugin's `steps.toml` is a script for it: `with` lists other built
plugins it needs, from its folder, its dependencies among them, and
each `[[step]]` does one thing:
`allow`, `admit` (with an answer for every ask), `answer` (a first
use), `frames`, `expect` and `lacks` (text in a panel's outline),
`click`, `write`, `toggle`, `run`, `expect_asset`, `expect_log`,
`reply` or `fail` for a fetch, `expect_scene` (how many entities,
meshes, materials, cameras and decals a plugin's scene holds) and `pick` (a
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

A panel, settings page, footer, overlay or panel section is built as
the dock, Settings or the window builds it when it shows it, an
overlay whether it is shown or not and a section whether it holds
anything or not. Its outline is a line for each node, `#` and its
key, indented under the node that holds it, and a line for each text
it shows. A widget is named by text it shows: `click` presses the
button whose text it is, `write` leaves text in the field whose
placeholder it is, `toggle` switches the switch nearest it, and
`hover` puts the pointer over a watched node by its text.

The test `the_samples_pass_their_steps` (ignored, as it builds the
samples) runs the scripts of hello, notes and board.

## The core

- **Tuning.** `engine/tuning/engine.toml` holds the sizes, timings and
  guards, and `theme.toml` the shell's theme. Both are built into the
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
  `plugin_limits.toml`, `desktop.toml`, `sound.toml`, the installed plugins in
  `plugins/` and the asset store in `assets/`, and is the agents' home
  too: their settings, records, sessions and the chief's
  `limits.toml`. The engine's files are named so none is one of
  theirs.
- **Writing a file whole.** `write_whole` writes a file beside its
  place as `saving_<name>` and moves it over the old one, so a stop in
  the middle leaves the old file or the new one. The workspaces, the
  grants, the plugins switched off, the person's plugin limits and the asset
  store are written with it.
- **The log.** A `LogLine` message, from a part of the engine or a
  plugin, is passed to the terminal and kept in `Log`, the last
  `[log] keep` lines, for the console.
- **Guards and limits.** `[guards]` in `engine.toml`, as the
  `Guards` resource, holds what keeps any plugin from crashing or
  freezing the engine: how deep a node may sit, and how long a call
  may hold the frame with no call time set (see [Limits](#limits)).
  None of it holds a plugin back. `[limits]`, as `StartingLimits`,
  holds the value each limit starts at when the person first
  switches it on; no limit is on until they do. The limits switched
  on for a plugin are `ActiveLimits` on its entity. A `hung` that is
  not a finite number above zero is refused, as a file that cannot be
  read is.

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
  fill, shaded toward the bottom, and `view.in_panel_padded(pixels)`
  the same inside other padding; a `PanelInset` beside the `Panel`
  is the padding a plugin declared for it. `tree.open_panel(id)` (`DockTreeExt`)
  brings a panel to the front.
- **The console** (`engine/console`) lists the log, newest first.
- **Settings pages** are entities with a `SettingsPage`, as plugins add
  them: a title, an icon and a line on what it holds. The Settings
  panel (`engine/settings`, opened from a tab bar's "+") shows a link
  row to each in the order of their plugins (see
  [the order of what plugins add](#commands-menus-and-the-status-line)),
  and under them each
  `SettingsFooter`'s content; the page gone through is shown under a
  head whose SETTINGS leads back. `world.open_settings_page(page)`
  (`ShellWorldExt`) shows one and brings Settings to the front. The
  shell's own are Sound (see [Sound](#sound)), Shortcuts (see
  [Shortcuts](#shortcuts)) and About.
- **Interface sounds** (`sounds.rs`) are the shell's own (see
  [Sound](#sound)).
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
  Besides: `field`, `switch` (a track whose knob glides to its end
  while on, in the accent, flipping on a press as a button does,
  `labelled_switch` with words beside it, and `toggle` the same as
  `switch`), `dropdown`, `choices`, `heading`, `section`,
  `caption`, `note` (a short aside: a value, or what an empty list
  says), `description`, `card`, `list` (its rows each a `list_row`),
  `link_row` and `link_rows`, `page_head`, `breadcrumb`, `divider`,
  `buttons` (a row that wraps), `scroll_area` and `title_button`.
  `breadcrumb(trail, current, go)` shows where a page is within a
  panel that has pages under it: each page above it, from the top, as
  a quiet button in the display face, a dim `/` after each, and the
  page shown, not a button, in the tone of a head; a press hands `go`
  the place in `trail` of its page. `StyleExt` wraps any
  view:
  `styled(kind)` as a button, `with_icon(glyph)`, `in_tab(shown)` and
  `in_panel()` or `in_panel_padded(pixels)`. A `BottomGradient`
  surface, as a panel or the bar, shades toward `background_bottom`.
- **Descriptions.** Lengthy text that explains, such as what a plugin
  is about, why it needs something or what a setting does, is in the
  description style: `description(text)`, or
  `label(text).rules(description_rules)`, which also reaches every
  label in a view it wraps. It sets each label to the small size, not
  bold, in the dim tone (`[text] small`, `text_muted`), and has it
  wrap. It is a set rule scoped to the view, the innermost there is,
  so of fynix's layers it sits after the theme's defaults and the set
  rules around it, and before what a call site writes and the show
  rules: a `.tone(..)` on the label still changes it, and a state
  rule on a button around it, such as a row lit under the pointer,
  wins too.
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
- **Scroll fades** (`fade.rs`): a `ScrollFade` scroll node fades
  each edge with more past it into the surface under it, easing in
  and out as it scrolls. Each fade is a `BackgroundGradient` node over
  its edge, out of the layout, under the scroll bars and passing the
  pointer through, `scroll_fade` deep. Its colour is the theme role
  `FadeSurface` names: `Background`, shaded as the `BottomGradient`
  panel it is on, `Surface` for a card, or `Sunken` for a track. Every `scroll_area` has
  one on the panel's background, `scroll_area_on(surface, ..)` on
  another surface, so plugin scrolls have it too; a stuck scroll at
  its end shows no fade at its bottom.
- **Plugin panels** and settings pages are built node by node from
  the same widgets and styles.
- **Icons** (`icons.rs`) are one family of line drawings made in code
  at start, so there is no file or font to load: a `Glyph` each, drawn
  at 12, 14, 16 and 20 pixels and tinted by a tone. A plugin names one
  by `Glyph::name`.
- **Pointers** are drawn in code too: see [Pointers](#pointers).
- **Stills.** `cargo run -p hamlet_engine --example stills -- --out
  <dir>` draws the widgets in each state, the icons, the shell with
  a menu open or Settings shown, and the workspace switcher with 1, 3
  and 9 workspaces, a tab under the pointer and one being renamed, and
  the Shortcuts page with keys of the person's, a chord two commands
  share and one asking to swap or replace, into PNGs with no window;
  `--scene <name>` draws one.

### Pointers

The pointer is the chief's taper, whose flame is the point, and the
chief's glove for what is held (`pointer.rs`). Each mode is a picture
of 32 pixels drawn in code in `pointer/art.rs`, with no file, in the
theme's colours, again whenever the theme changes, and set on the
window as Bevy's `CustomCursor::Image` with a hot spot of its own.
Until the pictures are drawn, or in an app without images, the
system's cursors fynix asks for are shown.

| Mode | Picture | Where |
|---|---|---|
| `Point` | The lit taper | Where nothing else holds |
| `Press` | The flame flares | A button, a switch, a row, a tab, a menu's item |
| `Pick` | A glass on a brass handle | A thing of a scene that can be selected |
| `Grab` | The open glove | A dock tab |
| `Carry` | The closed glove | A dock tab being dragged |
| `Slide` | Four arrowheads | The ground of a view being slid |
| `Turn` | Two curved arrows | A view being turned |
| `Write` | An upright bar | A field |
| `Resize(axis)` | A two-headed arrow | A dock splitter, along its axis |
| `Refuse` | A barred ring | A button that is disabled |
| `Wait` | A flame with fireflies going round it, eight pictures | For a plugin to show |

`NodePointer`, a mode and whether it is drawn faint (at 40% of its
alpha, so what it holds is seen through it), on a UI node asks for its
pointer. The pointer shown, in `PostUpdate`, is the one of a drag of
the dock, a tab or a splitter; else that of the node under the mouse
or the nearest node above it that asks for one: its `NodePointer`, a
system cursor fynix gave it turned into a mode, or `Press` for a
button (`Refuse` while it is disabled); else `Point`.

A plugin asks with the node property `pointer` (`.pointer(..)` in
the SDK, a `NodePointer` or none, as a value or a signal), on any of
its nodes, the frame showing a viewport included. It chooses only from
the modes, never a picture, so it cannot draw a pointer of its own.
The realm asks over its viewport for `Carry`, faint, while a town is
in hand and `Refuse` where it cannot be set down, then `Slide` or
`Turn` while a drag moves the camera, then `Pick` over a robot, a
house, a common, the mascot or the Hearth.

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
  they come back with their plugin. An area that keeps a panel it held
  keeps its id, and each panel left in it its tab's, so their views are
  not built again; an area that comes or goes builds the dock's layout
  again. A workspace with no panels shows one empty area, whose "+"
  adds one.
- **Kept.** What the person changes in the dock by hand (a tab moved,
  closed or added, a split dragged) is kept in the shown workspace.
- **Saved** in `workspaces.ron` in the home, once the workspaces have
  been still for `[workspaces] save_after` seconds, and as the engine
  closes, whole or not at all. RON, since a layout is a tree of enums,
  which RON writes as it is and TOML only as a table per level.
- **Restored** at start. A new home starts with one workspace, "Main"
  (`Workspaces::default`): the Realm panel in the middle with most of
  the room, Petitions and the plugin list as tabs on its left, and Chat and
  the Inspector over the console on its right. A bundled plugin's
  panel that is not there is left out until it comes, so without
  bundled plugins "Main" is the plugin list and the console. A file that cannot be read is
  moved aside to `workspaces_broken.ron` (or `_2`, `_3`, never over an
  earlier one), the console says so and where, and the workspaces start
  afresh. If it cannot even be moved, nothing is saved, so the file stays
  as it is.
- **The switcher** (`switcher.rs`, its motion in `switcher/motion.rs`)
  is one control in the bar, set apart from the menus by a thin divider
  (`[space] track_divider` high) and `section` of space: a track sunk
  into the bar (`surface_sunken`, a `border` frame, `track_inset` from
  the bar's edges, `track_padding` inside, `track_radius` corners) with
  a segment per workspace, its name in capitals in the
  display face (`[text] heading`). The one shown sits on a raised
  segment (`fill`, lit inside from below by `active` at `[glow]
  bottom`) with an `underline` line in `accent` under it, glowing; the
  segment glides from tab to tab over `[motion] interact`, at once
  under reduced motion. The others are dim and brighten under the
  pointer with a row's inner light (`Glow::Inner`), ticking; a press
  taps, and so does showing another by a key.
  - A press shows the workspace; the tip under the pointer names its
    key. Under the pointer an "x" after the name asks whether to
    close it, unless it is the last.
  - A double-click (two clicks within `[workspaces] double_click`
    seconds) renames it in place: a field over the tab, as wide.
    Enter keeps the name; Escape, a press outside or the keyboard
    going elsewhere leaves it. A name is trimmed, at most 40
    characters, and no other workspace's, whatever the case.
  - A right-click offers Rename, Duplicate (a copy after it,
    numbered), Move Left, Move Right and Close, each where it
    applies. Close, the "x" and the Close workspace command all ask
    first, in a popup under the tab with Close and Cancel.
  - Dragged past `[workspaces] drag_start` pixels, a tab follows the
    pointer and the others ease aside to leave a gap where it would
    land; let go, it moves there.
  - "+" is the track's last segment: a press adds a blank workspace,
    "Workspace" and a number, shows it and opens its name to be typed.
    The chevron beside it, or a right-click on "+", offers New
    workspace and the layouts plugins suggest ("New from layout...").
  - Past what fits, the tabs scroll across (by wheel too) with a
    `ScrollFade` at each edge into `surface_sunken`, the shown one is
    scrolled into sight, and a chevron at the end lists every
    workspace with its key.
- **Commands.** New (⌘T, Ctrl+T off macOS), rename, duplicate and
  close the shown workspace, next and previous, and Workspace 1 to 9
  are commands with no menu in the bar: the switcher holds them. ⌃⇥
  (Ctrl+Tab) shows the next workspace and ⌃⇧⇥ (Ctrl+Shift+Tab) the one
  before, going round; ⌘1 to ⌘9 (Ctrl+1 to Ctrl+9 off macOS) show the
  first nine. The tips write each key in force as the menus do.
- **Suggested.** A `SuggestedWorkspace`, as a plugin adds one, is
  offered under "+" while it is there, with its command "Take
  workspace" and its name. Taking it adds a copy, numbered if the name
  is taken, and shows it. The copy is the person's, to change or
  delete, and stays when the plugin goes.

## Commands, menus and the status line

- **A command** is an entity with a `Command`: its id (`engine/quit`,
  `hamlet.demo/hello`) and its label. Its owner observes `RunCommand` on
  it. `world.run_command(id)` (`ShellWorldExt`) runs one by id;
  `commands.spawn_command(..)` (`ShellCommandsExt`) spawns one with its
  menu entry and keys as its children.
- **A shortcut** is an entity with a `Shortcut`: a chord a command
  comes with and the command's id; a plugin's come from what it
  declares. `parse_chord` reads keys written as `Primary+Shift+H`:
  Primary is Command on macOS and Control elsewhere, and
  `chord.keys()` (`ChordExt`) writes a chord back so.
- **The keys in force** are `Bindings` (`keymap.rs`), built again as a
  command, a shortcut or the person's keys change: for each command,
  its own chords, or the person's in their place. A key press runs the
  first command bound to its chord, those with shortcuts first, unless
  a bare key goes to a text field being typed in or a key is being
  bound on the Shortcuts page. `bindings.label(id)` is a command's
  first chord as the platform writes it, for menus and tips.
- **How a chord is written** is fynix's: on macOS in symbols with
  nothing between, the modifiers in the order ⌃ Control, ⌥ Option,
  ⇧ Shift, ⌘ Command, then the key, as ⇧⌘T, and special keys too
  (⏎ ⌫ ⌦ ⎋ ⇥ ⇪ ← → ↑ ↓ ↖ ↘ ⇞ ⇟); elsewhere names joined by `+`, as
  Ctrl+Shift+T. Inter has every one of these glyphs.
- **A menu entry** is an entity with a `MenuEntry`: the title of its
  menu and the id of its command. The bar gathers the entries into
  menus, the engine's own first (Hamlet, View) and then the
  others in the order of their entries. A menu's title, in capitals, opens it
  under itself and stays lit while it is open; a press on another title
  opens that one instead. An entry shows its command's label and first
  key. An entry whose command is not there is left out.
- **The engine's own.** Run in Background and Quit Hamlet (⌘Q,
  Ctrl+Q off macOS) in the Hamlet menu and the workspace commands (see
  [Workspaces](#workspaces)). A panel has no entry in the View menu: it
  is opened from a tab bar's "+" (`world.open_panel(id)`).
- **The order of what plugins add.** What a plugin adds carries its
  `StartRank`, its plugin's place in the order plugins start (see
  [Dependencies](#dependencies)). Menu entries, status items and
  settings pages and footers are listed the engine's own first, then by
  their plugin's rank, then in the order they came, so they are the same
  at each start.
- **No status line.** The window has none at the bottom: what it
  showed is in the console. A plugin can still add a `StatusItem`
  (`status.rs`), but nothing draws it.
- **In the background.** Run in Background hides the window and the
  engine runs on, a frame every `[window] hidden_wait` seconds.
  `ShowWindow` shows it again and gives it the focus. The window's
  close button quits.

### Shortcuts

- **The page.** Shortcuts in Settings (`engine/shell/src/shortcuts.rs`,
  the bolt icon) lists every command, the engine's and the plugins',
  under the title of its first menu, or else by what added it (the
  engine's by what their name starts with, as Workspace), each with
  its keys in force. A search over it keeps the commands whose label
  holds what was typed.
- **Rebinding.** A press on a command's keys shows "Press keys"; the
  next chord pressed, with what is held, is its only one. Escape
  leaves it as it was, a modifier alone waits for more, and a key
  `parse_chord` has no name for is passed over. Until that key is up
  the chord runs no command.
- **Clear and reset.** The cross takes every key from a command; the
  arrow turning back, shown on one the person bound, puts its own
  back; RESET ALL, shown while any is bound, puts back every one.
- **Conflicts.** A chord pressed that another command has is not
  bound at once: under the row it says which command has it, with
  SWAP (the other takes this one's first key in its place), REPLACE
  (the other loses it) and CANCEL. A chord two commands are bound to
  anyway, as their own may be, is shown in red with "Also bound to"
  the other; only the first answers.
- **Kept** (`Keymap`) in `shortcuts.toml` in the home: under
  `[rebound]`, each command the person bound by its id, with its
  chords as `parse_chord` reads them (none for one cleared). Only what
  differs from a command's own is kept, so a key a command comes with
  later reaches whoever did not bind it; a binding of a command not
  there, as one of a plugin that is off, is kept for when it comes.
  It is written whole as it changes and applies at once. A file that
  cannot be read is left as it is, the log says so, and the keys are
  kept only until the engine closes.

## Plugins

`hamlet_engine_plugins` keeps each plugin as an entity.

- **A plugin** is an entity with a `PluginInfo`: id, name, version,
  and its `PluginKind`, built in (Rust, in the binary), bundled (WASM
  that comes with Hamlet) or installed (WASM the person added). A
  plugin built in has an `EntryPoint` too. Two plugins of one id are
  not kept: the second is turned away, and the log says so.
- **The entry point** is one function, given an `Entry` to declare what
  the plugin adds: panels, commands, menu entries, shortcuts, settings
  pages and footers, suggested workspaces, status items, overlays and
  panel sections. It never reaches the app or the world. A panel
  section (`PanelSection`) names an engine panel (`engine/name`), and
  one for a plugin's panel is left out.
  A name in it is the plugin's own; a command is named by its name, or
  by its full id for another's.
- **What it adds** is an entity each, linked to the plugin by
  `ContributedBy`; the plugin's `Contributions` lists them, and goes with
  it (`linked_spawn`). A plugin adds as much as it declares; what is
  not well named is left out and told in the log.
- **On and off.** Triggering `Enable` on a plugin marks it `Starting`;
  at its turn (see [Dependencies](#dependencies)) its entry point runs,
  what it declares is added, and it is marked `Enabled`. `Disable`
  despawns everything it added. Its panels leave the dock but stay in
  the workspaces, and come back when it is switched on again.
- **Stopped.** A plugin the engine stopped while it was on carries
  `Stopped` with why; what it added stays until it is switched off.
- **Built in.** `commands.add_builtin(manifest, entry)`
  (`PluginCommandsExt`) adds a plugin linked into the binary and
  switches it on, unless it is kept off. A manifest that `check`
  refuses is told in the log and not added.
- **The Plugins panel** (`engine/plugins`) is the engine's own. It
  shows the list of plugins or one plugin's page, as `Shown` in
  `list.rs` says, and goes back to the list when the plugin of the
  page shown goes.
- **The list** shows each plugin's name, with its id and version,
  whether it is on and how much it adds, starting, or why it was
  stopped or cannot run, under it, and the switch that turns it on or
  off at its top right, on while it is switched on even when it
  cannot run; a
  bundled one says it comes with Hamlet. A plugin that is off is shown
  in the theme's dim tone, all of it but its switch. A row follows its
  plugin in place, so a switch glides as it is pressed and the other
  rows stay as they are. A press on a
  plugin, or its chevron, opens its page. ADD PLUGIN... above them
  triggers `AddPlugin`, for a host to answer. Under it, "Search
  plugins" keeps those whose name or id holds the words, of any case,
  once they are entered, and the dropdown beside it those that are on,
  off, come with Hamlet or are installed; the two combine, an empty
  search shows all, and when none is left the list says "No plugin
  matches." Both are kept until the engine closes, above the list as
  it scrolls.
- **A plugin's page** (`list/page.rs`) is under the breadcrumb
  "PLUGINS / its name", whose PLUGINS goes back to the list and which
  stays as the rest scrolls: a head with its name, its id and version,
  where it comes from (built into the engine, comes with Hamlet, or
  installed) and its state, with its switch at the right; LIMITS (see
  [Limits](#limits)); WHAT IT ADDS, each panel, settings page and
  footer, command, menu entry, shortcut, status item, overlay, section
  and workspace it added, nothing while it is off; NEEDS, each plugin
  it needs with its requirement, "Met" or why not; NEEDED BY, the
  plugins that need it; then each
  `PluginPageSection` another part adds, given the plugin's entity,
  in the order they came, SOUND among them (`mute.rs`, a switch that
  mutes its sounds); and for an installed one UNINSTALL..., which
  says what goes and which plugins on stop with it and asks again,
  UNINSTALL triggering `Uninstall` and showing the list, CANCEL not.
- **Switches that move others.** Switching off a plugin that plugins
  on need, however indirectly, asks first under its row and on its
  page: "Switching off X also stops Y and Z.", with SWITCH OFF and
  CANCEL. Switching on one that needs plugins that are off offers to
  switch them on too: SWITCH ON ALL, ONLY THIS ONE (it waits, blocked,
  until they come) or CANCEL.
- **Kept off.** `SwitchedOff` holds the ids the person switched off in
  the Plugins panel, kept in `switched_off.toml` in the home
  (`plugins = ["hamlet.demo"]`). Only the switches there change it;
  the engine stopping a plugin, or a hot reload, does not. At
  start a plugin on it is left off, of any kind. Letting an install
  in, and uninstalling, take its id off. A file that cannot be
  read is left as it is: the log says so, and no plugin is kept off.
- **`commands.switch_on(..)`** adds what an `Entry` declares, at the
  plugin's `StartRank`, and marks the plugin on.
  A built-in plugin's entry point runs through it, and so does a WASM
  plugin once it has said what it adds.
- **A command's function** is given the world and the command's
  entity, whose `Command` and `ContributedBy` say which it is and
  whose.

### Dependencies

- **Declared.** A manifest's `[dependencies]` maps the id of each
  plugin it needs to a semver requirement of its version:

  ```toml
  [dependencies]
  "hamlet.agents" = "^0.1"
  ```

  `^0.1` takes any 0.1.x, so 0.1.0 fits. `Manifest::needs` gives them
  as `Dependency`s, and `PluginInfo` holds them. The engine holds one
  copy of each plugin, so a dependency is one plugin of one version.
- **Resolved** (`needs.rs`, the system `resolve`) as plugins come, go,
  start, stop or are switched. A plugin switched on (`Enabled`,
  `Starting` or `Blocked`) runs only while each plugin it needs is
  there, of a version that fits, switched on, not stopped and able to
  run itself. Else it is `Blocked` with why, the first unmet in id
  order: "Needs hamlet.agents ^0.1, which is switched off", "...,
  which is 0.2.0", "..., which isn't installed", "..., which isn't
  running", "..., which was stopped" or "..., which cannot start". A
  plugin running when it becomes blocked is stopped (`Disable`, its
  switch left on), and so, in the same frame, is each that needs it,
  however indirectly. Plugins that need one another, however
  indirectly, are each blocked with "In a cycle of dependencies:" and
  the id of each plugin of the cycle.
- **Back on its own.** When what it needs is there again, switched on,
  installed, restarted or reloaded to a version that fits, a blocked
  plugin is started again (`Enable`). The log says when a plugin
  cannot run and when it has what it needs.
- **The start order** (`StartOrder`) is every plugin by its depth, 0
  for one that needs no plugin there, else one more than the deepest
  it needs, and then by id. Its place is its `StartRank`. A plugin
  `Starting` and not blocked is on its way; plugins on their way start
  (a built-in plugin's entry point runs, a WASM plugin is instantiated
  and declares what it adds) in that order, each only once every one
  before it is not on its way any more. Components still compile in
  parallel: one compiled early waits its turn. So plugins start in the
  same order at every start, and what they add comes in that order.

### Limits

- **Full speed by default.** A plugin runs with no limit at all: no
  cap on its memory, no deadline on a call, nothing it may spend in a
  second, no count of what it holds, makes or asks, no GPU time held
  back. Nothing makes it wait, skips its work or stops it because it
  spends a lot.
- **Only the person turns a limit on**, for one plugin at a time.
  Each is `LimitKind` (`engine/api/src/limits.rs`), and once on, the
  engine holds the plugin to it:
  - **Memory**: bytes of all its memories summed; a growth past it
    fails the call and stops the plugin.
  - **Time of one call**: milliseconds, through the epoch; a call
    past it stops the plugin, "a call ran past the call time you
    set".
  - **CPU time a second**: milliseconds its calls take in a second;
    past it, what is in its inbox waits for the next second, and the
    log says so the first second it waits.
  - **GPU memory**: bytes of its meshes, buffers and textures as the
    engine counts them; a make past it fails, saying why.
  - **GPU time a second**: milliseconds of its compute and passes,
    where the GPU has timestamp queries; past it, they are held back
    until its scene is made again (see
    [Layer two](#layer-two-the-plugins-own-gpu-work)). Only such a
    plugin's work is timed.
  - **Scene entities**: entities of its scenes, each copy of
    instances counted; a make past it fails.
  - **Audio memory**: bytes of the samples of its clips; a make past
    it fails.
- **A plugin cannot ask for limits**, for itself or another: a
  `plugin.toml` with `[limits]` is refused.
- **On a plugin's page**, LIMITS lists each, a switch with its name
  beside its value: bytes as a number and a unit, KB, MB or GB of
  1,024 of the one before, in the largest unit it holds one of (64
  MB, 1.5 GB), milliseconds with "ms", entities as a count. With none
  on it says "None is on, so nothing holds it back." Switching one on
  holds the plugin to the value shown: the person's last, else
  `[limits]` in `engine.toml` (`StartingLimits`). A number typed, or
  another unit picked for the same number, becomes its value, on or
  off; one that is not a number of one or more is refused and said so
  under it. Switching it off keeps the value for next time.
- **The person's limits** are kept in `plugin_limits.toml` in the
  home, by plugin, each on or off with its value, and outlast
  reloads, new versions and restarts; uninstalling a plugin forgets
  them. Any value above zero is taken, with no ceiling. The file is
  written whole or not at all; one that cannot be read, that names
  anything else or a value of zero, is left as it is, the log says
  so, and the limits are kept only until the engine closes. The
  agents' `limits.toml` beside it is theirs, and never read here.

  ```toml
  [plugins."hamlet.realm"]
  memory = { on = true, value = 268_435_456 }
  call_time = { on = false, value = 1_000 }
  ```

- **In force.** The limits switched on are `ActiveLimits` on the
  plugin's entity, given as the plugin comes or its manifest changes,
  and again when the person's change. A change applies at the
  plugin's next call; GPU time within the frame.
- **What stays**, for every plugin alike. None is a limit on what a
  plugin spends; each keeps one plugin from crashing the engine,
  losing the device or getting past the sandbox:
  - **The sandbox**: a store of its own, WASI that grants nothing, and
    wasmtime's checks of every pointer and length a plugin hands
    over against its own memory.
  - **Capabilities and grants**: every power is asked for and
    answered (see [Permissions](#permissions)).
  - **The device's limits**: naga validation of every shader with
    wgpu's runtime bounds checks, workgroups and their sizes along
    each axis, the largest buffer and texture, and the samples a
    camera may ask for. Past them the device would be lost.
  - **Numbers**: values that are not finite are refused where they
    would move a scene, a sound or a layout, offsets are held to
    100,000 pixels, and counts saturate rather than overflow.
  - **`[guards] node_depth`**: how deep a node may sit under its
    panel's root. The layout of the interface is recursive, and a
    deeper tree would overflow its stack.
  - **`[guards] hung`**, 10 seconds: see
    [A call that hangs](#a-call-that-hangs).

### A call that hangs

A call into a plugin runs on the frame. While one holds the frame,
nothing else reaches the engine: no click on the plugin's page, no
menu. So a plugin with no call time set is stopped by the epoch once
one call has held the frame for `[guards] hung` seconds, "a call held
Hamlet for 10 seconds without returning". Epoch interruption stays
compiled in for this, and for the call time the person may set: the
ticker thread moves the epoch on every `[wasm] tick` milliseconds,
and the call past its deadline traps. A wait of the system's clock
ends by the same deadline. A plugin whose calls are long but return
can be switched off on its page between them, as any plugin can.

## Permissions

`hamlet_engine_grants` keeps what the person lets each installed plugin
do. Plugins built in are not asked about, and bundled ones are allowed
all they ask (see [Bundled plugins](#bundled-plugins)).

- **Asks.** A plugin's manifest names each capability it asks for, its
  target and a reason in its own words, shown with it. Each is answered
  Allow, Ask next time or Don't allow. Powers that are never granted
  (keys, secrets, answering for the person, the raw home, running
  processes, turning the sandbox off) are refused when a manifest
  asking for one is read: `never_granted` names them by any word of a
  capability, so `keys.read` and `agents.keys` are both refused.
- **Petitions** are questions for the person, each a `Petition` on a
  child of the plugin's entity, so they go with it. The log says when
  one comes, and the Petitions panel is opened in the dock.
- **The Petitions panel** (`engine/petitions`) holds everything that
  waits on the person. Its heading counts it all, "NOTHING NEEDS YOU",
  "1 THING NEEDS YOU" or "n THINGS NEED YOU", and under it each part
  that holds something: first the sections plugins add, those of
  plugins that come with Hamlet first, then PLUGINS, a card for each
  petition. With nothing, it says "Nothing waits for you. Petitions,
  objections, questions and faults come here, oldest first." A
  section of an installed plugin is headed by its name too ("AGENTS
  FROM ITS NAME"), so it never passes for the engine's, and only the
  engine lists and answers the petitions. Its status item, a bell,
  says how many wait.
- **Old ids** answer for nothing. The Permissions page leaves out what
  a plugin that no longer comes with Hamlet was allowed; ids in
  `switched_off.toml` and `plugin_limits.toml` of no plugin are never
  looked up; and a workspace's panel that no plugin adds is left out
  of the dock, and out of the layout the next time it is kept.
- **Merged plugins.** Bundled plugins merged into another are named in
  `MERGED` (`engine/core/src/bundle.rs`): `hamlet.chat`,
  `hamlet.inspector`, `hamlet.skills`, `hamlet.petitions`,
  `hamlet.status`, `hamlet.settings` and `hamlet.welcome`, all now in
  `hamlet.agents`. A saved workspace's panel of one is read under its
  new id (`hamlet.chat/chat` as `hamlet.agents/chat`), so it stays in
  place. The grants file drops their records as it is read, and the
  answers to a capability there is no more of, such as `agents.keys`,
  so the rest of the file is still read. What `switched_off.toml` and
  `plugin_limits.toml` hold for them is an old id, never looked up.
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
  time. The same question is asked once while it is open. The asset
  store, the bus and the
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
- **On a plugin's page** in the Plugins panel, WHAT IT MAY DO is a
  `PluginPageSection` of this crate: whether it was let in, each ask
  with its reason and the person's answer, as they are. An installed
  one has CHANGE IN PERMISSIONS (`world.show_permissions(id)`,
  `GrantsWorldExt`), which shows the Permissions page with that
  plugin's answers alone, until ALL PLUGINS or By capability is
  pressed.

## The WASM host

`hamlet_engine_wasm` runs plugins built as WASM components, with
wasmtime. `[wasm]` in `engine.toml` sets it up; it holds a plugin
only to the limits the person switched on and to the guards (see
[Limits](#limits)).

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
  and the folder is moved into place: the folder of the installed
  plugin of that id, which it replaces, or else `<home>/plugins/<id>`,
  numbered (`<id>_2`, `<id>_3`) past any folder already there. The
  person is asked about it at once. An id of a plugin built in or
  bundled is refused.
- **Uninstall** on the Plugins panel (`Uninstall`) takes an installed
  plugin away with its folder, and the grants forget what the person
  answered for it.
- **Switched on**, only when it is `Admitted`, a plugin's component is
  compiled off the frame, on
  the async compute pool, and kept, so switching it on again needs no
  compile. What it compiles to is also kept in
  `<home>/compiled/<id>/`, named by the hash of its `plugin.wasm`, so
  the next start reads it in place of compiling; one this engine
  cannot read is compiled again and written over. At its turn in the start order (see
  [Dependencies](#dependencies)) it is then instantiated in a store of
  its own and asked what it adds. A plugin that imports anything but
  the host's interface and
  the system's (`wasi:`) is not loaded, and one built against another
  version of the interface is told so.
- **The system** (`wasi.rs`) is WASI 0.2 from wasmtime, with a context
  of its own in each store that grants nothing but the clocks and
  randomness. A wait of its clock ends at the call's deadline (see
  [A call that hangs](#a-call-that-hangs)). Every write to stdout or
  stderr is kept, and cut into lines for the log.
- **What it adds** comes back from `declare` as `Contribution`s and is
  made into the same `Entry` a built-in plugin fills, so it is added
  and taken away as theirs is: its panels, settings pages,
  footers, overlays and sections are built node by node as it says, and its
  commands put their name in its inbox, to run it on the next frame.
  Its menu entries, shortcuts, suggested workspaces and status items
  are plain data. What a call sets with `set-status`, `show-overlay`,
  `set-section` and `open` is made once the call ends (`surfaces.rs`), on what the
  plugin added under that name.
- **Switched off**, its store goes, and with it its state.
- **The engine** is one for all plugins, with an epoch, compiling
  with cranelift at `OptLevel::Speed`, a plugin's functions in
  parallel. A thread ticks the epoch every `[wasm] tick` milliseconds;
  a plugin is held by time through it, and does not count its
  instructions. Instances are allocated on demand: each is given its
  memories and tables as it is made, as large as it asks, and nothing
  caps how many plugins run. A memory still sits in a reservation
  with guard pages, so its accesses need no bounds checks. A pool
  would make instantiating cheaper, but a plugin is instantiated
  once as it starts, and a pool caps the plugins, their instances,
  memories and tables up front.
- **Every call** into a plugin is given an epoch deadline: the call
  time the person set, else `[guards] hung` (see
  [A call that hangs](#a-call-that-hangs)). What a plugin hands over
  is bounded by its own memory alone: wasmtime checks the pointer and
  length of a string or a list against it before it copies any of it,
  and the store's hostcall fuel is left unbounded.
- **Memory** is counted over all of a plugin's memories together,
  held to the memory the person set, if they did.
- **Limits change as it runs.** Each call is armed with the limits as
  they are then, so a new value reaches a running plugin at its next
  call.
- **A fault** is a call past the call time set, past the memory set,
  hung, a trap, or what cannot be read; a plugin cannot be called
  again after one.
- **The log.** What a plugin logs or prints goes to the engine's log
  under its id.
- **Requests and mail.** What a plugin asked in a call is written as
  an `Asked` for the services once the call ends, `depth` one more than
  the work it was serving. A `Deliver` for it goes into its inbox, and
  the messages at the front of its inbox are handed over together in
  one `receive`. A call of one of its
  services (a `Serve`) goes into its inbox too, and `serve` answers it
  with a `Served`. Its tickets go on counting across a reload.

### Hot reload

- **Noticed.** Every `[wasm] watch` seconds each WASM plugin's
  `plugin.wasm` and `plugin.toml`, installed or bundled, are checked, and a change is taken
  once a file stayed the same from one check to the next.
- **A new `plugin.wasm`** of a running plugin is compiled off the frame
  and swapped in by one system in the frame's update, never in the
  middle of a call: the running version's `save` gives its state, the
  new one is instantiated in a store of its own, given the state by
  `restore`, and asked what it adds. Its
  inbox and its CPU time of the second carry over. Its nodes are taken
  away and each panel or settings page shown is mounted again; when it
  adds something else, what it added is taken away and added anew.
  The log says it was reloaded.
- **A build that fails**, or a new version at fault, leaves the old one
  running, and the log says why; the same file is not tried again until
  it changes. A running version at fault while it saves is stopped.
- **A plugin that is not running** forgets its compiled component, so
  it is compiled anew when it starts.
- **A new `plugin.toml`** that changes anything adds the plugin
  again, as if it had just come: one that asks for more is asked about
  again. Its state is not kept.

### Serving

- **Every frame, everything.** Each running plugin is handed all that
  is in its inbox, and a frame while its views move, every frame. No
  share of the frame is kept for others, and nothing it sends, makes
  or asks is counted against a second.
- **The inbox** holds what the person did to its nodes, commands,
  messages, calls to serve, and panels to mount and unmount, as many
  as come. A call of a plugin that does not run fails.
- **CPU time.** The time of its calls in a second is counted (the
  wasm crate's `budget.rs`), and only matters when the person set a
  CPU time a second: past it, what is in its inbox waits for the next
  second (see [Limits](#limits)).

### Panels

- **Built node by node** (`retained.rs`). A WASM plugin's panels and
  settings pages are the same `Panel` and `SettingsPage` as a built-in
  plugin's, and carry a `Surface`: their name within the plugin, and
  why it was stopped if it was. When the dock shows a panel, or
  Settings a page, a root is built for it, inside a panel's padding
  or the one the plugin declared for the panel, and the plugin is asked to `mount` it there; when the root goes, it
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
  label, a `content_button` setting them out as `icon_button` does,
  or, a row holding nodes, them in a `ButtonStyle::Row` as `link_row`
  sets out its own, the last at its end; an
  icon one of the engine's set by its name (`plus`, `check`, `bell`,
  ... as `Glyph::name` has them), a name the set does not have shown
  as it is; a toggle a `switch`, or a `labelled_switch` when it has
  text, a field `field`, or `message_field`
  with more than one line (`message.rs`), a list `list_row`s, a
  scroll `scroll_area`, with `StickToEnd` when it sticks
  (`stick.rs`), a divider `divider`, and a dropdown
  `dropdown`. A node with a tip has fynix's tooltip, a `note` of it,
  and one with a menu fynix's context menu, a `submenu` for each
  group; a click opens it as a right-click does.
  A widget is built again when what it is built as changes, such as
  a button's style, a dropdown's options, whether it has a tip, or
  its menu.
- **Room for a scroll.** A holder is marked (`HoldsScroll`) while a
  scroll is under it, kept as nodes come, go and move. The frame of a
  marked node, and the panel's root, may be shorter than what it
  holds (`min_height` 0) and, under a column, take the room left
  (`flex_grow` 1), as an engine panel's columns do, so the scroll is
  given the panel's height and not its content's. A frame with no
  scroll under it keeps its height, and a `grow` the plugin set wins.
- **Moved by the engine.** A tone, size, opacity, space or offset set
  over a motion is moved from where it is shown over the theme's curve
  for it, a tone as its colour. A role is filled in from the theme as
  it is now, while it moves and again whenever the theme changes.
- **Inputs back.** A click, the pointer over a watched node or off it,
  a button down on it or up, a toggle switched and a field's text (on
  Enter, or as the field is left, and sent from a field of several
  lines) go into the inbox, and are handed
  over together in one `input`. A plugin that answers that something
  still moves is given a `frame` each frame until it answers that
  nothing does. Any other call that changed its nodes is given a
  frame after it too.
- **As many as it makes.** A plugin makes, sets, orders and watches as
  many nodes as it likes; only none sits deeper than
  `[guards] node_depth` under its root, and a call that makes one
  deeper fails.
- **Bounded values.** An opacity is held from 0 to 1 and an offset to
  100,000 pixels either way; one that is not finite is taken as its
  first value, so the layout never meets it.
- **Costs.** `NodeStats` counts the calls into plugins for their
  views, the calls they made on nodes, and the time of both.
- **Stopped.** A plugin at fault is stopped: its store goes, it is
  marked `Stopped` with why, the log says so, and its panels and
  settings pages show that it was stopped and why. Switching it off
  and on starts it afresh.

## Bundled plugins

Hamlet's own features are WASM plugins that come with the engine. They
are a kind of their own, apart from the plugins built into the binary
in Rust and from those the person installs.

- **The folder.** `plugins/` at the repository root is a cargo
  workspace of its own, left out of the root's, with one release
  profile built for speed, every plugin and crate alike (`opt-level =
  3`, `lto`, one codegen unit). Each plugin
  is a crate `plugins/<name>/`, named as its folder, with its
  `plugin.toml`, depending on `hamlet_engine_bevy` by path.
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

Two plugins come with Hamlet: `hamlet.agents` and `hamlet.realm`.
The engine's name and version, and the
interface's, are the About page of Settings (`engine/shell/src/about.rs`).

`plugins/agents` (`hamlet.agents`, asking `agents.read`,
`agents.chief`, `agents.settings`, `desktop.open https` and
`desktop.settings`, and sending the event `carry`) is Hamlet's own
shell over the agents service: the chat, the Inspector, the Skills
panel, the agents' section of the Petitions panel, their status
items, their settings pages and the first-run card. It holds no key:
a key is typed, and a sign-in made, on the engine's key card. Each
part is a module of its own with its own `plugin`, added by the
crate's: `chat/`, `inspector/`, `skills.rs`, `petitions.rs`,
`status.rs`, `settings/` and `welcome/`, and `shared/`, what they
share. The plugin's views are named in one list, so the section of
the Petitions panel is `petitions` and the settings page `agents`.
Built with the script, it is checked by its `steps.toml`, which
shows each panel, the section, a settings page and the card, and
opens the form FOUND A HAMLET shows; the harness runs no agents
service, so only what shows before the agents answer is checked:

```sh
cargo run -p hamlet_engine_harness --bin plugin_check -- plugins/dist/hamlet.agents --steps plugins/agents/steps.toml
```

- **Settings** (`settings/`) adds five pages to Settings
  and a footer. Endpoints: each endpoint with whether it has a key or
  is the ChatGPT plan, MANAGE USAGE for the plan, REMOVE, and ADD A
  PLACE FOR MODELS, which begins a setup that adds one. Models: the
  default model as a dropdown of the model run now and those allowed,
  LIST MODELS, FIND, what each endpoint answered, and the first 40
  models found, each with a switch to allow it beside its name, its
  prices in and out
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
- **The first-run card** (`welcome/`) is an overlay, `welcome`. It
  begins a setup at start when no model was chosen, and is shown
  while a setup is under way (`Setup::begun`), whoever began it. Its
  steps: the ways in (sign in with OpenRouter, continue with ChatGPT,
  a key, a model on this computer, or leave), looking for a local
  server (LOOK AGAIN), and the models found, narrowed with FIND, of
  which the one picked is tried. A sign-in and a key are handed to
  the engine's key card with `ShowKeyCard`, which hides once a place
  answers, showing the card's models. The setup itself, its problems
  in plain words included, runs in the service.
- **The status items** (`status.rs`) are six, from the service's `Status`: the sandbox, the
  model, the link (red when not connected), what the open session
  used, the first hamlet's spend, and whether the open session works.
  Between the spend and the work, a growing item shows the address
  under the pointer in the accent, as the desktop service tells its
  watchers. How many things wait is the Petitions panel's item. The
  harness does not read them.

`plugins/realm` (`hamlet.realm`, asking `agents.read`, `agents.chief`,
`render.scene` and `render.shader`, and needing `hamlet.agents`,
whose Inspector's `carry` it hears and whose Inspector it brings
forward) is the
realm, drawn in its panel,
Realm (`hamlet.realm/realm`, with the globe icon), from the agents
service's roster, as `crates/hamlet_realm` draws it: the endless land of [`endless-world.md`](endless-world.md) in
tiles about the camera with its rivers and trees, a town for every
hamlet with a house, a yard and a robot for every agent, the roads
between agents with their bridges and lit grooves, and the chief's
Hearth with its mascot, which goes by a Wickstep to the agent the
chief talks to, and the library (`places/library.rs`), a stone house
under a dome on ground of its own beside the first town, with its
librarian on the porch; a click on it selects the library. The sky
follows the chief's clock (`Focus::offset`).
Its tuning is `assets/world.toml` and `assets/palette.toml`, built in.
- **The source** is grouped by what it is: `world/` the land and its
  look, `places/` what stands on it, `folk/` the robots and the chief,
  `view/` the camera, the pointer and the signs, and `build/` the
  meshes, materials and transforms handed over a frame at a time.
  `realm.rs`, `bridge.rs`, `society.rs` and `tuning.rs` stand at the
  root.
- **As a Bevy plugin.** The land, the society, the roster's bridge
  and each part of the scene (the ground, the towns, the roads, the
  robots, the chief, the camera, the rings and the signs) are
  resources, and what they draw are entities with the SDK's scene
  components. `realm.rs` adds the systems: those that hear the agents
  service, the Inspector and the person over the view
  (`SceneInput`, in the order it came), and a frame's, in one chain
  run on each call that moves time on. `build/upload.rs`'s `Draw` is
  what they change the scene with. Nothing is saved across a reload.
- **The panel** has no padding, and is a viewport filling it to its
  edges, its camera's (`scene::viewport`), so it is docked, tabbed, split and closed as
  any panel is, and its signs and tags are words over that viewport.
  The realm is made at `Startup`, on the plugin's first call after it
  declares what it adds. While the panel is behind another tab or out
  of the dock, its camera does not draw.
- **A frame at a time.** Everything is built a step at a time, a
  frame at a time, nearest the camera first: `build/upload.rs` holds
  each frame to 8 milliseconds of building, so the view stays smooth,
  and a mesh with no time left waits for the next frame.
  This pace is the realm's own; the engine holds it to nothing.
- **The roster** is followed in `bridge.rs` as `main` follows the
  core's entities: a hamlet founds its town at its spot or the first
  free site, its agents take their plots in the order they came, one
  that leaves is a ruin or a wing of the house it merged into, and the
  roads and what each agent does follow theirs.
- **The camera** (`view/camera.rs`) frames every hamlet and the
  chief's Hearth, which it shows from the start, before any town is
  founded; the left button
  slides the ground, the right turns and tilts it, and the wheel comes
  nearer. Reset Camera, Focus on Selection (F), Follow Selection
  (Shift+F) and the sky held to day or night are in the View menu.
- **The pointer.** A robot, a house, a common, the Hearth or the
  mascot under it has a ring of light, as the one selected has. A
  robot is found by a box of `[pick] robot_box` about it, the nearest
  the pointer's ray enters, through a house standing in front of it,
  and a click there selects it. Each ring is
  a decal of the realm's own shader (`build/mark.rs` and `mark.wgsl`,
  as are the runes of a Wickstep). Its line is worked out for each
  pixel, its edges soft over one pixel, and is `[pick] ring_width`
  wide whatever the size of what it is about. It falls only on the
  ground: the heights of the land and the yards about it are in its
  buffer, and a wall, a robot or a barrel standing higher is left
  alone. A click selects its agent,
  hamlet, the Hearth or the library in the window's focus and brings
  the Inspector forward; a click on the bare ground selects nothing.
  A hamlet, the Hearth or the library selected elsewhere is brought
  into view. Houses, commons, the Hearth and the library have signs
  while the camera is near, and a robot or the mascot a tag under the
  pointer.
- **Moving a town.** It hears the Inspector's `carry`: the town is in
  hand, its footprint follows the pointer over the ground, gold where
  it fits and red where it does not, a click sends `/move-hamlet` to
  its hamlet as a decree, and the right button puts it back.

The panels of `hamlet.agents` each follow the focus the agents
service keeps, so what one opens or selects the others show. Its
parts share `shared/`: the service as they read it (its `plugin`,
which hears
each reply and event in `PreUpdate`, keeps the focus, the roster, the
needs and the library's skills as resources and writes what it heard
as a `Heard` message, a request that failed told in the log once;
`start`, a system for `Declare`; and `ask`),
a message, a change, the card of an agent's questions, Markdown as
labels can set it (headings, paragraphs, items, quotes, code and
rules; a link is its words, and each `https` link is a button under
the text that opens it and leads to its address), a confirmation
on an overlay, the
verb beside CANCEL with a warning for agents it cuts short,
`breadcrumb`, the shell's breadcrumb made of the SDK's nodes: a quiet
button for each page above, a dim `/` after each, and the page shown
in the display face and the tone of a head, and `link_row` and
`link_rows`, the rows of a list as the Plugins list's: a row button
with a chevron at its end, a divider between each and the next.

- **The chat** (`chat/`) is the panel Chat, apart from the Inspector. Its
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
- **The Inspector** (`inspector/`) shows what is selected. A page under another is headed by a breadcrumb
  of the pages above it, each pressed to go back to it, fixed over
  what scrolls; names are as stored and a tab is not part of it:
  HAMLETS / the hamlet, HAMLETS / the hamlet / the agent, the same
  with PROMPT after the agent on its prompt, HAMLETS / THE LIBRARY,
  and ARCHIVE / the session in the reader of the ARCHIVE tab. Its
  lists of hamlets, of a hamlet's agents, of past sessions and of the
  hamlets closed down are `link_rows`. An agent: its
  state, STOP while it works, More (`more.rs`: a
  menu naming agents as they are named, of talking to it, its prompt,
  stopping it, Build a road to each agent of its hamlet it has no
  road to yet, Give a skill... with each skill another has or a
  starter one, Merge X into Y... each way with every other agent of
  its hamlet, Remove the road to each agent it is joined to
  (`/unroad`), undoing the last change, making it mayor and removing
  it, each a decree to its hamlet, asked about first where `main`
  asks; a mayor's has only talking, its prompt, stopping and
  undoing), its purpose, its
  model and use, its spend, the Model dropdown (`RunModel`), and NOTES
  (REFLECT NOW), CHARACTER (EDIT), ARCHIVE and PROMPT. The archive
  lists the past sessions with a search and counts, each read on a
  click, and read, resumed (bringing the chat to the front) or
  deleted from its menu, and reads one
  forty messages at a time. One that is gone, under its hamlet, with
  RESTORE and DELETE FOR GOOD. The library, with OPEN CHAT while it is empty,
  which brings the chat to the front. Every hamlet, FOUND A HAMLET
  over the search as ADD PLUGIN... is over the Plugins list's, each
  with Close Down in its menu, those closed down, and
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
  task's cost and messages, a road's price, the models it may run,
  each a switch beside its card line (its name, its prices, what it
  holds and its note), and its default, what its mayor may do
  without asking, and its
  most agents and roads, each written with `SetBylaw`) and RECORD.
  The realm's page and the bylaws share the controls of a limit,
  `shared/`'s `whole` and `setting`, and set a yes or a no with a
  `switch` named beside it.
- **The Skills panel** (`skills.rs`) is the agent's own skills
  over the library's, each
  with TAKE OUT, and OPEN CHAT, which brings the chat to the front,
  while the library is empty.
- **The agents' petitions** (`petitions.rs`) are the AGENTS section of
  the engine's Petitions panel: the reason field, STOP ALL, and what the agents wait on the
  chief for, oldest first, with the answers each takes; a question's
  card is the one the chat shows. It tells the engine how many wait,
  so it shows while any do, and brings the panel to the front when
  more wait. The plugins' petitions under it stay the engine's own.

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
- **No caps.** A plugin makes, holds, writes and reads as many assets,
  of any size, as it likes.
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
  call itself, the person allowed it (or is asked first), a running plugin exports it (else no
  implementer), and both sides name the same types (else a mismatch).
  The bus then hands the plugin a `Serve` and remembers the call
  (`Calls`).
- **Answered.** The result of `serve`, or why it failed, comes back as
  `Answered` or a failure; a plugin past the call time the person set
  leaves the caller out of budget and is stopped, as any plugin at
  fault is. A call whose plugin stops before it serves it fails.
- **Asynchronous.** Each plugin has a store of its own, and a host
  call cannot reach another store while its own is in use, so a call
  is not made within the caller's call: it goes through the callee's
  inbox, served on the next frame, and its answer through the caller's
  on the frame after. The depth a request is made at follows the chain
  of messages, and saturates rather than overflows.

## Events

- **Declared.** A plugin sends events its manifest names with the type
  of their payload, and hears another's by its full id, with the type
  it expects.
- **Subscribed** by name at run time (`subscribe`), to an event its
  manifest hears; `Subscriptions` on its entity, let go when it is
  switched off.
- **Sent** with `emit`, as many as it likes; a refusal is replied to,
  a sent event is not. Events need no grant: a plugin hears only what it
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
- **No caps.** A plugin makes as many fetches as it likes, at once,
  of answers of any size, for as long as each takes.
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
  A plugin switched off is told nothing more until it watches again.
- **Settings.** `Settings` gives the model, `reflect_every`, `allow_net`
  and the endpoints by name and address, each with only whether it has
  a key or is the ChatGPT plan, with the link and the listing. Each of
  `SetModel`, `SetReflectEvery`, `SetAllowNet` and `RemoveEndpoint`
  saves the settings; removing an endpoint forgets its key and signs
  out of ChatGPT. `OpenUsage` opens the plan's usage page.
- **Models.** `TestConnection`, `ListModels`, `Catalogue`,
  `AllowModel`, `PriceModel` (only a model with no price, or the
  person's own) and `TryModel`.
- **Setting up a place for models**, as the first-run card does:
  `Providers`, `BeginSetup` (adding or not), `ShowKeyCard`,
  `FindLocal`, `PickModel` and `EndSetup`, followed in `Setup`:
  whether one is under way (`begun`, from `BeginSetup` until it ends),
  its stage, who answered and with which models, and the problem in
  plain words. The endpoint being set up, and a key a sign-in gave,
  stay in the service.
- **The key card** (`keys.rs`) is the engine's own overlay
  (`engine/keys`), where the person types a key or signs in, so no
  request carries a key. `ShowKeyCard` shows it while a setup is under
  way, for a key (USE A KEY: WHO GIVES YOU YOUR MODEL, a provider or
  OTHER with WHERE YOUR MODEL RUNS, YOUR KEY and CONNECT) or a sign-in
  to OpenRouter or ChatGPT (SIGN IN, begun as it is shown). BACK hides
  it. It hides once a place answers, for the plugin's card to pick a
  model, and a sign-in to OpenRouter that did not finish turns it to
  the key of OpenRouter.
- **The charter**: `Charter`, `EditCharter` (the home's own, opened in
  the system's editor) and `ResetCharter`.
- **The realm's limits**: `Limits`, and `SetLimit` of one key of
  `votes`, `hamlets` or `road.grows_over_days`, failing with the
  problem when the file cannot take it. Neither it nor `SetBylaw`
  writes the lasting permissions, which only the chief gives: a key
  under `permissions`, a value of more than one line, or a table where
  a value goes is refused.
- **Lasting permissions**: `Grants` and `Revoke`.
- **Status**: the sandbox, the model, the link, the open session's use,
  the first hamlet's spend, how many things wait for the chief, and
  whether the open session works.
- **The chief**: `Send` a line, a message or a slash command, to an
  agent or a hamlet's mayor, and `Veto`. `Send`, `Open` and `Decree`
  take only a living agent or a hamlet open or closed down, as each
  name has a session of its own; another is not found.
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
  `Focus` reads it and each change is an `AgentsEvent::Focus`. The
  selection is kept in the home's `selection.toml` and selected again
  at the next launch if it is still there; the library is not kept.
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
  left, the step it is on with its count of steps and failed tool
  calls since it took up its work, and whether the last turn of its
  conversations ended in an error (`fault`); the spot a hamlet's
  chief set it down on, its place in the order the realm's record
  founded the hamlets, and its budget as numbers (the share left,
  the rest in words, the period and the share of it gone by); and
  the roads between agents, each end by its hamlet and number, with
  their use, the messages they carried and the latest sender; it is
  told again as `AgentsEvent::Roster` when it changes. `Record` is a hamlet's budget, votes, comings and goings
  and limits file. `Bylaws` is what a hamlet's own limits file holds
  for it alone, and `SetBylaw` writes one key of that file, refusing
  a key of the realm's and failing with the problem when the file
  cannot take it. `RunModel` has an agent run a model from now on,
  written as the app does: the mayor's pinned in its hamlet's limits,
  another's chosen as its mayor would, its hamlet let run it.
- **What befalls agents**, each agent by its hamlet and number:
  `Carried` (a message went along a road, told after the roster
  that has the road), `Consulted` (an agent called on the
  librarian), `Voted` (an agent cast a ballot), and `Saying` (what
  an agent says or thinks as its turn goes, an agent it delegated
  to included, told as it changes).
- **What waits for the chief** (`needs.rs`): `Needs`, told again as
  `AgentsEvent::Needs` when it changes and every ten seconds for the
  time waited. `Answer` gives a choice with a reason or a reply. The
  answers to an agent's questions are picked in the service (`Pick`,
  `Turn`, `Submit`), so a card shown in two panels shows the same
  sheet. Each answer, whoever gave it, is told as `Answered` with
  its choice, and each veto as `Vetoed`.
- **Gifts** (`gifts.rs`): `Offers` is the skills a living agent
  could be given from another agent of its hamlet; `Give` gives one
  as the chief's `/give` does, failing unless it arrived. `Hand` puts a copy of a file or folder the person dropped
  on the plugin's views (the number of its `SceneDrop`, kept in the
  grants' `DroppedFiles`) in an agent's inbox, with the app's checks
  (32 MB at most), and tells the agent in the chief's conversation
  with it; a plugin names no path, and nothing of the home is
  handed.
- **The chief's records** (`chief.rs`), in the app's files under
  the home: `Chief` is the chief's name, look (`chiefs/<user>.toml`),
  the spot its Hearth was set down on (`chiefs/<user>.place`) and
  the library's (`library.place`); `SetLook`, `SetHearth` and
  `SetLibrary` keep them, each told as `AgentsEvent::Chief`. Only
  this window's chief is known: the core has no chiefs of its own
  to arrive or leave.
- **Capabilities**, each on the target `local`: `agents.read` for what
  is read and watched, and the focus, `agents.settings` for what
  changes the settings, the catalogue's models, the setup, the
  charter, the realm's limits and a hamlet's bylaws, and
  `agents.chief` for sending, vetoing, revoking, rewinding, decrees,
  the model an agent runs (`RunModel`), the character, deleting a past
  session, answering what waits, gifts and the chief's records. A use
  answered Ask next time asks the person first, as the other services
  do.
- **Secrets stay in the service.** No request carries a key and no
  reply holds one, a ChatGPT token or a sign-in. A manifest asking for
  `agents.keys`, or any capability with `keys` among its words, is
  refused as it is read. Keys are kept in the system's keychain or a
  private file of the home, as the app keeps them, but under the
  keychain service `hamlet_engine` (`KEYCHAIN_SERVICE` in
  `engine/core`), so the engine never reads or deletes the app's keys.

## The desktop

`hamlet_engine_desktop` is the machine the engine runs on.

- **The protocol** is `hamlet_engine_api::desktop`: a plugin asks with
  `Request::Desktop(DesktopRequest)`, `desktop` in the WIT, and is
  answered with `Reply::Desktop(DesktopReply)`. `Watch` sends each
  `DesktopEvent` from then on with the ticket of the `Watch`: the
  settings changed, or the address under the pointer. A plugin
  switched off is told nothing more until it watches again, and the
  address it told of goes.
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
  `material` (`standard-material`: base colour, emissive,
  perceptual roughness, metallic, reflectance, alpha mode and cutoff,
  unlit, both faces, fog enabled; a mesh's
  vertex colours
  tint it; or a decal's), `entity` (under a parent or the scene's
  root, with a key of the plugin's: its transform, visibility,
  parent, mesh and material or decal, light, shadows, words over it
  and what it watches),
  `instances` (copies of one mesh in one material, ten floats a copy,
  copy `i` tagged `i` for a shader; Bevy draws them together) and
  `camera` (at an entity, along its forward). Dropping a handle takes
  its thing away; dropping an entity takes away what is under it. A
  mesh, material, shader, buffer or texture still drawn or run with
  stays, counted against the GPU memory the person may set, until
  nothing uses it.
- **Lights** are directional (with shadows), point and spot, each a
  light of an entity; ambient light is a camera's. The side of each
  cascade of a directional light's shadow map is `shadow-map`, a power
  of two up to the GPU's widest
  texture. Bevy's map is one for the whole engine, so it is drawn at
  the largest side any scene asks for, 2048 when none does
  (`effects.rs`).
- **Fog volumes** (`set-fog-volume`) are Bevy's: a box of haze one unit
  each way about the entity, shown by a camera with volumetric fog
  where a directional light with shadows and `set-volumetric` lights
  it, as shafts. Bevy shows every fog volume to every camera with
  volumetric fog, whatever its layer, so one plugin's fog volumes and
  volumetric fog are drawn at a time: the first to ask holds them
  until it has neither, and another's are left out, the log saying
  so.
- **Decals** are Bevy's forward decals (`decal.rs`): `draw-decal`
  lays a material made by `material.decal` (one colour) or
  `gpu.decal-material` (its colour times a texture's) on what lies
  under and over a square one unit wide on the entity's x and z,
  placed, turned and sized by its transform. Its look is a standard
  material, always blended: base colour and alpha, emissive, unlit
  and fog enabled, and `depth-fade-factor`, how far from the square what it falls
  on may lie before it fades out. `set-decal` changes it and keeps
  its texture. Once a scene has a decal, each of its cameras has a
  depth prepass, which the decal reads. A decal's material draws no
  mesh, and a mesh's no decal; the call that tries fails. A decal is
  counted as its entity and its material. `draw-decal` also lays a
  decal of the plugin's own shader (`gpu.shaded-decal`, below).
- **A camera's settings**: a perspective or orthographic projection,
  what it clears to, high range, tonemapping (all of Bevy's nine),
  exposure, distance fog, bloom, ambient light, `msaa` (Bevy's `Msaa`:
  `off`, `sample2`, `sample4`, `sample8`, lowered to the most the GPU
  has for a camera's images where it asks more) and
  `shadow-filtering-method` (Bevy's `ShadowFilteringMethod`:
  `hardware2x2`, `gaussian`, `temporal`). Bevy needs a camera with
  screen-space ambient occlusion to draw without MSAA, so the engine
  draws it as `off` while it has the occlusion and as asked otherwise.
  Every name of the interface that has a counterpart in Bevy is
  Bevy's own, in kebab-case.
- **A camera's effects** (`set-effects`) are Bevy's own, each off when
  none: depth of field (Gaussian or bokeh), colour grading (exposure,
  temperature, tint, hue, saturation), screen-space ambient occlusion
  (which draws without MSAA, and adds the depth and normal prepasses)
  and SMAA, the scattering sky of an earth-like world
  (`atmosphere`, drawn in high range; the engine makes one sky for all
  scenes once a camera asks for it and keeps it, as Bevy's renderer
  fails if its medium goes mid-frame) and volumetric fog. Depth of
  field reads the camera's own depth, so a pass needs none. In the
  SDK these are components named and shaped as Bevy's
  (`scene/effects.rs`): `DepthOfField`, `ColorGrading`,
  `ScreenSpaceAmbientOcclusion`, `Smaa`, `AtmosphereSettings` and
  `VolumetricFog` on the camera's entity, `VolumetricLight` on a
  light's, `FogVolume` on an entity of its own, and the resource
  `DirectionalLightShadowMap`.
- **One scene a camera.** Each scene is on a render layer of its own
  and its cameras see that layer alone, so a camera shows its own
  scene and nothing else.
- **Only changes cross.** Each call is one change, numbered by the
  host and applied as the frame's calls end; nothing is sent again
  each frame. `animate` asks for `tick` each frame, for a plugin that
  moves things itself.
- **A viewport** is a frame of the plugin's panel that `show-in` gives
  a camera of target `viewport`: its widget shows the image the camera
  draws into, sized to the room the layout gives it (the frames above
  it grow, `[render] grow_levels` at most), and the camera draws only
  while one shows it with room: a viewport in a tab behind another is
  kept, laid out with none, and told so as `resized`, its image kept
  at its last size. The camera draws before the interface, so an image
  made anew or resized is drawn before it is shown. In the SDK,
  `scene::viewport` is that frame.
- **Behind the dock.** A camera of target `background` draws into the
  window under the interface. One plugin holds the window's background
  at a time (the shell's `Backdrop`); another's make is refused. While
  one does, the shell's root has no fill and lets the pointer through,
  and the interface is drawn over the scene without clearing it, so
  the scene shows where the dock has no panel. The plugin is told the
  window's size as `resized`, as a viewport's is told its own.
- **The pointer.** Over a view, its moves (with the buttons held),
  buttons and wheel are told to the plugin with the key of the
  camera's entity, in the interface's pixels, each with the keys held
  (`modifiers`: shift, control, alt, meta); behind the dock, only
  where no UI node is under it, and over a viewport only where no
  card, menu or popup covers it. A press begun over a view is let go
  anywhere. A ray from the camera through the pointer finds the
  nearest watched entity of the plugin's (Bevy's mesh ray casting):
  `over` and `out` as it changes, `click` on a press of any button
  let go within `[render] click_slop`, with the button and the keys
  held, each with the point hit and its normal, and the copy of
  instances.
- **Keys.** A key pressed or let go (`key`, by its `key-code`) is
  told to the plugin of the view under the pointer, or else of the
  view pressed last, until a press elsewhere. The shell keeps its
  own (`ShellKeys`): a key typed in a field, one bound to a command,
  and any pressed while a menu, a popup or an overlay is open or a
  shortcut is being set, so Escape closes what is open first.
- **Files dropped** on the window (`dropped`) go to the plugin of
  the view the pointer is seen over within `[render] drop_wait`
  seconds, the system not moving the pointer while it drags; one
  that falls on no view is let lie. Only a plugin allowed
  `desktop.drop` on `files` is told, with the path and a number;
  answered Ask next time, the drop waits for the person. The number
  is kept in the grants' `DroppedFiles`, so a service hands over the
  file the person dropped by it, never a path a plugin names.
- **Words over the world** (`set-label`) are nodes of the interface
  centred `gap` pixels over the entity where the camera sees it,
  hidden past `far`: over each viewport, or behind the dock on a
  layer of their own under every root of the interface (`label.rs`),
  so the dock's panels cover them. A label is `plain` (small words),
  a `sign` or a name `tag`, each a box of the theme's `[world]`.
- **Balloons** (`set-balloon`) of speech or thought are kept over an
  entity as words are (`balloon.rs`): the surface in the frame of
  what floats with a point for speech, or a dimmer one with trailing
  rounds for a thought, dots that swell in turn while it has no
  words. One pops in over `[world] balloon_arrive` and shrinks away
  over `balloon_leave` once taken off.
- **Words beside the pointer** (`set-pointer-label`) follow it, in
  the plugin's colours and size, while it is over one of the
  plugin's views.
- **Menus and cards.** `ui.open-menu` opens a menu at the pointer,
  the shell's own context menu with its groups and critical items;
  the item chosen is a `chose` of its key, a menu closed otherwise a
  `closed`. `ui.make-card` gives a frame the plugin builds a card
  under; `place-card` shows it beside the pointer (out of its way,
  fading in, the pointer passing through), `here` (where the pointer
  is, over everything, as a menu opens; a press elsewhere or Escape
  closes it, a `closed`), or hides it, and `entity.hang-card` hangs it
  `gap` pixels over an entity in the first view of the scene shown,
  cut where the view ends (`float.rs` in the shell). A card is
  `plain`, a `card`, a `tag` or a `menu`, at most `widest` pixels.

### Layer two: the plugin's own GPU work

- **WGSL, checked first.** `shader.compile` takes plain WGSL of one
  kind, without `#`, of any size. The engine's
  bindings come before it and its entry points after it, so the source
  declares no bindings, no entry point but a compute shader's `main`
  and no `override`; naga parses and validates the whole, and a
  refusal names the line of the source. A material gives
  `fn shade(surface: Surface) -> vec4<f32>`, or `-> Shading` (colour,
  emissive, metallic, roughness and reflectance, for a lit material's
  sheen), and may give
  `fn displace(vertex: Vertex) -> vec3<f32>`; a decal
  `fn shade(decal: Decal) -> vec4<f32>`; a pass
  `fn post(drawn: Pass) -> vec4<f32>` (`drawn`, as WGSL reserves
  `pass`); compute `@compute @workgroup_size(..) fn main`, of at most
  the device's invocations a workgroup and its size along each axis,
  and of at most its bytes of `var<workgroup>` memory. The fixed
  names: `params` (16 `vec4<f32>`, the plugin's 64 floats), and
  `picture`, `picture_sampler` and `data` (a read-only storage
  buffer) of a material and a decal; `screen`, `screen_sampler` and `frame` (size
  and time) of a pass; `data` and `extra` (read-write storage
  buffers) and `frame` (time, delta and a count) of compute.
- **Bounded.** The shader Bevy is given has wgpu's runtime checks on
  (`ValidateShader::Enabled`): every index is bounds-checked and every
  loop is bounded by naga's force-loop-bounding. Where the backend has
  no loop bounding (not Vulkan, Metal or DX12), a source with a loop
  is refused.
- **Materials** (`gpu.shaded-material`) are Bevy's `Material` with the
  plugin's shader in place of its own, lit by the scene's lights
  through Bevy's PBR functions or shown as `shade` gives, fogged and
  tonemapped as Bevy's own; `set-params` changes their params. They
  are in the prepasses and cast shadows as a standard material does
  (an entity's `set-shadows` turns it off): the plugin's shader is
  wrapped for Bevy's prepass too, running the same `displace` with the
  prepass's time and the mesh's normals, so a shadow moves with what
  casts it, and cutting a masked material under half its alpha, and a
  blended one under 0.05, as `shade` gives it.
- **Compute** (`gpu.compute`) runs over the plugin's buffers
  (`gpu.buffer`, of 16-byte items; a write uploads the whole buffer as
  the plugin last wrote it), once or every frame, before the cameras
  draw. A dispatch is at most the device's workgroups along each
  axis, as one past them would lose the device; a plugin dispatches
  as many as it likes.
- **The device's sizes.** A buffer, and a mesh's vertices and
  triangles, are at most the device's largest buffer (the smaller of
  its `max_buffer_size` and `max_storage_buffer_binding_size`); a
  texture is at most its widest 2D texture on each side. A make past
  one is refused.
- **Shader decals** (`gpu.shaded-decal`, laid with `draw-decal`) are
  drawn through a box one unit each way about the entity, placed,
  turned and sized by its transform, only its far sides, whatever
  lies in front (`decal.rs`). For each
  pixel the depth prepass gives what is seen there, and `shade` is
  given it as a `Decal`: `position`, that point in the world;
  `local`, the same in the box, -0.5 to 0.5 each way; `uv`, its
  place on the box's square, `local.xz + 0.5`; and `time`. A pixel
  of what lies outside the box is left out, after `shade`, so its
  derivatives (`fwidth`) hold. `shade` gives a colour and its alpha,
  blended by the alpha mode, which is one that blends (`add` adds the
  colour times its alpha). It casts no shadow and has no prepass of
  its own; `set-params` changes its params. It is counted as a
  shaded material, and as a decal.
- **Passes** (`gpu.set-pass`) run over the image of one of the
  plugin's cameras in Bevy's 3D schedule, after it draws and before
  its tonemapping.
- **Textures** (`gpu.texture`) are sRGB images a material or a
  decal samples.
- **GPU time** is timed only for a plugin the person gave a GPU time
  a second (`Metered`, in `guard.rs`); any other's compute and passes
  run untimed, with no clock read back. Where the device has
  timestamp queries, each compute and pass of a metered plugin is
  timed (`[render] timed_slots` at once, shared by them; one that
  finds every slot taken does not run until a slot is let go, then
  keeps its slot while it lives and runs every frame, untimed only
  while the clock is not yet made on the device), read back each
  frame, and its milliseconds summed over a second; past its GPU time
  its compute and passes are held back as a whole until its scene is
  made again, as it is switched off and on, reloaded or stopped, and
  the log says so. Let run again each second, a plugin near its time
  would have them go off and on, and its view flicker. A material is drawn inside Bevy's
  own passes, among every other mesh, so it is not timed apart.
- **A failing device.** Bevy's `RenderErrorHandler` is the engine's:
  when the device is lost or wgpu reports an error, the plugins with
  GPU work of their own are stopped (`GpuLost`) and the renderer is
  built again (`RenderErrorPolicy::Recover`). With no such work it is
  built again all the same. One that fails again within ten seconds
  of being built stops the plugins with any scene; with none, nothing
  is drawn for a second, then twice as long each time it fails again,
  a minute at most, before it is built again, so a failure of the
  engine's own does not flash each frame. The engine is not quit.

### Capabilities and limits

- `render.scene` makes meshes, materials (a decal's too), entities,
  instances and cameras; `render.shader` compiles material, decal and pass shaders, makes
  shaded materials and decals and sets passes; `render.compute` compiles compute
  and dispatches it. A buffer or texture needs either of the last two.
  Each is asked for on the target `gpu`. A make without its capability
  fails saying so; one answered Ask next time fails saying it waits,
  and the person is asked once.
- **As much as it likes.** A plugin holds as many meshes, materials,
  lights, cameras, shaders, buffers and textures as it makes, and
  changes its scenes as often as it likes. Only when the person set
  them are its scene entities (each copy counted) and its GPU memory
  (meshes, buffers and textures, as the engine counts their bytes)
  held; a make past one fails with why (see [Limits](#limits)).
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

## Sound

`hamlet_engine_audio` plays sound on Bevy's audio, for the shell and
for plugins. With no audio output, as in the harness, sounds are kept
and come to their end all the same, and nothing is heard.

- **A clip** (`Clip`) is samples handed over as they are to be heard:
  floats from -1 to 1, interleaved, one channel or two with the left
  first, at a rate of 8,000 to 96,000 frames a second. A sample past
  -1 or 1 is held to it; one that is not finite, a count that is not
  whole frames, another number of channels or another rate is
  refused. No file is decoded.
- **A clip filled** is opened empty and has samples appended until
  it is closed. A sound may play it while it is filled: it plays
  from a copy of its own, sent each part appended over a channel to
  the output thread, and past the samples it has it waits in
  silence. It loops only once closed. A clip opened `once` is played
  by one sound once, not looped: its samples go to that sound, the
  output thread lets go of each part once played, and the plugin's
  audio memory counts them only until they are heard, by the time
  played. Its sound ends when the clip is closed and heard to its
  end.
- **A sound** is a clip playing, an entity with a `Sounding` and
  Bevy's player: unplaced, heard the same wherever the listener is,
  or placed at a point or at an entity, following it. Its volume (0
  to 1) and speed (1/4 to 4, its pitch moving with it) can change as
  it plays, and its volume can fade evenly to another over seconds,
  moved each frame; it plays once or loops, and stops when it is
  stopped or taken away. One that does not loop is taken away at its
  end, by the time it played at its speeds.
- **Heard from a listener.** A plugin names the entity of its scene
  its placed sounds are heard from; without one, its scene's origin.
  Bevy hears every sound from one listener, so each placed sound is
  moved, each frame before Bevy reads it, to where it is from its
  owner's listener: two plugins with cameras in different places
  each hear their own as `main`'s `Ears` do. Within `[sound] reach`
  of the listener a sound is at its full volume, and beyond it falls
  with the square of the distance; the ears are `[sound] ears`
  apart.
- **Categories** (`Category`): interface, world, music and voices. A
  sound's volume is its own, times the engine's volume of its
  category and the master's (`[sound]`), times the person's.
- **The person's volumes** (`SoundSettings`): a switch and a level
  for the master and for each category, and the plugins muted, kept
  in `sound.toml` in the home, written whole once they stayed still
  for `[sound] save_after` seconds, and as the engine closes. A file
  that cannot be read is left as it is, the log says so, and the
  volumes are kept only until the engine closes.
- **The Sound page** of Settings (`engine/shell/src/volumes.rs`, the
  speaker icon) shows Master, then Interface, World, Music and
  Voices, each its name over a switch and a slider, dim while it is
  off.
- **Muting a plugin.** SOUND on each plugin's page in the Plugins
  panel has a switch, "Mute its sounds", kept by the plugin's id in
  `sound.toml`; a muted plugin's entity carries `Muted`, and its
  sounds play silent. Uninstalling a plugin forgets it.
- **The capability** `audio.play`, on the target `output`, makes
  clips and plays them. A bundled plugin that asks for it has it;
  an installed one asks the person, and a make or play answered Ask
  next time fails saying it waits, and the person is asked once.
- **As many as it likes.** A plugin holds as many clips and plays as
  many sounds as it likes. Only when the person set its audio memory
  are the bytes of the samples of its clips held (four a sample; a
  clip let go of while a sound plays it stays counted until none
  does); a make past it fails, saying why, and an append past it
  fails as `full`, appending nothing, for the plugin to append the
  same samples later.
- **The interface** `audio` in the WIT, in the world `sounds`, which
  `render-plugin` includes: the resource `clip` (`make` with the
  samples, the channels and the rate; `open` with the channels, the
  rate and whether it is played `once`; `append` samples, failing as
  `full` or `refused` with why; `close`), the resource `sound`
  (`play`, `play-at` a point and `play-on` an entity of the plugin's
  scene, each with a `playback`: its category, volume, speed and
  whether it loops; `set` its volume and speed; `fade` its volume
  over seconds; `playing`, false once it ended or was stopped;
  `stop`; dropping it stops it) and `listen` at an entity of its
  scene or none. A number that is not finite fails the call. The
  host keeps each plugin's clips and sounds in a `Ledger` as it
  calls, and makes them in the world as the call ends
  (`engine/wasm/src/audio.rs`); each frame the ledger hears which
  sounds came to their end and the bytes heard of each clip played
  once, and a plugin numbering anew, after a reload, has its sounds
  stopped.

### The interface's sounds

The shell plays the interface's own sounds (`engine/shell/src/
sounds.rs`), of the category interface, each made in code at start
(`sounds/synth.rs`) from sines and filtered noise, in D major, as
`main` makes them. Each is written as a `Cue` and played no more than
`[interface_sounds] most` at once and not again within `gap`, its
pitch moved a little each time (to another note of the scale for one
with a note) and its volume by `sway`.

| Sound | When |
|---|---|
| Tick, one small high tick | The pointer comes onto a plain, primary, critical or quiet button, a row, or a segment of the workspace switcher |
| Tap, a soft knock | A button, a switch, a row, a tab, a menu's title or item, or a field goes down, or a key shows another workspace |
| Confirm, two bell notes | A primary button does its work |
| Deny, two dull notes going down | A critical button does its work |
| Pop open and pop close, air up and down | A menu, a context menu, a dropdown's list or another popup opens or closes, not one taking another's place |
| Tab, a hollow click | A dock tab is brought to the front by hand, not one a tab set down brings with it within `drag_quiet` |
| Grab and dock, wood | A dock tab is taken hold of, and set down |
| Fold, paper | A fold opens or shuts |
| Send, air and two tines | A message field sends what is typed |
| Sash, the window's frame and glass | The window is hidden to run in the background, or shown again |
| Error, two muted notes of a bar | An error reaches the log |

### Not yet

The realm's world sounds and its music (`main`'s `Cue`s of the
world, the ambience and the breaths of music) come once the realm is
a Bevy plugin: it plays its clips through `audio`, placed at its
entities and heard from its listener between the camera and the
ground.

## Where the limits and guards are

A limit the person may switch on for a plugin is a `LimitKind` and a
field of `PluginLimits`, its meaning beside it in
`engine/api/src/limits.rs`; the value each starts at is `[limits]` in
`engine/tuning/engine.toml`. What keeps every plugin from crashing or
freezing the engine is a field of `Guards`, `[guards]`, its meaning
beside it in `engine/core/src/budget.rs`. The host's set-up is
`[wasm]`, how scenes are shown `[render]`, how sounds are heard
`[sound]` and `[interface_sounds]`, and the harness's waiting
`[harness]`.
