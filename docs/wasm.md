# The WASM host

An installed plugin is a WASM component. `hamlet_wasm` loads the ones the
chief approved, runs each in a store of its own under limits, and calls it
once a frame with what happened. A plugin never sees Bevy or the world.
The built-in plugins of [`sdk.md`](sdk.md) are Rust in the binary and do
not pass through it.

The host is behind the `wasm` feature of the binary:
`cargo run --features wasm`. Without it the engine is not compiled.

## The crates

| Where | What it is |
|---|---|
| `crates/hamlet_wasm` | The host. The engine is wasmtime, pinned in the root `Cargo.toml`, and no other crate names a wasmtime type |
| `crates/hamlet_wasm/wit/hamlet-plugin.wit` | The interface, the one description of what a plugin imports and exports |
| `crates/hamlet_guest` | What a plugin is written with: the bindings, the `Plugin` trait and the `export!` macro |
| `guests/hello` | A plugin that logs on every tick. A workspace of its own, since it is built for `wasm32-wasip2` |
| `assets/wasm.toml` | The limits |

In `hamlet_wasm/src/`, a file for each part: `engine.rs` (the engine and
its ticker), `limits.rs`, `manifest.rs`, `install.rs`, `approved.rs`,
`interface.rs` (the generated host side of the WIT), `host.rs` (the linker
and what a plugin may call), `instance.rs`, `call.rs`, `fault.rs`, and the
Bevy side: `load.rs`, `inbox.rs`, `run.rs`. `lib.rs` has the plugin.

## The interface

```wit
package hamlet:plugin@0.1.0;

interface host {
  enum level { info, warn, error }
  log: func(level: level, message: string);
}

interface plugin {
  variant incoming { tick(u64) }
  variant intent { log-line(string) }
  record outgoing { intents: list<intent> }
  entry: func();
  step: func(inbox: list<incoming>) -> outgoing;
}
```

`interface::VERSION` is the package version. A manifest that names another
one is turned down. `entry` runs once when the plugin loads. `step` runs for
each frame in which the plugin's `Inbox` holds something, and the intents it
returns are carried out by the host.

## The frame

A plugin is an entity with `PluginInfo` (`builtin: false`), the SDK's
`Manifest`, `Status`, `Instance` and `Inbox`. `Instance` holds the store:
a wasmtime `Store` is `Send` and `Sync` when its data is, so it is a
component and needs no resource of its own.

The `Update` sets `PluginSet::{Collect, Step, Apply}` run in that order.

1. `Collect` puts a `tick` in the inbox of every running plugin.
2. `Step` runs only while some inbox holds something. It calls `step` once
   for each running plugin that has mail. A call that fails leaves the plugin
   `Status::Failed` and the host running.
3. `Apply` runs when a plugin was stepped and carries out its intents: a
   `log-line` is written to the app's log, labelled with the plugin's id.

What a plugin passes to `log` is written to the log the same way, labelled
with the id. A headless run (`--headless`) installs the log only with the
`wasm` feature on.

## Limits

The values are in `assets/wasm.toml`, and `Limits` in `limits.rs` says what
each means. They apply to every plugin alike, whatever it is granted.

- **Time.** The engine has epoch interruption, and a thread advances the
  epoch every `tick_ms`. A call, and the instantiation, may last `call_ms`
  worth of ticks; past it the call traps as `Fault::OutOfTime`.
- **Memory and tables.** A `ResourceLimiter` refuses a growth past
  `memory_mib` or `table_elements`, which traps the call as
  `Fault::OutOfMemory`. The pooling allocator is sized to the same memory.
- **Log lines.** A call keeps at most `log_lines` lines of `log_bytes` each.

A saved change to the file reaches `Limits` while the app runs, and the next
call takes the new `call_ms`. The engine keeps the tick and the pool it was
built with, and a plugin keeps the memory, table and log limits it was loaded
with.

## What a plugin can import

The linker defines the `hamlet:plugin/host` interface and nothing else. A
plugin written in Rust with `std` imports the `wasi:` interfaces of its
standard library; each of those is defined as a function that traps when
called, so such a plugin loads, and one that prints or reads the clock is
failed. Any import that is neither `hamlet:plugin/host` nor `wasi:` fails
the load with the name of the import.

## Installing a plugin

```
<home>/plugins/
  approved.toml        plugins = ["hello"]
  hello/
    manifest.toml
    plugin.wasm
```

`manifest.toml` has `id`, `name`, `version` and `interface`, and may have
`needs` (an `id` and a `range` each), `exports`, `imports` and `asks` (a
`capability` and a `reason` each). The scan turns a folder down when a file
is missing, the manifest cannot be read, its `id` is not the folder's name or
its `interface` is not `interface::VERSION`; the reason is logged.

`approved.toml` lists the ids that may load. A plugin that is installed and
not listed is never compiled, and a missing or unreadable list approves
nothing. The file is how the chief's answer is kept until the petition
exists.

## Building a plugin

A plugin is a `cdylib` for `wasm32-wasip2` that depends on `hamlet_guest`:

```rust
use hamlet_guest::{Incoming, Intent, Level, Plugin};

struct Hello;

impl Plugin for Hello {
    fn entry() {
        hamlet_guest::log(Level::Info, "hello");
    }

    fn step(inbox: Vec<Incoming>) -> Vec<Intent> {
        Vec::new()
    }
}

hamlet_guest::export!(Hello);
```

To build and install the example into a home:

```sh
rustup target add wasm32-wasip2
HAMLET_HOME=/tmp/hamlet-try guests/hello/build.sh
echo 'plugins = ["hello"]' > /tmp/hamlet-try/plugins/approved.toml
HAMLET_HOME=/tmp/hamlet-try cargo run --features wasm -- --headless
```

The default `cargo build`, `clippy` and `test` need neither the target nor
the network.

## Tests and the bench

The tests make their plugins from core modules in text and the WIT file, so
they need no target. `cargo run --release -p hamlet_wasm --example bench --
<plugin.wasm>` times the compile, the `.cwasm`, an instantiation and a
`step` of a built plugin.
