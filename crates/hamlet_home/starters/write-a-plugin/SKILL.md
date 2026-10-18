---
name: write-a-plugin
description: Write, build, check and propose a Hamlet plugin: a new panel for the chief's dock. Use when asked for a panel, a view or a plugin for Hamlet.
usage: scripts/new-plugin.sh <data|wasm> <name> <folder>   (run in the Hamlet source folder)
---

A plugin is a panel in the chief's dock. It is a folder with a `manifest.toml`
and either a `panel.ui` (a **data** plugin: a small program, no build) or a
`plugin.wasm` (a **wasm** plugin: Rust built for `wasm32-wasip2`). Prefer a data
plugin when the panel only shows things and has a few buttons and fields.

Only the chief installs a plugin. You write it in your workspace, check it,
and tell the chief the folder. Never write into the chief's home, never touch
`plugins/grants.toml`, and never try to answer a petition yourself.

## Needs

The Hamlet source folder, with `templates/`, `scripts/` and `docs/authoring.md`
(read it first). Ask the chief where it is if you do not know. A wasm plugin
also needs cargo with the `wasm32-wasip2` target. Whether that builds from
your sandbox is in `docs/authoring.md`, under "Building from an agent's
sandbox".

## Steps

1. **Start** from a template, into a folder of your workspace. The name is
   lowercase letters, digits and `_`:
   ```sh
   scripts/new-plugin.sh data tide_table workspace/tide_table
   ```
2. **Write** it. In `manifest.toml` name only the capabilities the plugin
   needs, each with a reason the chief will read. Edit `panel.ui` or
   `src/lib.rs`, and the sample `steps.toml`.
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
   Fix every `FAILED step` and run it again. A step is `tick`, `click`, `type`,
   `toggle`, `set-agents`, `grant`, `deny`, `render` or `expect`.
5. **Propose** it: tell the chief the folder and what each ask is for. The
   chief copies it to `<home>/plugins/<name>/` and starts Hamlet, which asks
   before the plugin runs. After that, a running Hamlet reloads the plugin
   when its files change.

## Rules

- `interface` in a wasm plugin's manifest is the version this Hamlet speaks,
  and the template has it. A plugin built for another says so and does not load.
- A capability the manifest does not name cannot be used. `read.agents` is the
  one a plugin can use today; the others can be asked for and are checked
  later. `keys`, `petitions.answer`, `home.raw`, `process.run`, `sandbox.off`
  and `storage.other` are never granted, and a manifest that asks for one is
  turned down.
- A gated call that is not allowed returns an error. Show a line that says
  so; do not fail.
- Every button, checkbox and text field needs an id (`.id("go")`). Siblings
  may not share one.
- A call is held to a short time and a small memory (`docs/wasm.md`, Limits). A
  plugin that traps or is too slow is stopped. There is no storage yet: what
  the plugin knows is lost when it reloads.

## What goes wrong

- Printing or reading the clock traps the plugin. Log with `hamlet_guest::log`.
- A button with no id never reaches the plugin.
- A plugin built for the old interface: build it again from the template.
- A transcript can look right and the panel still look wrong. Use `render` and
  open the picture before you say it works.
