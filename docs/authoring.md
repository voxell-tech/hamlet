# Writing a plugin

How to start a plugin, build it, check it without a window, install it in a
home, and edit it while Hamlet runs. What a plugin is made of is in
[`wasm.md`](wasm.md) and [`panels.md`](panels.md), and what it may be allowed
to do in [`permissions.md`](permissions.md).

A plugin is a folder with a `manifest.toml` and one of:

- `plugin.wasm`, a **wasm** plugin: Rust built for `wasm32-wasip2` with
  `hamlet_guest`.
- `panel.ui`, a **data** plugin: a program in Hamlet's panel language and no
  code, so nothing to build.

Where the panel only shows things and has a few buttons and fields, a data
plugin is enough.

A third kind, a **native** plugin, is a Rust library that runs as Hamlet with
no sandbox and no limits, for what the sandbox cannot do: add Bevy systems
and observers, reach the app itself. It is built with the compiler and the
compiled engine of the Hamlet that loads it, and the chief gives it full
access or none. Make one in the checkout's workspace with
`scripts/new-native-plugin.sh <name>`, or in a folder of its own with
`scripts/new-native-plugin.sh <name> <folder>`, built against an SDK kit that
`scripts/fetch-native-sdk.sh` downloads for a release, or that
`scripts/make-native-sdk.sh <kit>` makes from that Hamlet's build, and read
[`native.md`](native.md). Prefer a WASM or data plugin when either will do:
the chief answers one question for them, not "everything".

## What is here

| Where | What it is |
|---|---|
| `templates/plugin/` | A wasm plugin to copy: `Cargo.toml`, `manifest.toml`, `src/lib.rs`, `build.sh`, `steps.toml` |
| `templates/data-plugin/` | A data plugin to copy: `manifest.toml`, `panel.ui`, `steps.toml` |
| `scripts/new-plugin.sh` | Copies a template into a folder and fills in its placeholders |
| `templates/native-plugin/`, `scripts/new-native-plugin.sh`, `native/spark/` | A native plugin to copy into `native/` or a folder of its own, the script that does it, and a sample to read |
| `scripts/make-native-sdk.sh`, `scripts/fetch-native-sdk.sh` | Make the SDK kit a native plugin outside the checkout is built with from one build of Hamlet, or fetch the one a release published |
| `scripts/vendor-plugin-deps.sh` | Copies the crates a wasm plugin builds from into it, for a build with no network |
| `crates/hamlet_harness` | The harness that runs a plugin headlessly: a `Harness` to drive in Rust, and a script runner |
| `examples/plugin_check` | The harness as a command |
| `crates/hamlet_home/starters/write-a-plugin` | The starter skill that teaches an agent this page's steps |
| `guests/notes/` | A plugin to read after the template: `model.rs` (what it keeps), `storage.rs` (the notes on disk), `update.rs` (what an event does, and the commands it answers), its menu line, shortcut, settings page and inspector tab in `lib.rs`, `view.rs` (the panels), and its `steps.toml` |
| `guests/pantry/` | A plugin to read for the database: `lib.rs` keeps records in a table (`hamlet_guest::db::Table`), finds them by a field and draws what the table holds; its `steps.toml` restarts the app to see they are kept |
| `guests/herald/` | A plugin to read for what a plugin may do outside its panel: `lib.rs` sends a message and runs a command as the chief (`hamlet_guest::send_chat`, `run_as_chief`), keeps a draft in its folder of files (`hamlet_guest::files`) and fetches a page (`hamlet_guest::net`); its `manifest.toml` lists the hosts and commands in the `scope` of its asks, and its `steps.toml` answers each ask |
| `guests/surveyor/` | A plugin to read for the world: `lib.rs` reads the realm (`hamlet_guest::world::view`), marks the agents at work over it (`hamlet_guest::overlay`) and brings the camera to one (`hamlet_guest::world::focus`); its `steps.toml` makes up a world with `set-world` |
| `guests/clipper/` | A plugin that uses Notes: its `manifest.toml` (what it needs, imports and asks), `update.rs` (the call, the reply, the events and the command a context menu item runs) and `steps.toml`, a script for the two plugins together |

The template plugin has a panel, a button that counts in its state and one read
behind a capability (`read.agents`). It asks to use the capability with
`request`, shows a line for each answer (`granted`, `denied`, or still
`pending`), and redraws when the chief answers.

## Keeping state and data

A plugin's code has no variables that outlive a call, and needs none: the
`Plugin` trait has no `self`, and each call into the plugin starts from what the
host keeps for it. There are two places to keep something.

- **State**, `hamlet_guest::State<T>`: a value of a type of the plugin's own,
  held by the host in memory. It is kept across a reload and across a switch
  off and on, and dropped when Hamlet restarts. Use it for what the panel
  shows and the user chose, such as the selected row or a search text.
- **Storage**, `hamlet_guest::storage`: keys and values on disk in the plugin's
  folder, capped. It outlives Hamlet. Use it for what the user made, such as
  notes. `storage::load::<T>(key)` and `storage::save(key, &value)` keep a
  value of a type of the plugin's own; `get`, `set`, `delete` and `keys` keep
  bytes.

```rust
#[derive(Default, Serialize, Deserialize)]
struct Model { looks: u32 }

let mut model = State::<Model>::load();
model.looks += 1;
State::save(&model)?;
```

Both are written with `postcard`, a compact format with no field names, so a
type that changed shape cannot read what an older build wrote: `State::load`
then gives the default, and `storage::load` gives `None`. Keep a stored value
readable across versions by only adding fields at its end, or by giving it a
version. A plugin needs `serde` with its `derive` feature beside `hamlet_guest`.
Both are capped by `assets/wasm.toml` (see [`wasm.md`](wasm.md)); a save over a
cap returns an error, so show or log it. Do not hold either in a `static`,
`Cell`, `RefCell` or atomic.

## The loop

You need the Hamlet source folder, cargo, and for a wasm plugin the target:
`rustup target add wasm32-wasip2`. The commands below are from the source
folder unless they name another.

**1. Start.** `scripts/new-plugin.sh <wasm|data> <name> [folder]` copies the
template into `folder` (`<name>` in the current folder by default). The name is
the plugin's id: lowercase letters, digits and `_`, starting with a letter.

```sh
scripts/new-plugin.sh wasm tide_table /tmp/tide_table
```

```
/tmp/tide_table is ready: wasm plugin tide_table, interface 0.1.0
build:  /tmp/tide_table/build.sh /tmp/tide_table/build/tide_table
check:  cd <the source folder> && cargo run --example plugin_check \
          --features wasm -- /tmp/tide_table/build/tide_table --script /tmp/tide_table/steps.toml
```

The placeholders it fills in:

| Placeholder | Becomes |
|---|---|
| `{{name}}` | The id, also the crate's name |
| `{{title}}` | The name in words: `Tide Table` |
| `{{TITLE}}` | The same in capitals, as a panel's title |
| `{{interface}}` | The interface version this Hamlet speaks, read from `crates/hamlet_wasm/src/interface.rs` |
| `{{hamlet}}` | The source folder, or `$HAMLET_SRC` if that is set: where `crates/hamlet_guest` and `crates/hamlet_wasm/wit` are |

**2. Build.** `build.sh <folder>` builds with cargo and puts `manifest.toml` and
`plugin.wasm` in `folder`, a plugin folder. With no argument it builds into
`$HAMLET_HOME/plugins/<name>`. A data plugin has no build: its folder is
already a plugin folder.

```sh
/tmp/tide_table/build.sh /tmp/tide_table/build/tide_table
```

**3. Check, with pictures.** `plugin_check` installs the folder in a scratch
home, puts the plugin through a script and prints a transcript. It needs the
`wasm` feature (on by default), and the first run builds Hamlet's crates.

```sh
cargo run --example plugin_check --features wasm -- \
  /tmp/tide_table/build/tide_table \
  --script /tmp/tide_table/steps.toml --out /tmp/tide_table/pictures
```

```
plugin tide_table
  panel main
    column
      heading "TIDE TABLE"
      row
        button "Look" #look
      note "Looked 0 times." #looks
      note "Waiting for the chief to answer." #count
step 1: tick 1
step 2: expect main shows "Waiting for the chief"
step 3: render main to asked.png
wrote /tmp/tide_table/pictures/asked.png
step 4: set-agents (2 agents)
step 5: grant read.agents
  log info: allowed to use read.agents
  panel main
    column
      heading "TIDE TABLE"
      row
        button "Look" #look
      note "Looked 0 times." #looks
      note "2 agents live here." #count
step 6: expect main shows "2 agents live here"
step 7: click main/look
  panel main
    column
      heading "TIDE TABLE"
      row
        button "Look" #look
      note "Looked 1 times." #looks
      note "2 agents live here." #count
step 8: expect main shows "Looked 1 times"
step 9: render main to allowed.png
wrote /tmp/tide_table/pictures/allowed.png
```

The transcript shows each panel's tree, one line for each node, when it
changes, and what the plugin logged. A step that fails ends the run with
`FAILED step N: why` and exit code 1; the steps after it are not run. Open the
pictures: a tree can read right and the panel still look wrong.

**4. Install in a home.** The first install is the chief's. Put the plugin in a
scratch home and run Hamlet against it, never your own home:

```sh
HAMLET_HOME=/tmp/hamlet-try /tmp/tide_table/build.sh
HAMLET_HOME=/tmp/hamlet-try cargo run
```

Hamlet finds the folder at start and asks, in Needs You, "Install Tide Table
0.1.0?" with what it asks for and why. Allow brings it up, with every ask
allowed. A plugin folder that is added while Hamlet runs is found at the next
start.

**5. Edit and see it reload.** With Hamlet running, change `src/lib.rs`, say the
button's label, and run `build.sh` again. Within half a second the host builds
the new version beside the running one, and when it starts, takes its place:

```
INFO hamlet_wasm::reload::swap: plugin tide_table: reloaded 0.1.0
```

A build that does not load leaves the running version in place, logs why and
shows the reason as a dim line under the plugin on the Plugins page of
Settings, with a ROLL BACK button:

```
WARN hamlet_wasm::reload::swap: plugin tide_table: reload failed: could not load: failed to parse WebAssembly module: magic header not detected: ...
```

The reload, what it keeps and what it loses are in [`wasm.md`](wasm.md). The new
version starts from its `entry` with the same state and the same storage, so a
panel keeps what it showed.

The log lines above are from a headless run (`--headless`) against a scratch
home that already recorded the plugin as installed. The window was not part of
that run.

### A data plugin

```sh
scripts/new-plugin.sh data greeter /tmp/greeter
cargo run --example plugin_check --features wasm -- \
  /tmp/greeter --script /tmp/greeter/steps.toml --out /tmp/greeter/pictures
```

There is nothing to build. To install, copy the folder to
`<home>/plugins/greeter/`. Saving `panel.ui` there reloads it, and the panel
keeps its state. The nodes of a panel written in the language get ids made up
by the language (`n.1`), which is what `click` and `type` name; the transcript
shows them.

## The manifest

```toml
id = "tide_table"
name = "Tide Table"
version = "0.1.0"
interface = "0.1.0"

[[asks]]
capability = "read.agents"
reason = "To count the agents the panel is about."
```

- `id` is the folder's name, written as a name (`a-z`, `0-9`, `.`, `_`, `-`, 64
  bytes at most) and not `host`, `chief`, `window`, `needs` or `skills`. `name` and `version` are what Settings shows.
- `interface` is the version of the host interface the plugin was built for,
  `interface::VERSION`, and a wasm plugin needs it. A data plugin has `kind =
  "data"` and an optional `icon` (the name of an icon) instead.
- `[[asks]]` name the most the plugin may use, each with a `capability` and a
  `reason` the chief reads. A capability that is not named cannot be used or
  asked for. Name only what the plugin uses, with a reason that says what for.
- `ticks` is true when the manifest does not say, and the plugin is given a
  `tick` at most `tick_hz` times a second. A plugin that does something only when
  it is sent something (a click, a command, an event) sets `ticks = false`, and is
  not stepped on frames in which nothing came for it.
- `needs` and `wants` name plugins this one depends on, each an `id` and a
  version `range`; a plugin whose needs are not met does not start, and Settings
  says why. `exports` and `imports` list the commands, events and collections the
  plugin offers and uses; nothing outside them is reachable. See
  [Two plugins working together](#two-plugins-working-together).

A build for another interface version is not loaded, and the reason says "built
for interface X, this Hamlet speaks Y". Build the plugin again from the
template. A new version that asks for more than the one before waits for the
chief's answer before it runs, as `wasm.md` says.

## Permissions

The chief chooses for each ask: Allow, Ask next time or Don't allow
([`permissions.md`](permissions.md)). A plugin sees it like this:

| The verdict | A gated call | `request` |
|---|---|---|
| Allowed | works | `granted` |
| Denied | returns its error | `denied` |
| Not decided | returns its error, and the chief is asked | `pending`, and the chief is asked |
| Not in the manifest | returns its error | `denied`, and nobody is asked |

The answer to a `pending` request reaches the plugin as `granted` or `denied`
in its next `step`. The template redraws on either, so the panel shows the
answer. Do not fail on an error: show a line that says what is missing.

In the harness the plugin is installed with every ask undecided. `grant` and
`deny` answer as the chief does, and the plugin gets its message.

## The script

A script is a TOML file with a list of `[[step]]`. Each step has one key:

| Step | Does |
|---|---|
| `tick = 2` | Runs two frames; the plugin gets a tick in each |
| `click = { panel = "main", node = "look" }` | Presses a button |
| `type = { panel = "main", node = "name", text = "Ada" }` | Commits a text in a text field or a text area |
| `toggle = { panel = "main", node = "hint" }` | Flips a checkbox |
| `run-menu = "New note"` | Chooses the menu line of that label, as the chief does. It fails while the plugin does not hold `ui.menu` |
| `press = "Cmd+Shift+N"` | Presses a chord. A plugin that does not hold `ui.keys` does not hear of it |
| `set-agents = [{ name = "Juniper", hamlet = "Pear Tree Hollow" }]` | Replaces the agents a plugin that may read them sees |
| `set-papers = { notes = "..", character = "..", skills = ["a"] }` | Replaces the notes, character and skill names of the selected agent that a plugin that may read them sees |
| `set-selected = "Juniper"` | Selects the agent of that name in the world, or none for `""`; a plugin that imports `host/selection-changed` and may read the agents hears of it |
| `set-world = { hamlets = [{ id = "home", name = "Pear Tree", at = [0, 0, 0] }], agents = [{ name = "Juniper", hamlet = "home", at = [4, 0, 2], busy = true }], selected = "agent:Juniper", hour = 9.5 }` | Replaces the realm a plugin that may read it (`read.world`) sees: its hamlets, its agents and where each stands, the thing selected as `kind:name` and the hour |
| `choose-file = "tide.csv"` | Closes the file dialog a plugin opened with `pick-file` with this file chosen, a path from where the check runs, or with nothing for `""`. No dialog is shown; it fails where no plugin asked for a file |
| `model-replies = ["noon"]` | Makes these the replies of the model, in order, to the plugins' `model-call`s. No server is reached, and until this step there is no model |
| `choose-folder = "notes"` | Closes the folder dialog a plugin opened with `pick-folder` with this folder chosen, or with nothing for `""`, as `choose-file` does |
| `plugin = "clipper"` | Puts a plugin in front, where several were opened: the steps after it are done to that one |
| `grant = "read.agents"` | The chief allows a capability the manifest asks for. For a scoped capability, `plugin.use:notes` or `collections.read:notes/notes` |
| `deny = "read.agents"` | The chief refuses it |
| `render = { panel = "main", file = "after.png" }` | Draws the panel in a dock, as the window does, to a PNG under `--out` |
| `context = { subject = "agent:Juniper", item = "Clip name to Notes", file = "menu.png" }` | Chooses the item of the menu of that thing, `kind:name`, as the chief does: the plugin's command runs on the thing. `item` is the item's label, or what it says of the thing. `file` is optional: a picture of the menu, drawn before. It fails while the plugin does not hold `ui.context` |
| `section = { title = "NOTES", file = "tab.png" }` | Draws the inspector's page of the selected agent turned to the plugin's tab of that title. It fails while the plugin does not hold `ui.inspector` |
| `page = { title = "NOTES", file = "page.png" }` | Draws the plugin's page of that title in the Settings. It fails while the plugin does not hold `ui.settings` |
| `expect = { panel = "main", text = "2 agents" }` | Fails unless a node of the panel shows the text |
| `expect-told = "ran notes/new"` | Fails unless a notice the chief was given in the chat holds the text, such as the one that tells of an act done as the chief. The harness has no chat and no network, so a message ends in "sent nothing" and a fetch to a listed host in "there is no network" |
| `expect-overlay = "circle 5 at agent:Juniper"` | Fails unless a mark the plugin draws over the world, as the transcript writes it, holds the text. The transcript shows the marks, one line each, when they change |
| `call-tool = { tool = "height", args = '{"port": "Dover"}', shows = "6.1 metres" }` | Has an agent call a tool the plugin gave, the way the app carries a call, and fails unless the text returned holds `shows`. `hamlet` (default `Home`) is the agent's hamlet, `fails = true` expects an error and checks `shows` in it, and `offered = false` checks the tool is not offered (before `grant = "agent.tool"`, or to another hamlet) and calls nothing |
| `steer = { tool = "bash", args = "{}", expect = "hold", shows = "deletes" }` | Puts a point of a turn of `agent` (`Juniper`) of `hamlet` (`Home`) to the plugins that hold `agent.steer`, as the app does before the call of `tool`, or before a model call where it is left out, and checks what the plugin in front made of it: `note`, `hold`, `failed` or `none`, with `shows` in its words |
| `turn = { phase = "after", agent = "Juniper", number = 2, tools = ["bash"] }` | Tells the plugins that an agent's turn began (`before`) or ended (`after`), as the app does, to those that hold `agent.hook` and import the host event. `hamlet` defaults to `Home`; a plugin whose ask lists other hamlets does not hear of it |
| `restart = true` | Quits the app and starts it again on the same home. What the plugins kept on disk (storage, files, database) and what the chief decided is there; what they held in memory, including the state the host keeps for them, is not |

`panel` is the id the plugin gave the panel (`main`), and `node` the id it gave
the node. A click on a node that is not there fails and lists the ids that are.
Every action runs three frames, so the plugin sees it, answers and has its
panels drawn again. A panel is asked for its tree again only after an event
reached it or the plugin asked for a redraw, so after `set-agents` click or
redraw before looking at the agents.

`plugin_check <plugin dir>... [--script steps.toml] [--out dir] [--no-pictures]`
runs it. Several folders are installed in one home, so that plugins that use each
other can be checked together; the first is in front until a `plugin` step says
another, a panel is then named `plugin/panel` in the transcript and a log line
starts with its plugin. With no script it runs two frames and draws every panel
of each plugin. The exit code
is 0 when every step worked, 1 when one failed and 2 for bad arguments or a
folder that does not load. Pictures are drawn offscreen and need a GPU;
without one a `render`, `section` or `page` step fails with that reason, and
`--no-pictures` leaves them out, and the picture of a `context` step.

### In Rust

`hamlet_harness::Harness`, behind the `wasm` feature, is what the script runs
on:

| Call | Does |
|---|---|
| `Harness::open(folder)` | Installs the plugin folder in a scratch home that goes with the harness, and brings it up |
| `Harness::open_all(&[folder, ..])`, `select(id)`, `ids()` | The same for several plugins in one home. What is done next is done to the one in front, the first until `select` says another |
| `sized(size)` | The size of the pictures, in pixels |
| `tick()` | Runs one frame |
| `click(panel, node)`, `type_text(panel, node, text)`, `toggle(panel, node)` | Do what a person does to a node; an error says what is not there |
| `run_command(id)`, `run_menu(label)`, `press(chord)` | Do what the chief does: run a command (`plugin/command`, or the name alone for the plugin in front), choose a menu line, press a chord |
| `run_context(subject, item)` | Chooses an item of the menu of a thing, `kind:name`, which runs the plugin's command on it |
| `sections()`, `pages()` | The titles of the tabs and pages the plugin in front added that show |
| `render_context(subject, path)`, `render_section(title, path)`, `render_page(title, path)` | Draw the menu of a thing with the plugin's items, the inspector's page of the selected agent turned to the plugin's tab, or the plugin's page in the Settings, to a PNG |
| `set_agents(&[(hamlet, name)])` | Makes up the agents |
| `set_papers(notes, character, &skills)` | Makes up the papers of the selected agent |
| `set_selected(name)` | Makes up the agent selected in the world |
| `set_world(survey)` | Makes up the realm a plugin that holds `read.world` reads |
| `overlay_of(id)` | The marks the plugin draws over the world, one line each |
| `choose_file(file)` | Chooses a file in the place of the dialog a plugin opened, or nothing |
| `grant(capability)`, `deny(capability)` | Answer as the chief, for the plugin in front. A scoped capability is `plugin.use:notes` |
| `tree(panel)`, `outline(panel)` | The tree a panel shows, as nodes and as lines |
| `panels()`, `logs()`, `stopped()` | The panels the plugin declared, what it logged, why it stopped (`tree_of`, `outline_of`, `panels_of` and `stopped_of` take a plugin id) |
| `render(panel, path)` | Draws the panel to a PNG |

`hamlet_wasm`'s `fixture` feature makes test plugins from text, as the host's
own tests do.

## A menu line and a shortcut

A plugin puts a line in a menu and binds a key by naming a command of its own. The
command is what runs; the line and the key only point at it. Notes does both for
`new`, which adds an empty note and opens it. Its `manifest.toml` lists the command,
and asks for the two capabilities with a reason each:

```toml
[exports]
commands = ["add", "new"]

[[asks]]
capability = "ui.menu"
reason = "To put New note in the HAMLET menu."

[[asks]]
capability = "ui.keys"
reason = "To start a new note with Cmd+Shift+N."
```

```rust
fn entry() {
    hamlet_guest::panel("main", "NOTES", "notes");
    hamlet_guest::command("new", "New note", Some("plus"));
    hamlet_guest::menu_item("hamlet", 1, "New note", "new");
    hamlet_guest::key("Cmd+Shift+N", "new");
}

// in step, for each Incoming::Command(call): the chief is `call.sender`
"new" => open_new(session, index, "", out)
    .inspect(|_| out.push(hamlet_guest::open_panel("main"))),
```

`menu` is `hamlet` or `view`. Lines are grouped by `section` and a hairline stands
between groups. `Cmd` is the command key of a Mac and Control elsewhere. A key
needs Cmd, Ctrl or Super to work while a text field has the focus, so a shortcut
never takes what is typed. Every command is answered with `reply`, the chief's too:
an error is shown to the chief as a notice.

The line and the key are held until the chief allows `ui.menu` and `ui.keys`
([`permissions.md`](permissions.md)). A script grants them and uses both:

```toml
[[step]]
grant = "ui.menu"

[[step]]
grant = "ui.keys"

[[step]]
run-menu = "New note"

[[step]]
press = "Cmd+Shift+N"
```

A chord that another plugin has too runs neither, and one that Hamlet uses is not
the plugin's; the Plugins page of Settings says so.

## An item of a context menu

A right click on a thing of the world opens a menu, and a plugin adds an item to the
menu of one kind of thing (`agent`, `house`, `mayor`, `ruin`, `hamlet`, `chief`,
`hearth`, `library` or `ground`, see [`sdk.md`](sdk.md#a-subject)). The item runs a
command of the plugin, which is given the thing. Clipper adds "Clip name to Notes"
to the menu of an agent, and its command sends the name to Notes:

```toml
[exports]
commands = ["clip"]

[[asks]]
capability = "ui.context"
reason = "To put Clip name to Notes in the menu of an agent."
```

```rust
fn entry() {
    hamlet_guest::command("clip", "Clip name to Notes", None);
    hamlet_guest::context_item("agent", 200, "Clip name to Notes", "clip");
}

// in step, for each Incoming::Command(call) with call.command == "clip"
let thing = hamlet_guest::subject::of(&call)?;   // kind, id, name
```

Items are ordered by `section`, then by label; Hamlet's own items use the sections 10
to 120, so 200 comes after them. `label` may hold `{name}`, the thing's name. The
command is answered with `reply`, as every command is. The item is held until the
chief allows `ui.context`; a script grants it, draws the menu and chooses the item:

```toml
[[step]]
grant = "ui.context"

[[step]]
context = { subject = "agent:Hazel", item = "Clip name to Notes", file = "context.png" }
```

## A tab of the inspector and a page of the settings

A tab and a page show a panel of the plugin, so the plugin declares the panel first
and draws it as it draws any other. Notes has both:

```toml
[imports]
events = ["host/selection-changed"]

[[asks]]
capability = "read.agents"
reason = "To know which agent is selected, for its tab in the Inspector."

[[asks]]
capability = "ui.settings"
reason = "To put its page, NOTES, in the Settings."

[[asks]]
capability = "ui.inspector"
reason = "To put a NOTES tab in the Inspector's page of an agent."
```

```rust
fn entry() {
    hamlet_guest::panel("settings", "NOTES SETTINGS", "sliders");
    hamlet_guest::panel("mentions", "NOTES OF AN AGENT", "book");
    hamlet_guest::settings_page("NOTES", "notes", "settings");
    hamlet_guest::inspector_section("agent", "NOTES", "mentions");
}

// in panel_view: "settings" has the checkbox "Open notes in preview", and
// "mentions" lists the notes that mention the selected agent
```

The tab takes the icon of its panel. The plugin learns which agent is selected from
the event `host/selection-changed` (its payload is the name, empty for none) and
draws the panel again with a `redraw` intent. A choice made on the page, such as the
checkbox, arrives as `toggled` of the panel `settings`, and Notes keeps it in its
storage and reads it when it opens a note. The command `new` of Notes brings its
panel forward with `open_panel("main")`, an intent returned from `step` that needs no
capability and works only for the plugin's own panels. A script grants the
capabilities and draws both pages:

```toml
[[step]]
grant = "ui.inspector"

[[step]]
section = { title = "NOTES", file = "agent-tab.png" }

[[step]]
grant = "ui.settings"

[[step]]
page = { title = "NOTES", file = "settings-page.png" }
```

At the width the inspector has by default a tab of a plugin, with the four of its
own, does not fit, and the strip folds into a list beside the tab that is open.

## Acting for the chief, keeping files and fetching

Four more things a plugin may do are behind a grant, each asked for in the
manifest with a reason (`guests/herald` does all four):

```toml
[[asks]]
capability = "chat.send"            # a message to an agent, as the chief
reason = "To say hello to the agent scout."
scope = ["scout"]                   # the agents it may write to

[[asks]]
capability = "commands.run"         # a registered command, as the chief
reason = "To open a new note for you."
scope = ["notes/new"]               # the commands it may run

[[asks]]
capability = "files.write"          # and files.read: its own folder only
reason = "To keep a draft."

[[asks]]
capability = "net"
reason = "To fetch its greeting page."
scope = ["api.example.com"]         # the hosts, over https
```

```rust
// Returned from `step`. The first use asks the chief and is dropped; the
// answer arrives as `Incoming::Granted`/`Denied`, and the plugin tries again.
out.push(hamlet_guest::send_chat("scout", "Hello"));
out.push(hamlet_guest::run_as_chief("notes/new"));
out.push(hamlet_guest::net::get(1, "https://api.example.com/hello"));

// The answer to the fetch, in a later `step`.
if let Some((1, outcome)) = hamlet_guest::net::answer(&incoming) { /* .. */ }

hamlet_guest::files::write("drafts/a.txt", b"hello")?;
let bytes = hamlet_guest::files::read("drafts/a.txt")?;
```

A message and a command are noted in the chat as the plugin's, and a message
reaches the agent as `[sent by plugin <id>] ...`. Each reaches only what the
`scope` of its ask lists; with no `scope` it reaches nothing. Each is limited to a number a
minute. A path of the folder of files that has `..`, is absolute or goes through a
link is an error. A fetch to a host that is not in the `scope`, or over http, is
answered with an error. The limits are in `assets/wasm.toml`; the rules are in
[`permissions.md`](permissions.md) and [`wasm.md`](wasm.md). In a script, `grant`
answers an ask and `expect-told` checks the notice that tells of an act.

A file from outside its folder reaches a plugin only as the chief picks it, with
`fs.pick` (and `files.read` to read the copy):

```rust
out.push(hamlet_guest::files::pick(7, "a tide table"));

// In a later `step`: the path of the copy in the plugin's folder.
if let Some((7, Ok(path))) = hamlet_guest::files::picked(&incoming) {
    let bytes = hamlet_guest::files::read(path)?;
}
```

The system's dialog names the plugin; the file the chief picks is copied to
`picked/<name>` in the folder, within its caps, and the plugin never sees where
it came from. In a script, `choose-file = "tide.csv"` picks in the dialog's place,
and `choose-file = ""` closes it with nothing.

## Keeping records in a database

Where keys and values are too few, a plugin keeps tables of records in a database
of its own, behind the capability `db`, which the chief allows when the plugin is
let in (`guests/pantry` is the example):

```toml
[[asks]]
capability = "db"
reason = "To keep what is in the pantry."
```

```rust
const STOCK: Table<Item> = Table::new("stock");   // hamlet_guest::db::Table

// A record: an id, a value written with postcard, and fields to find it by.
STOCK.put("tea", &Item { name: "Tea".into(), count: 2 }, &[("kind", "drink")])?;
let tea: Option<Item> = STOCK.get("tea")?;
let page = STOCK.find("kind", "drink", None, 20)?;   // page.rows, page.next
STOCK.list(page.next.as_deref(), 20)?;               // by id, in pages
STOCK.delete("tea")?;
```

A table is of `a-z`, `0-9`, `_` and `-`, and is there once it holds a record. A
record has at most a few fields, each a short name and value, and a find looks
at them without decoding the value. The records are on disk, so they are there
after a reload, a restart and a switch off, and the chief decides at removal
whether they stay. They are the plugin's own: another plugin gets them only
through a collection. The caps are the `db_` values of `assets/wasm.toml`, and
`hamlet_guest::db` has the bare calls for a plugin that writes its own bytes. In a
script, `restart = true` starts the app again to see the records are still there.

## Two plugins working together

Plugins meet through the host only: a command, an event or a collection, each
listed in the manifests of both and allowed by the chief
([`wasm.md`](wasm.md) has the rules and the errors). `guests/notes` and
`guests/clipper` are a pair: Clipper sends the selected agent's name to Notes,
lists the titles of the notes it reads from Notes and counts the notes Notes
saved.

**The plugin that offers.** Notes lists what it offers in its `manifest.toml`,
declares the command in `entry` and answers it in `step`:

```toml
[exports]
commands = ["add"]
events = ["note-saved"]

[[exports.collections]]
name = "notes"
prefix = "note."       # the notes are the keys of its storage that start so
```

```rust
fn entry() {
    hamlet_guest::panel("main", "NOTES", "notes");
    hamlet_guest::command("add", "Add a note", None);
}

// in step, for each Incoming::Command(call)
let added = match call.command.as_str() {
    "add" => calls::args::<String>(&call).and_then(|words| add(index, &words)),
    other => Err(format!("no command `{other}`")),
};
out.push(calls::reply(&call, added.as_ref().map_err(String::clone)));
// and after a note is saved
if let Ok(intent) = events::publish("note-saved", &id) {
    out.push(intent);
}
```

`calls::args` reads the arguments the caller wrote and `calls::reply` writes the
answer, both with `postcard`, so the two plugins use the same type. Every
command is answered, with an error where it cannot be done.

**The plugin that uses.** Clipper needs Notes, lists what it imports and asks for
`plugin.use`, for the collection and for the agents:

```toml
[[needs]]
id = "notes"
range = ">=0.1, <1"

[imports]
commands = ["notes/add"]
events = ["notes/note-saved", "notes/collection-changed", "host/selection-changed"]
collections = ["notes/notes"]

[[asks]]
capability = "plugin.use"
reason = "To send what it clips to Notes and hear when a note is saved."
scope = ["notes"]

[[asks]]
capability = "collections.read"
reason = "To list the titles of the notes."
scope = ["notes/notes"]

[[asks]]
capability = "read.agents"
reason = "To know which agent is selected."
```

A call starts in `step` and its answer comes in a later one, so what a call is for
is kept in `State`, in `hamlet_guest::Calls`, with a tag of the plugin's own type:

```rust
#[derive(Default, Serialize, Deserialize)]
struct Model { calls: Calls<Why>, selected: String, line: String, saved: u32 }

// the button
if let Ok((_pending, intent)) =
    model.calls.call("notes", "add", &name, Why::AddNote)
{
    out.push(intent);
}

// each incoming, first
if let Some((Why::AddNote, outcome)) = model.calls.route(&incoming) {
    model.line = match calls::returned::<u64>(&outcome) {
        Ok(id) => format!("Notes saved it as note {id}."),
        Err(why) => format!("Notes did not take it: {why}."),
    };
}
```

`calls::returned` turns the outcome into the type Notes answered with, and an
error, whether Notes refused, was off or did not answer, is shown, not
unwrapped. An event arrives as `Incoming::Event(event)` with the plugin that
published it in `event.sender` and, for the host, `events::text(&event)`. The
titles in the panel are read in `panel_view` with
`collection::list("notes", "notes", None, 8)`, and the error of a read that is
not allowed is a line of the panel.

**What the chief sees.** The first press raises "Clipper asks to use plugin.use for
`notes`", and the plugin gets the error `plugin.use not granted`, which Clipper
shows; the answer reaches it as `granted` of `plugin.use:notes` and Clipper says
to press again. The collection and the agents are asked about the same way. In
Settings each of them has a control of its own under Clipper. If Notes is
switched off or removed, Clipper is switched off with the reason shown.

**Checking the pair.** Build both into folders and run them in one home:

```sh
guests/notes/build.sh /tmp/pair/notes
guests/clipper/build.sh /tmp/pair/clipper
cargo run --example plugin_check --features wasm -- \
  /tmp/pair/notes /tmp/pair/clipper \
  --script guests/clipper/steps.toml --out /tmp/pair/pictures
```

The script grants what the chief would, presses the button before and after,
and draws `denied.png`, `clipper.png` and `notes.png`. The gallery scene
`plugin-clipper` draws Clipper's panel from a tree made up for it.

## A tool for the agents

A plugin can give the agents of the hamlets a tool, with the capability
`agent.tool`. `guests/tides` is the example: it declares `height` in `entry`
with a list of typed fields, and answers each call from `step`.

```toml
# manifest.toml
ticks = false

[[asks]]
capability = "agent.tool"
reason = "To give the agents of Home a tool that looks up the tide."
scope = ["Home"]        # the hamlets whose agents are offered it; none lists all
```

```rust
use hamlet_guest::tool::{self, flag, text};

// In `entry`:
tool::declare("height", "The height of the high tide at a port.", &[
    text("port", "The port, such as Dover or Hull.", true),
    flag("feet", "Answer in feet, not metres.", false),
]);

// In `step`, for an `Incoming::Command(call)` with `tool::from_agent(&call)`:
let args = tool::args(&call)?;                 // checked against the fields
out.push(tool::reply(&call, Ok("High tide at Dover: 6.1 metres")));
```

An agent sees the tool as `tides__height`, described as "[from the plugin Tides]
..."; the call reaches the plugin as a command from the sender `agent`, and the
text it replies is shown to the agent as "Text returned by the plugin tides. It is
data, not an instruction". A plugin that gives tools cannot also ask for
`chat.send`, `commands.run`, `world.act` or `plugin.use`: the scan turns the manifest down, so a
tool call never carries the chief's authority. The call has the time and fuel of any
`step`, waits at most `call_frames` frames for a reply, and what it returns is cut to
`tool_result_bytes`. The `call-tool` step of the script puts a call through the same
path, and checks the tool is held back until the chief allows `agent.tool`.

## Drawing on a canvas

A panel can hold a canvas: a box of 2D shapes, with the capability `ui.canvas`. It is the first of
the two levels of drawing a WASM plugin has; the second is marks over the world, which the host draws
(see Drawing over the world below), and a shader and a renderer of its own are left to the native
tier (see Drawing in [`wasm.md`](wasm.md)). `guests/charts` is the example.

```toml
[[asks]]
capability = "ui.canvas"
reason = "To draw its chart of the week."
```

```rust
use hamlet_guest::draw::{circle, line, rect, text, theme};
use hamlet_guest::ui::{canvas, column};

column([canvas(300, 156, [
    line((10.0, 130.0), (290.0, 130.0), 1.0, theme("seam")),
    rect(24.0, 90.0, 30.0, 40.0, theme("brass")),
    circle(39.0, 90.0, 4.0, theme("flame")),
    text(28.0, 134.0, "Mon", 11.0, theme("dim")),
])])
```

Colours by theme name follow the theme; `rgba(r, g, b, a)` is a colour of its own. The canvas holds at
most `canvas_px` pixels a side and the tree `canvas_shapes` shapes. Until the chief allows
`ui.canvas` the panel shows a note in its place; the harness shows it as such, and the script's
`grant = "ui.canvas"` answers. The transcript lists a canvas and its shapes, one line each, and
`expect` finds the words of a `text` shape. The gallery scene `plugin-canvas` draws a canvas.

## Drawing over the world

A plugin with `world.overlay` gives the host a list of marks, and the host draws them over the
REALM panel each frame where their anchors are seen: a robot by its agent's name, a hamlet's
common by its id, or a point of the world. `guests/surveyor` is the example: it reads the world
with `read.world`, marks the agents at work, and with `world.act` brings the camera to one
(`hamlet_guest::world::focus`, `follow` and `select`, which the chief allows at the first use and
for the hamlets the ask's `scope` lists).

```toml
[[asks]]
capability = "world.overlay"
reason = "To label the agents it watches over the world."
```

```rust
use hamlet_guest::draw::theme;
use hamlet_guest::overlay::{self, agent, circle, hamlet, line, text};

out.push(overlay::draw(vec![
    circle(agent("Juniper"), 0.0, -40.0, 5.0, theme("lamp")),
    text(agent("Juniper"), 8.0, -48.0, "busy", 12.0, theme("ink")),
    line(agent("Juniper"), (0.0, 0.0), Some(hamlet("home")), (0.0, 0.0), 1.5, theme("seam")),
]));
// An empty list takes the marks away.
out.push(overlay::draw(Vec::new()));
```

The offsets are pixels from where the anchor is seen, so a label stays the same size however far
the camera is; a mark whose anchor is out of sight is not drawn. Each `draw` replaces the last, at
most a few times a second, and the marks, their words and the changes a second are capped
([`wasm.md`](wasm.md)). The panel's corner says "Drawn by" and the plugin's name while it draws. The
script's `expect-overlay` checks a mark as the transcript writes it, such as `circle 5 at
agent:Juniper +0,-40 lamp`, and `set-world` makes up the world a plugin reads. The gallery scene
`plugin-overlay` shows Surveyor's marks over a picture of the world.

## Watching the agents

A plugin with `agent.hook` is told when an agent's turn begins and ends, and only told:
it cannot change or stop the turn, and it never hears what was said. `guests/diary` is
the example: it imports the host events and reads each with `hamlet_guest::events::turn`.

```toml
[imports]
events = ["host/before-turn", "host/after-turn"]

[[asks]]
capability = "agent.hook"
reason = "To keep a count of the turns the agents of Home take and the tools they use."
scope = ["Home"]        # the hamlets it hears of; none lists all
```

```rust
if let Incoming::Event(event) = incoming
    && let Some(turn) = hamlet_guest::events::turn(&event)
{
    // turn.agent, turn.hamlet, turn.number and, after, turn.tools (names only)
}
```

The script's `turn = { phase = "after", agent = "Juniper", number = 2, tools = ["bash"] }`
tells the plugin of a turn as the app does (`hamlet` defaults to `Home`). A plugin with
`agent.hook`, `agent.steer` or `agent.tool` cannot also ask for `chat.send`, `commands.run`,
`world.act` or `plugin.use`.

A plugin with `agent.steer` may change a turn, only to make it stricter: before a model call it
may give the agent a short note, and before a tool call it may hold the call back with a reason.
It answers `Incoming::Steer` in the same `step`, which has a small budget of its own and which the
turn waits for:

```rust
if let Some(point) = hamlet_guest::steer::point(&incoming) {
    match &point.tool {
        None => intents.push(hamlet_guest::steer::note(point, "Keep replies short.")),
        Some(tool) if tool.args.contains("rm -rf") => {
            intents.push(hamlet_guest::steer::hold(point, "it deletes a folder"))
        }
        Some(_) => {}
    }
}
```

The script's `steer = { tool = "bash", args = "{\"command\": \"rm -rf notes\"}", expect = "hold" }`
puts a point to the plugin as the app does, before a model call where `tool` is left out, and
checks what it made of it: `note`, `hold`, `failed` or `none`, with `shows` for the words.

## Limits

Every plugin is held to the limits of `assets/wasm.toml` whatever it is
allowed: the time, fuel and memory of a call, log lines, panels, the nodes, depth and
text of a tree, events a second, intents a call, the events it publishes a second
and a step, the ticks a second, the calls waiting and how deep
they go, and the size of a page of a collection ([`wasm.md`](wasm.md)). A
call that traps, runs out of time or fuel or returns a bad tree stops the plugin;
its panels say so and have a RESTART button, as does its line on the Plugins
page. The host starts it again once by itself, a few seconds later, before it
waits for the chief.

A plugin may import only the host's functions. One written in Rust with `std`
loads, and one that prints or reads the clock traps: log with
`hamlet_guest::log`.

## Common mistakes

- A button, checkbox or text field with no `.id(..)`, or two siblings with the
  same id: the tree is turned down, the call fails and the plugin is stopped.
- Asking for a capability the plugin does not use, or with no reason, or one
  that is never granted (`keys`, `petitions.answer`, `home.raw`, `process.run`,
  `sandbox.off`, `storage.other`): the manifest is turned down.
- Calling a gated function and failing on its error instead of showing a line.
- A manifest `interface` that is not the one the plugin was built for.
- Looking at the agents after `set-agents` without a click or redraw.
- Counting on state across a restart of Hamlet. Only storage outlives it.
- Changing the type of a state or a stored value and expecting the old bytes to
  read.
- A folder whose manifest `id` is not the folder's name.
- Running a command, hearing an event or reading a collection of a plugin that
  the manifest does not list under `imports`: the call is answered with an
  error. The same for a command the other plugin does not list under `exports`.
- Not answering a command (the caller gets an error after `call_frames`
  frames), or giving a `call-id` that is still waiting.
- Using another plugin without `needs` for it: the call fails when it is not
  running, where `needs` would have kept this plugin from starting.
- A `menu_item` or a `key` for a command that `entry` did not declare, or the
  manifest did not list under `exports.commands`: it is dropped with a warning.
- A menu line or a shortcut with no `ui.menu` or `ui.keys` in the manifest: it
  never shows, and the chief is not asked. The same for a context menu item with
  no `ui.context`, a tab with no `ui.inspector` and a page with no `ui.settings`.
- A context item for a kind of thing that there is not (the list is in
  [`sdk.md`](sdk.md#a-subject)), or a tab for a page that has no tabs (only
  `agent`): it is dropped with a warning.
- A tab or a page for a panel that `entry` did not declare: it shows nothing.
- Reading `args` of a command a context item runs as anything but a
  `hamlet_guest::Subject`: it is what the chief sent.
- Putting a call's state in a variable: the answer comes in a later `step`, so
  keep it in `State`, as `hamlet_guest::Calls` does.

## Building from an agent's sandbox

An agent's commands run in the sandbox of [`sandbox.md`](sandbox.md): no
network, writes only in its workspace (and in the folder of a hamlet that works
on one). Whether a wasm plugin can be built there was tried on macOS with the
Seatbelt policy an agent's shell gets, run through `hamlet_sandbox` itself with
the policy unchanged. Linux with bubblewrap was not tried.

**Where the hamlet has no folder** (the workspace-only policy: the system
folders and the workspace are readable, only the workspace is writable): it
fails. `cargo` is a rustup shim and stops at "could not read settings file
`~/.rustup/settings.toml`: Operation not permitted", and the toolchain's own
`rustc` cannot open its libraries, since both are under the home folder. It
works only when everything the build reads is in the workspace. That was made
to work by hand, and nothing here does it for you:

- a copy of the toolchain in the workspace: `bin/cargo`, `bin/rustc`, the
  `lib/*.dylib`, `lib/rustlib/<host>` and `lib/rustlib/wasm32-wasip2`, about
  520 MB, with a link `lib/rustlib/<host>/lib/libLLVM.dylib` to
  `../../../libLLVM.dylib`;
- a copy of `crates/hamlet_guest` and `crates/hamlet_wasm/wit` in the
  workspace, with the guest's `Cargo.toml` written out without its fields
  inherited from the workspace, and the plugin made with `HAMLET_SRC` pointing
  at it;
- the crates, vendored with `scripts/vendor-plugin-deps.sh <plugin dir>`;
- the command `RUSTC=<toolchain>/bin/rustc CARGO_HOME=<workspace>/cargo-home
  <toolchain>/bin/cargo build --offline --release --target wasm32-wasip2`.

With those it built the template to a 58 KB `plugin.wasm` in about 5 seconds.
A toolchain installed under `/usr` or `/opt/homebrew`, which the base policy
reads, may need less; none was installed here to try.

**Where the hamlet works on a folder** (the open policy: the machine is
readable, only that folder and the workspace are writable): it works, with the
plugin in the folder.

```sh
cargo build --offline --release --target wasm32-wasip2
```

It needs the crates without the network. They are there when they are vendored
into the plugin (`scripts/vendor-plugin-deps.sh <plugin dir>`, run by someone
with a network, which also writes `.cargo/config.toml` to read them), or when
the cargo cache of the home folder already holds them, which the policy lets a
build read without writing. With neither (an empty cache and nothing vendored)
the build fails to resolve `wit-bindgen`. With the crates vendored it built
with an empty, workspace-local `CARGO_HOME` too. The wall limit of a command is
120 seconds and its CPU limit 60: the template takes about 5.

**Checking from the sandbox.** `plugin_check` is run as a program that was
built already, since building Hamlet in the sandbox needs a writable `target`
beside the source and far more than the limits allow. Run in the open policy it
printed the whole transcript. A `render` step failed there with "Unable to find
a GPU", since the policy does not give a command the graphics device, and the
step reports that as a failure; use `--no-pictures`, and take the pictures
outside the sandbox.

So an agent can write and test a plugin from its sandbox only where its hamlet
works on a folder, with the crates vendored (or already cached) and
`plugin_check` built beforehand; it gets the text of every panel, not the
pictures. Without a folder it cannot build at all unless the chief sets up the
workspace as above. The chief can always build the agent's folder outside the
sandbox.
