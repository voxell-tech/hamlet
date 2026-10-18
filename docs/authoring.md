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

## What is here

| Where | What it is |
|---|---|
| `templates/plugin/` | A wasm plugin to copy: `Cargo.toml`, `manifest.toml`, `src/lib.rs`, `build.sh`, `steps.toml` |
| `templates/data-plugin/` | A data plugin to copy: `manifest.toml`, `panel.ui`, `steps.toml` |
| `scripts/new-plugin.sh` | Copies a template into a folder and fills in its placeholders |
| `scripts/vendor-plugin-deps.sh` | Copies the crates a wasm plugin builds from into it, for a build with no network |
| `crates/hamlet_harness` | The harness that runs a plugin headlessly: a `Harness` to drive in Rust, and a script runner |
| `examples/plugin_check` | The harness as a command |
| `crates/hamlet_home/starters/write-a-plugin` | The starter skill that teaches an agent this page's steps |

The template plugin has a panel, a button and one read behind a capability
(`read.agents`). It asks to use the capability with `request`, shows a line for
each answer (`granted`, `denied`, or still `pending`), and redraws when the
chief answers. It keeps no state.

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
/tmp/tide_table is ready: wasm plugin tide_table, interface 0.2.0
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
`wasm` feature, and the first run builds Hamlet's crates.

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
      note "2 agents live here." #count
step 6: expect main shows "2 agents live here"
step 7: click main/look
step 8: render main to allowed.png
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
HAMLET_HOME=/tmp/hamlet-try cargo run --features wasm
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

The reload, what it keeps and what it loses are in [`wasm.md`](wasm.md). A
plugin has no storage yet, so it starts again from its `entry` and its state
is lost.

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
interface = "0.2.0"

[[asks]]
capability = "read.agents"
reason = "To count the agents the panel is about."
```

- `id` is the folder's name. `name` and `version` are what Settings shows.
- `interface` is the version of the host interface the plugin was built for,
  `interface::VERSION`, and a wasm plugin needs it. A data plugin has `kind =
  "data"` and an optional `icon` (the name of an icon) instead.
- `[[asks]]` name the most the plugin may use, each with a `capability` and a
  `reason` the chief reads. A capability that is not named cannot be used or
  asked for. Name only what the plugin uses, with a reason that says what for.
- `needs`, `exports` and `imports` are read, and nothing acts on them yet.

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
| `type = { panel = "main", node = "name", text = "Ada" }` | Commits a text in a text field |
| `toggle = { panel = "main", node = "hint" }` | Flips a checkbox |
| `set-agents = [{ name = "Juniper", hamlet = "Pear Tree Hollow" }]` | Replaces the agents a plugin that may read them sees |
| `grant = "read.agents"` | The chief allows a capability the manifest asks for |
| `deny = "read.agents"` | The chief refuses it |
| `render = { panel = "main", file = "after.png" }` | Draws the panel in a dock, as the window does, to a PNG under `--out` |
| `expect = { panel = "main", text = "2 agents" }` | Fails unless a node of the panel shows the text |

`panel` is the id the plugin gave the panel (`main`), and `node` the id it gave
the node. A click on a node that is not there fails and lists the ids that are.
Every action runs three frames, so the plugin sees it, answers and has its
panels drawn again. A panel is asked for its tree again only after an event
reached it or the plugin asked for a redraw, so after `set-agents` click or
redraw before looking at the agents.

`plugin_check <plugin dir> [--script steps.toml] [--out dir] [--no-pictures]`
runs it. With no script it runs two frames and draws every panel. The exit code
is 0 when every step worked, 1 when one failed and 2 for bad arguments or a
folder that does not load. Pictures are drawn offscreen and need a GPU;
without one a `render` step fails with that reason, and `--no-pictures` leaves
them out.

### In Rust

`hamlet_harness::Harness`, behind the `wasm` feature, is what the script runs
on:

| Call | Does |
|---|---|
| `Harness::open(folder)` | Installs the plugin folder in a scratch home that goes with the harness, and brings it up |
| `sized(size)` | The size of the pictures, in pixels |
| `tick()` | Runs one frame |
| `click(panel, node)`, `type_text(panel, node, text)`, `toggle(panel, node)` | Do what a person does to a node; an error says what is not there |
| `set_agents(&[(hamlet, name)])` | Makes up the agents |
| `grant(capability)`, `deny(capability)` | Answer as the chief |
| `tree(panel)`, `outline(panel)` | The tree a panel shows, as nodes and as lines |
| `panels()`, `logs()`, `stopped()` | The panels the plugin declared, what it logged, why it stopped |
| `render(panel, path)` | Draws the panel to a PNG |

`hamlet_wasm`'s `fixture` feature makes test plugins from text, as the host's
own tests do.

## Limits

Every plugin is held to the limits of `assets/wasm.toml` whatever it is
allowed: the time and memory of a call, log lines, panels, the nodes, depth and
text of a tree, events a second and intents a call ([`wasm.md`](wasm.md)). A
call that traps, runs out of time or returns a bad tree stops the plugin; its
panels say so and have a RESTART button.

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
- Counting on state across a reload.
- A folder whose manifest `id` is not the folder's name.

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
