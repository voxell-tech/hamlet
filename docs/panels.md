# Panels: the tree and Hamlet's panel language

A plugin's panel is described in one of two ways, and both end as the same
tree of nodes drawn with the looks of the native panels:

- a tree built in code, which a native or WASM plugin returns;
- a program in Hamlet's panel language (OpenUI Lang 1.0, through the vendored
  `openui-rs`), which a WASM plugin returns as text or a data plugin keeps in
  a file.

## Where it is

| Where | What it is |
|---|---|
| `hamlet_sdk::tree` | The tree model (`node`), `PluginTrees`, the read-model of what each panel shows (`store`), the view built from it (`view`), the `Interaction` event (`event`) and the ids of panels in the dock (`id`) |
| `crates/hamlet_openui` | The adapter: the library of components, the evaluation of a program into a tree, the state and actions of a panel, and the data plugin |
| `vendor/openui-rs` | The language's runtime, a git submodule like `vendor/fynix`. It is never edited here. `openui-lang` is its crate |
| `assets/panel_lang.toml` | The limits |
| `guests/greeter_ui` | A data plugin: `manifest.toml` and `panel.ui` |

In `hamlet_openui/src/`: `components.rs` (the list of components),
`library.rs`, `limits.rs`, `source.rs` (the text of a panel, parsed once),
`evaluate.rs` (a program into a tree), `convert.rs` (the language's elements
into nodes), `read.rs` (queries), `action.rs`, `interact.rs`, `draw.rs`,
`stale.rs`, `state.rs`, `wire.rs` and `data.rs`. `lib.rs` has the plugin.

## The tree

A node is an optional id, a kind and its children. The kinds are the widgets
of [`wasm.md`](wasm.md): column, row, scroll, label, heading, note, icon,
button, checkbox, text field and divider. The view is rebuilt only where
the tree differs: nodes are matched by id among their siblings, so a field being
typed in keeps its text and focus.

## The language

A program is one statement per line, `name = expression`, with the entry
`root`. `$name` is state, `Name(..)` a component, `@Name(..)` a built-in or an
action, `Query(..)` a read. The language is the upstream one, unchanged; see
`vendor/openui-rs/docs/compatibility.md` for what its runtime supports.
`openui-rs` is at milestone 1: forms, validation rules and editing are not in
it, and so not here.

### The components

Hamlet's own library, in `components.rs`. The root is `Column`. Props are
positional.

| Component | Props |
|---|---|
| `Column`, `Row`, `Scroll` | `children`: a list of components |
| `Label`, `Heading`, `Note` | `text` |
| `Icon` | `name`, a `Glyph` of `hamlet_sdk::icons`, in any case |
| `Button` | `label`, `action?` |
| `Checkbox` | `label`, `checked?` (bound to a `$variable`), `action?` |
| `TextField` | `text?` (bound to a `$variable`) |
| `Divider` | none |

`PanelLibrary::signatures` gives each signature as the language prints it,
for a prompt to be made from later.

### State

`$variables` and the values bound to them live in `State` on the panel's
entity. A `TextField($name)` writes to `$name` when its text is committed
(Enter, or leaving the field), and a `Checkbox("x", $on)` when it is toggled. The
panel is then evaluated again. A node that is not bound shows the value the
program gives, and a text typed in it is put back.

### Actions

A button or checkbox may carry an action: `Button("Go", @Set($n, 1))` or
`Action([@Set(..), @Reset(..)])`. `@Set` and `@Reset` change the panel's own
state and are run. `@Run`, `@ToAssistant`, `@OpenUrl` and custom actions are
not run: each becomes a line at the top of the panel and a log line. Forms
and validation are not implemented.

### Reads

`agents = Query("agents", {}, {rows: [], error: null})` is a read of the
agents. It is answered from `AgentList` and needs the grant `read.agents`
(`Granted` of the plugin that owns the panel). The result always has `rows`,
each with `name` and `hamlet`, and `error`: without the grant `rows` is empty
and `error` reads `read.agents not granted`. A query of any other name is
answered the same way with an error. The panel is drawn again when the agents or
the grants change.

### Problems

Parsing and evaluation never panic. An unknown component, a wrong type, a
limit that was passed or an action that was not run is a problem. The first is
shown as a dim line at the top of the panel, and all are logged with the id of
the plugin. What can be drawn is drawn.

The tree a program gives is a column of that line (if any) and a column of the
content, so a program's root keeps its place when a problem comes and goes.

### Limits

`assets/panel_lang.toml`, in `PanelLimits`: the nesting, nodes, steps and
string bytes of the language's runtime, and the nodes and levels of the tree.
What is over a limit is dropped and reported.

## A data plugin

A plugin with no code: a folder of the install folder with `manifest.toml` and
`panel.ui`.

```
<home>/plugins/
  approved.toml
  greeter_ui/
    manifest.toml     id, name, version, kind = "data", icon?, asks
    panel.ui
```

`kind` is `wasm` (the default, with `interface` and `plugin.wasm`) or `data`.
A data plugin is scanned, approved (`approved.toml`) and switched with
`Enable` and `Disable` as a WASM plugin is. It has one panel, `<id>/main`,
titled with its name; its grants are the approved ones that its manifest asks
for. The file is read when the plugin is brought up, and again on `Enable`; a
saved change to it is not followed while the app runs. Like the other
installed plugins it is loaded by the host behind `--features wasm`, though
it needs no engine.

## A WASM plugin that returns the language

`panel-view` returns a `view`: `tree(tree)` or `lang(string)`. In
`hamlet_guest`, `Plugin::panel_view` returns `ui::View`, made from a node
with `.into()` or from a program with `ui::lang(text)`. The host gives the
text to the panel, which draws it as above and answers its own events; the
plugin gets no `click`, `toggled` or `text-changed` from such a panel. If the
plugin later returns a tree, the panel goes back to the tree.

## The example

```sh
mkdir -p /tmp/hamlet-try/plugins/greeter_ui
cp guests/greeter_ui/* /tmp/hamlet-try/plugins/greeter_ui/
echo 'plugins = [{ id = "greeter_ui", grants = ["read.agents"] }]' \
  > /tmp/hamlet-try/plugins/approved.toml
HAMLET_HOME=/tmp/hamlet-try cargo run --features wasm
```

`greeter_ui` shows a heading, a text field bound to `$name`, a checkbox, a
greeting computed from both, a button that resets them and the agents from a
`Query`. Without a window:
`cargo run --example ui_gallery -- --scene plugin-openui --out <dir>`.
