# A tour of the code

Where things are and where to change them. For what Hamlet is and how to
run it, read [`README.md`](../README.md) first. For the rules of working
here, [`AGENTS.md`](../AGENTS.md).

## The idea in one paragraph

Agents are folders and append-only records on disk. The core runs their
loops and writes the records. Everything shown is rebuilt from the records:
first as entities, then as a small model of towns, houses and roads, then
as a drawn world with panels around it. Nothing on screen is the truth; the
files are.

## How a change travels

```mermaid
flowchart TD
    typed([You type a line])
    chat["hamlet_ui: chat"]
    session["hamlet_core: session"]
    rules["hamlet_rules: commands, chief"]
    agent["hamlet_core: agent, the loop"]
    tool([An agent calls a tool])
    records[("hamlet.jsonl, realm.jsonl,<br/>sessions/*.jsonl")]
    entities["Entities: Settlement, Citizen,<br/>Mayor, Busy, Left ..."]
    society["Society: towns, agents on plots, roads"]
    drawn["Ground, houses, roads,<br/>robots, light, sound"]
    panels["Panels: hamlet_ui"]

    typed --> chat
    chat -- UserMessage --> session
    session -- "a chief's command" --> rules
    session -- "anything else" --> agent
    tool --> agent
    rules --> records
    agent -- "model, tools,<br/>letters to others" --> records
    records -- "hamlet_core: agents<br/>watches the files" --> entities
    entities -- "hamlet_realm: hamlet<br/>the bridge" --> society
    society -- "hamlet_realm: generator" --> drawn
    entities --> panels
    records -- "the sessions' transcripts" --> panels
```

## The crates

All are under `crates/`, and each depends only on those above it in this
table. The binary and the examples are the root package.

| Crate | What it is |
|---|---|
| `hamlet_sandbox` | Per-platform process sandboxes |
| `hamlet_model` | The model client, its settings, the catalogue of models, the session log |
| `hamlet_home` | A home on disk: its layout, charter and prompt, the chief's limits, the starter skills |
| `hamlet_rules` | The hamlet's record and the rules over it. Plain files and functions, no Bevy |
| `hamlet_core` | The runtime as Bevy plugins: agents, sessions, tools and oversight. No rendering |
| `hamlet_desktop` | The machine the app runs on: kept awake while an agent works, its programs that show a file or a link, its folder dialog and the app's icon |
| `hamlet_assets` | The files under `assets/`, built into the app and loaded as Bevy assets |
| `hamlet_offscreen` | An app that draws into an image with no window and saves a still. The examples and the plugin harness draw with it |
| `hamlet_vfx` | The visual effects: what each is and how it plays |
| `hamlet_realm` | The 3D world, drawn from the core's entities |
| `hamlet_sdk` | What a plugin is built with: the plugin types, the theme, looks and icons, the dock's panels |
| `hamlet_ui` | The window: panels, menus, chat and pointer around the world |
| `hamlet_wasm` | The host of the installed WASM plugins, behind the binary's `wasm` feature. See [`wasm.md`](wasm.md) |
| `hamlet_openui` | Panels written in Hamlet's panel language: the components, the evaluation into a panel tree, state, actions and the data plugin. See [`panels.md`](panels.md) |
| `hamlet_guest` | What a WASM plugin is written with: the interface bindings, the `Plugin` trait, the tree builder and the helpers that keep its state and its storage |
| `hamlet_harness` | Runs a plugin headlessly against made-up state: clicks, typed text, grants, the tree of each panel as text and a picture of it. Behind its `wasm` feature. See [`authoring.md`](authoring.md) |

The plugins are under `plugins/`: `plugins/all` is `hamlet_plugins`, which
lists them, and each other folder is one plugin crate. See
[`sdk.md`](sdk.md). A plugin built for WASM is under `guests/`, in a
workspace of its own.

Inside a crate, a module with several subjects is a file and a folder of
the same name, and each file opens with a line saying what it holds. Read
`lib.rs` for the list.

`hamlet_realm` has one folder for each thing in the world: `generator/`
(land, towns, houses, roads and traffic from a seed), `keeper/` (the
robots), `chief/` (the wizard and its Hearth), `hand/` (what the chief does
by hand), `pick/` (pointing, selecting and the camera) and `sound/`.
`RealmPlugin` in its `lib.rs` is the one thing the app adds: a flat list of
the `plugin` of each module, scene first (settings, palette, play, sound,
debug, sky, stage, keeper, stock), then the generator, then what the chief does in it, then
`hamlet.rs`. An app that wants only the scene adds the first group itself,
as the style test does; the town generator adds it with `generator::plugin`
and an `Origin` for its seed.
`hamlet_ui` has `screens/` for the panels that draw from the core's state
alone, one file or folder each, and `widgets/` for what every panel is built
from: `feel/`, `tabs.rs` and `tuning.rs`. The other panels are files beside
them. How things look (`theme.rs`, `looks.rs`, `icons.rs`, `platform.rs`)
is in `hamlet_sdk`.

Names to keep apart:

- `hamlet_core/src/agent.rs` is the loop one agent runs. `agents.rs` is
  every agent as an entity.
- `hamlet_rules/src/hamlet.rs` is a hamlet's record on disk.
  `hamlet_realm/src/hamlet.rs` is the bridge that puts hamlets into the
  world.
- `chief.rs` is a chief's commands in `hamlet_rules`, the wizard in
  `hamlet_realm` and the chief's panel in `hamlet_ui`.
- `hand.rs` is the hand in `hamlet_realm`. In `hamlet_ui` it carries what
  the hand did to the core.

## Where do I change...

Paths are under `crates/`.

| To change | Go to |
|---|---|
| What an agent is told every turn | [`prompt.md`](prompt.md); the charter is `assets/charter.md` |
| The tools an agent has | `hamlet_core/src/tools/`; the hamlet's own in `hamlet_core/src/agent/hamlet_tools.rs` |
| The questions an agent asks the chief | `hamlet_rules/src/chief/asking.rs`; their card, in Needs You and the chat, in `hamlet_ui/src/asking.rs` |
| Selecting and copying text in the panels | `hamlet_ui/src/select.rs`; [`chat.md`](chat.md) |
| A chief's slash command | The table `ALL` in `hamlet_rules/src/commands.rs`, which names who carries it out |
| What needs the chief's approval | `hamlet_core/src/oversight/danger.rs` |
| A plugin's petition: installing it, letting it use more, removing it | `hamlet_core/src/oversight/plugin.rs`, `hamlet_sdk/src/capability/petition.rs`, `install.rs`; removing it, `hamlet_wasm/src/uninstall.rs`; [`permissions.md`](permissions.md) |
| What an agent's commands may touch | `hamlet_sandbox/src/`; [`sandbox.md`](sandbox.md) |
| The folder a hamlet works on | `hamlet_rules/src/work.rs` keeps it; `Tools::work_on` in `hamlet_core/src/tools.rs` applies it |
| The workbench and its clockwork on a common | `hamlet_realm/src/works.rs` |
| The library of shared skills | `hamlet_rules/src/library.rs` |
| Spending and limits | `hamlet_rules/src/budget/`, `hamlet_home/src/limits.rs` |
| Models, their prices and who may run them | `hamlet_model/src/catalogue.rs`, `hamlet_rules/src/models.rs` |
| The model client | `hamlet_model/src/provider/` |
| Signing in with ChatGPT to run models on the chief's plan | `hamlet_model/src/chatgpt.rs`; its client, on the Responses API, in `hamlet_model/src/provider/responses.rs` |
| Removing, merging, restoring an agent | `hamlet_rules/src/lifecycle.rs` |
| Founding, naming, moving, closing a hamlet | `hamlet_rules/src/realm.rs` |
| A starter skill | `hamlet_home/starters/<name>/SKILL.md` |
| A panel | Its file in `hamlet_ui/src/screens/`, or in `hamlet_ui/src/` |
| The right-click menu | `hamlet_ui/src/world_menu/items.rs` |
| A colour in the panels | `assets/theme.toml`, its fields in `hamlet_sdk/src/theme.rs` |
| How a control looks or feels | `hamlet_sdk/src/looks.rs`, `hamlet_ui/src/widgets/feel/`; [`ui-warmth.md`](ui-warmth.md) |
| An icon | `hamlet_sdk/src/icons.rs`; [`icons.md`](icons.md) |
| A plugin, or what a plugin may add | `plugins/<name>/`, `hamlet_sdk/src/plugin/`; [`sdk.md`](sdk.md) |
| Which plugins are built in | `plugins/all/Cargo.toml` and `src/lib.rs` |
| The Skills panel | `plugins/skills/src/` |
| What an installed plugin may call, and its limits | `hamlet_wasm/wit/hamlet-plugin.wit`, `hamlet_wasm/src/host.rs` and `host/`, `assets/wasm.toml`; [`wasm.md`](wasm.md) |
| Which installed plugins load, and what each is granted | `hamlet_wasm/src/install.rs`, `load.rs`, `hamlet_sdk/src/capability/`; [`permissions.md`](permissions.md), [`wasm.md`](wasm.md) |
| Reloading a plugin when its files change, its last good version, the compiled cache | `hamlet_wasm/src/reload.rs` and `reload/`, `cache.rs`, `hamlet_sdk/src/reload.rs`, `assets/wasm.toml`; [`wasm.md`](wasm.md) |
| What a plugin's panel is drawn as | `hamlet_sdk/src/tree/view/node.rs` (a node as a view), `tree/node.rs`, `tree/store.rs`; `hamlet_wasm/src/wire.rs` and `draw.rs` for a WASM plugin's; [`wasm.md`](wasm.md) |
| What a plugin's panel shows when it stopped | `hamlet_sdk/src/tree/view/stopped.rs`, `hamlet_wasm/src/stop.rs`; [`wasm.md`](wasm.md) |
| How a click or typing reaches a plugin | `hamlet_sdk/src/tree/event.rs`, `hamlet_wasm/src/event.rs`; [`wasm.md`](wasm.md) |
| A panel written in the panel language, its components, actions and limits | `hamlet_openui/src/components.rs`, `action.rs`, `evaluate.rs`, `assets/panel_lang.toml`; [`panels.md`](panels.md) |
| A plugin that is a panel and no code | `hamlet_openui/src/data.rs`, `hamlet_wasm/src/install.rs`, `load.rs`; `guests/greeter_ui/`; [`panels.md`](panels.md) |
| Writing a WASM plugin | `hamlet_guest/src/lib.rs` and `ui.rs`, `guests/hello/`, `guests/roster/`, `guests/notes/`; [`wasm.md`](wasm.md), [`authoring.md`](authoring.md) |
| A plugin's own storage, its caps and the state the host holds for it | `hamlet_wasm/src/storage.rs`, `host/keeping.rs`, `instance.rs`, `assets/wasm.toml`; for a guest `hamlet_guest/src/storage.rs` and `state.rs`; [`wasm.md`](wasm.md), [`authoring.md`](authoring.md) |
| A text area or markdown in a plugin's panel | `hamlet_sdk/src/tree/view/area.rs`, `markdown.rs` and `markdown/parse.rs`; [`wasm.md`](wasm.md) |
| The Notes plugin | `guests/notes/`; its panel in the gallery, `examples/ui_gallery/scenes/notes.rs` |
| Starting a plugin, its templates and the script that copies them | `templates/plugin/`, `templates/data-plugin/`, `scripts/new-plugin.sh`; [`authoring.md`](authoring.md) |
| Testing a plugin headlessly: the harness, its script and its pictures | `hamlet_harness/src/harness.rs`, `script.rs`, `run.rs`, `render.rs`, `examples/plugin_check/`; [`authoring.md`](authoring.md) |
| What an agent is taught about writing a plugin | `hamlet_home/starters/write-a-plugin/SKILL.md` |
| Building a plugin from an agent's sandbox | `scripts/vendor-plugin-deps.sh`; [`authoring.md`](authoring.md) |
| The Plugins page of Settings, and the permissions views | `hamlet_ui/src/screens/settings/plugins.rs` and `plugins/`; [`permissions.md`](permissions.md) |
| A colour in the world | `assets/palette.toml` |
| A size, speed or timing | `assets/world.toml`, `vfx.toml`, `ui.toml`, `wasm.toml`, `panel_lang.toml`. What each value means is beside its field in the code |
| The height something stands at | `LandData::surface`: the ground as it is drawn, never the smooth `height` |
| How a house looks | `hamlet_realm/src/generator/house.rs` |
| How a robot moves | `hamlet_realm/src/keeper/acts/`; [`robots.md`](robots.md) |
| An effect | `hamlet_vfx/src/`; when it is set off, `hamlet_realm/src/generator/magic.rs` |
| A sound or the music | `hamlet_realm/src/sound/`; [`sound.md`](sound.md) |
| The camera | `hamlet_realm/src/pick/camera.rs` |
| The wizard or the Hearth | `hamlet_realm/src/chief/`; [`chief.md`](chief.md) |
| What is saved between launches | `hamlet_ui/src/remember.rs`; settings in `hamlet_model/src/config.rs` |

## Tuning while the app runs

The files under `assets/` are built into the app. With the `hot-reload`
feature they are also watched: save a change and the running app takes it.

```sh
HAMLET_HOME=/tmp/hamlet-try cargo run --features hot-reload
```

Three things do not follow a saved change: the `[generator]` section of
`world.toml`, since the land keeps the settings it was built from; the
colour of a mesh that is already built; and `charter.md`, which a home's
own `charter.md` overrides instead.

A file that cannot be read is reported and the last good values stay.

## Seeing your work without a window

These draw to files and open nothing:

| Command | Shows |
|---|---|
| `cargo run --example ui_gallery -- --scene <name> --out <dir>` | One panel scene. `examples/ui_gallery/main.rs` lists the scenes. `plugin-roster` is an installed plugin's panel drawn from a tree, `plugin-notes` and `plugin-notes-preview` the Notes panel editing a note and showing it as markdown, `plugin-petition` the petitions of plugins, `plugin-permissions` and `plugin-capabilities` their Settings views, `plugin-reload` the Plugins page after reloads |
| `cargo run --example plugin_check --features wasm -- <plugin dir> [--script steps.toml] [--out <dir>]` | A plugin's panels as text after each step of a script, and a picture of each: see [`authoring.md`](authoring.md) |
| `cargo run --release --example far_lands -- --out <dir>` | The land, many towns, a town being moved; `--bench` times it |
| `cargo run --example chief_sheet -- --out <dir>` | The wizard, its looks, a Wickstep, the library |
| `cargo run --example cursor_sheet` | Every cursor |
| `cargo run --release --example sound_sheet` | Every sound and the music, as WAV files |

`style_test` (one fixed scene and the robots' motions) and `town_gen` (the
generated world, grown from the keyboard) open a window.

Run the app itself against a scratch home, never your own:

```sh
HAMLET_HOME=/tmp/hamlet-try cargo run
```
