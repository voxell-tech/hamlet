---
name: write-a-plugin
description: Write, build, check and propose a Hamlet plugin: a panel for the chief's dock, with the menu lines, shortcuts, context menu items, inspector tabs, settings pages, tools for the agents and marks over the world it may add. Use when asked for a panel, a view or a plugin for Hamlet.
usage: scripts/new-plugin.sh <wasm|data> <name> [folder]   (run in the Hamlet source folder)
---

A plugin is a folder with a `manifest.toml` and either a `panel.ui` (a **data**
plugin: a program in Hamlet's panel language, no build) or a `plugin.wasm` (a
**wasm** plugin: Rust built for `wasm32-wasip2` with `hamlet_guest`). Prefer a
data plugin when the panel only shows things and has a few buttons and fields.
A wasm plugin may also add menu lines, shortcuts, items of the menu of a thing
in the world, inspector tabs, settings pages, tools for the agents, a canvas
and marks over the world, each behind a capability the chief allows.

A third kind, a **native** plugin, is a Rust library that runs as Hamlet with
full access and no limits. It is for what the sandbox cannot do; prefer a wasm
or data plugin whenever either will do. It is in `docs/native.md`, started with
`scripts/new-native-plugin.sh`.

Only the chief installs a plugin. You write it in your workspace, check it,
and tell the chief the folder. Never write into the chief's home, never touch
`plugins/grants.toml`, and never try to answer a petition yourself.

## Needs

The Hamlet source folder, with `templates/`, `scripts/` and `docs/authoring.md`
(read it first; `docs/wasm.md` and `docs/permissions.md` say what a plugin may
call and be allowed). Ask the chief where it is if you do not know. A wasm
plugin also needs cargo with the `wasm32-wasip2` target. Whether that builds
from your sandbox is in `docs/authoring.md`, under "Building from an agent's
sandbox".

## Steps

1. **Start** from a template, into a folder of your workspace. The name is
   lowercase letters, digits and `_`, starting with a letter:
   ```sh
   scripts/new-plugin.sh wasm tide_table workspace/tide_table
   ```
2. **Write** it. In `manifest.toml` name only the capabilities the plugin
   uses, each with a reason the chief will read. Edit `panel.ui` or
   `src/lib.rs`, and the sample `steps.toml`. `guests/` in the source folder
   has plugins to read: `notes` (state, storage, a menu line, a shortcut, a
   tab and a page), `pantry` (a database), `herald` (acting as the chief,
   files, the network), `surveyor` (the world), `clipper` (using another
   plugin) and `tides` (a tool for the agents).
3. **Build** a wasm plugin into a folder (a data plugin has nothing to build):
   ```sh
   workspace/tide_table/build.sh workspace/tide_table/build/tide_table
   ```
4. **Check** it headlessly, from the Hamlet source folder. It prints the tree of
   each panel after every step, what the plugin logged, and where the pictures
   are:
   ```sh
   cargo run --example plugin_check --features wasm -- \
     <the plugin folder> --script <steps.toml> --out <pictures folder>
   ```
   Fix every `FAILED step` and run it again. Several folders may be given,
   to check plugins that use each other in one home. The steps:
   - acting on a panel: `tick`, `click`, `type`, `toggle`;
   - acting as the chief: `run-menu`, `press`, `context`, `choose-file`,
     `grant`, `deny`, and `plugin` to put another plugin in front;
   - making up the world: `set-agents`, `set-papers`, `set-selected`,
     `set-world`, `turn`, `call-tool`, `restart`;
   - looking: `render`, `section`, `page`, `expect`, `expect-told`,
     `expect-overlay`, `expect-scene`.

   Each is in `docs/authoring.md`, under "The script". Where there is no
   GPU, as in your sandbox, add `--no-pictures`.
5. **Propose** it: tell the chief the folder and what each ask is for. The
   chief copies it to `<home>/plugins/<name>/` and starts Hamlet, which asks
   before the plugin runs. After that, a running Hamlet reloads the plugin
   when its files change.

## Rules

- `interface` in a wasm plugin's manifest is the version of the host
  interface this Hamlet speaks, `0.1.0`, and the template fills it in. A
  plugin built for another says so and does not load.
- A capability the manifest does not name cannot be used. They are
  `read.agents`, `read.chat`, `read.activity`, `read.papers`, `read.world`,
  `chat.send`, `commands.run`, `world.act`, `ui.menu`, `ui.keys`,
  `ui.context`, `ui.inspector`, `ui.settings`, `ui.canvas`, `world.overlay`,
  `render.scene`, `render.shader`, `net`, `fs.pick`, `fs.folder`,
  `files.read`, `files.write`, `db`,
  `model.call`, `plugin.use`, `plugin.export`, `collections.read`,
  `collections.write`, `agent.tool` and `agent.hook`. All but `read.chat`, `read.activity`,
  `fs.folder` and `model.call` are checked today; those four can be asked
  for and are checked later. `plugin.use`, `collections.read` and
  `collections.write` are chosen for each plugin or collection the manifest
  `imports`.
- A plugin that exports a typed interface (`[[exports.interfaces]]`) asks for
  `plugin.export`; one that calls it lists it under `[[imports.interfaces]]`
  with the same functions and types, and asks for `plugin.use`. The host
  checks every call's arguments and result against the types
  (`hamlet_guest::invoke`, `guests/harbour`, `docs/wasm.md`).
- `keys`, `petitions.answer`, `home.raw`, `process.run`, `sandbox.off` and
  `storage.other` are never granted, and a manifest that asks for one is
  turned down. So is one with `agent.tool` or `agent.hook` that also asks
  for `chat.send`, `commands.run`, `world.act` or `plugin.use`.
- A gated call that is not allowed returns an error. Show a line that says
  so; do not fail.
- Every button, checkbox, text field and text area needs an id (`.id("go")`).
  Siblings may not share one.
- A call is held to a short time and a small memory (`docs/wasm.md`, Limits). A
  plugin that traps or is too slow is stopped. A plugin has no variables that
  outlive a call: keep what the panel shows with `hamlet_guest::State`, which
  survives a reload, and what the user made with `hamlet_guest::storage` or
  `hamlet_guest::db`, which survive a restart (`docs/authoring.md`).

## What goes wrong

- Printing or reading the clock traps the plugin. Log with `hamlet_guest::log`.
- A button with no id never reaches the plugin.
- A plugin built for the old interface: build it again from the template.
- A menu line, shortcut, context item, tab or page whose capability the
  manifest does not name never shows, and the chief is not asked.
- Using another plugin's command, event or collection that the manifest
  does not list under `imports` fails.
- A transcript can look right and the panel still look wrong. Use `render` and
  open the picture before you say it works.
