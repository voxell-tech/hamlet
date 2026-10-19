# Native plugins

A native plugin is a dynamic library (`.dylib`, `.so` or `.dll`) that the
host opens when it starts. It is Rust, built with the same compiler and the
same engine as the Hamlet that loads it, and it runs as Hamlet: in the same
process, with the same rights, with no sandbox and no capability checked.
It is the tier for what a WASM plugin cannot do: add Bevy systems,
observers and resources, and reach anything the app reaches. The price is
trust, which is why the chief is asked for it by name.

The plugins compiled into the app (`plugins/`) do not leave the build, and
the WASM plugins stay in their sandbox. Native is a third kind, beside them.

| | Built in | WASM | Native |
|---|---|---|---|
| Where the code is | Linked into the app | `plugin.wasm` in the home | A library in the home |
| What limits it | Nothing | The interface, the grants, the fuel | Nothing |
| What the chief answers | Nothing | A list of capabilities, one by one | "Full access", once |
| Removed while running | Switched off | Switched off, unloaded | Switched off; its code stays until restart |

`hamlet_native` (`crates/hamlet_native`) holds both halves: the contract a
library keeps, and the host that loads one (behind its `host` feature, on by
default; a library turns it off).

## The folder

```
<home>/plugins/<id>/
  manifest.toml
  lib<name>.dylib      (libname.so, name.dll)
```

```toml
id = "spark"
name = "Spark"
version = "0.1.0"
kind = "native"
library = "libnative_spark.dylib"
```

The id is checked as a WASM plugin's is: a name, not a reserved one, and the
folder's own. `library` is a file name in that folder, with the extension of
the system: one plain name, with no folder, drive, `..` or backslash. A
library that is a link is refused. `kind = "native"` is what sends a folder here; the WASM host
leaves it alone. A manifest names no `[[asks]]`: there is nothing to ask for
when everything is allowed.

## What the chief is asked

A native plugin is installed as any plugin is, with a petition in Needs You
and an INSTALL button on the Plugins page. The question is not a list of
capabilities:

> Install Spark 0.1.0?
> FULL ACCESS: this plugin runs as Hamlet with no limits.

Nothing of the library is opened until the chief says yes: opening a library
runs its code. The answer is kept in `plugins/grants.toml` with the hash of
the library file (SHA-256). A library whose hash is not the one answered for
is asked about again, and is not opened meanwhile; so are a new library put
in the folder and a plugin that was declined and changed. The file that is
opened is a private copy in `native-cache/<id>/<hash>.<ext>` of the home,
made after the answer and hashed again, so a file swapped after the answer
is not opened. Removing the plugin deletes its copies. A plugin is never
updated by itself. There is no signing: what the chief trusts is the
"full access" answer and the file it was given for. Whoever can write to the
home can replace a file, and with it can do anything the app can.

The Plugins page lists it as **NATIVE** with a warning line under its name,
and it has no PERMISSIONS button. It can be switched off and on (its panels,
commands, menu lines, keys, context items, inspector tabs and settings pages
come and go, as for any plugin) and uninstalled.

## What cannot be undone while the app runs

- A library is never unloaded. Its code stays in the process until the
  process ends. Removing a plugin deletes its folder and its panels, and
  takes effect fully when Hamlet starts again (on Windows a loaded library
  cannot be deleted, so the file stays until then).
- Switching a plugin off removes what it added through the SDK, and nothing
  else. Systems, observers and resources it added to the app keep running
  until restart.
- A new version of a library takes effect on restart, and is asked about
  first if its hash changed.

The Plugins page says so under every native plugin.

## The gate

Rust has no stable ABI, so the host calls a library's Rust code only when
the library was built exactly as the host was. A library exports four
symbols (the `hamlet_native::export!` macro writes them):

| Symbol | Type | What |
|---|---|---|
| `hamlet_native_abi_version` | `extern "C" fn() -> u32` | The version of this contract. Read first; a library of another one is turned down before anything else of it is read |
| `hamlet_native_abi_size` | `extern "C" fn() -> u64` | The layout of `Abi`: its size in the high 32 bits, and a hash of its alignment and the offset of each field in the low. Read second; a library whose `Abi` is laid out otherwise, or that lacks the symbol, is turned down before its `Abi` is read, and the refusal names the symbol |
| `hamlet_native_abi` | `extern "C" fn() -> Abi` | What the library was built with, below |
| `hamlet_native_entry` | `fn(&mut App) -> bool` | Called once, with the app; false when the entry panicked |

The contract version stays at 1 until Hamlet 0.1.0 is published. Until
then a change to `Abi` is caught by its layout, not by a new version.

`Abi` is `repr(C)` and holds, in this order:

1. the contract version;
2. the Hamlet version (`CARGO_PKG_VERSION` of `hamlet_native`);
3. the compiler (`rustc -vV`, on one line), the one `rust-toolchain.toml`
   pins;
4. the profile (`debug` or `release`);
5. a fingerprint of the engine crates: a hash of the `Cargo.lock` entries of
   `bevy`, `bevy_internal`, `bevy_app`, `bevy_ecs`, `bevy_fynix` and
   `hamlet_sdk`, with their versions, sources, checksums and dependency
   lists (so a different set of features changes it). `build.rs` finds the
   lock above the target folder, or the file `HAMLET_LOCK` names, and fails
   the build when there is none; a lock with none of those packages is
   hashed whole;
6. a hash of the sources of `hamlet_native`, `hamlet_sdk` and the path
   crates under the SDK, `hamlet_base`, `hamlet_home`, `hamlet_assets`,
   and `bevy_fynix`, `fynix` and `fynix_macros` (in `vendor/fynix`): each
   crate's `Cargo.toml` and every file under its `src`, by its path in the
   crate and its bytes, a crate after the other in that order and the files
   of each in the order of their paths. An edit of any of them, a comment
   or a feature too, changes it, so a library built against the SDK before
   the edit is refused (`SDK sources`);
7. a hash of the identities (`TypeId`) of `App`, `World`, the SDK's `Entry`
   and `Declared`.

The host reads the library's values and compares them with its own in that
order, and refuses at the first difference, saying which field, the host's
value and the library's, in the plugin's line on the Plugins page and in the
log. The last field is what the others cannot see: a crate built with other
features or other settings is a different type to the host, whatever it
looks like. Sources the gate does not hash (the engine's, or another crate
of the checkout) are not seen when they change under the same lock. The
gate has no way of knowing how a library was built beyond these, and a
library that lies about them is a library the chief gave full access.

`hamlet --native-abi` prints the host's values, as TOML.

Opening the file (`dlopen`) runs the library's initializers before any check
is possible; that is why only a library the chief let in is opened.

## Building one so that it passes

A type is the same type to the host and to the library only when both use
the same compiled crate. The compiler names a crate by its name and the
metadata cargo hashes for it: where the package comes from, its version,
its features, the profile, the compiler, and the same of each dependency.
Where a path package comes from is its path from the workspace's root, or
its whole path outside the workspace, so the same source built in another
workspace is another crate. A plugin built in a folder of its own, with
path dependencies on this checkout and a copy of its `Cargo.lock`, passes
the fingerprint and is refused for its engine types. A build with fewer or
more features of any crate under the engine is refused the same way.

There are two builds that pass: in the checkout, and outside it with an SDK
kit. The compiler is the one `rust-toolchain.toml` pins in both.

### In the checkout

- `scripts/new-native-plugin.sh <name>` makes `native/<name>` from
  `templates/native-plugin`. It is a member of the workspace, and its
  dependencies are the workspace's own.
- `native/<name>/build.sh [folder] [release]` builds the workspace with
  `cargo build --workspace` and puts the manifest and the library in the
  folder (or `$HAMLET_HOME/plugins/<name>`). A debug host loads a debug
  library; a release host a release one.
- `scripts/build-native.sh <home> [release]` does the same for the sample in
  `native/spark/`.

The whole workspace is built, not the plugin alone, because the features of
a crate are what everything built together asks for: a build of the plugin
alone enables fewer, and the gate says so. The host is the app built by
`cargo build` or `cargo run`, which asks for the same.

`native/` is in the workspace but not in the default members, so
`cargo build`, `cargo test` and `cargo clippy` leave its libraries alone.

### Outside, with an SDK kit

A kit is the compiled crates of one build of Hamlet. The plugin is compiled
against them, so it links the very engine and SDK that build has, and
nothing of the engine is compiled again.

Getting one:

- For a release of Hamlet,
  `scripts/fetch-native-sdk.sh <version> <target> <kit>` downloads the kit
  published with it (`hamlet-sdk-<version>-<target>.tar.gz`, see
  [`releases.md`](releases.md)) and unpacks it into `<kit>`.
  The target is the triple of the Hamlet the plugin is for:
  `aarch64-apple-darwin`, `x86_64-pc-windows-msvc` or
  `x86_64-unknown-linux-gnu`.
- For a Hamlet built from a checkout,
  `scripts/make-native-sdk.sh <kit> [cargo build options]` runs the build
  of the Hamlet that will load the plugins, with its options (none for `cargo build`, `--release` for
  `cargo build --release`), and makes the kit from it. The release is made
  with `--release --locked -p hamlet --target <target>`.

A kit holds:

- `deps/`: the libraries of the engine, the SDK and the contract and of
  everything they depend on in that build, and the procedural macros among
  them (what a macro depends on is inside its own library). The rest of
  the app is left out: `cargo tree` of the same packages, features and
  target says what they depend on;
- `externs`: the crates a plugin names and their files: `bevy`, each
  `bevy_*` (the engine's derives write paths such as `bevy_ecs::`),
  `hamlet_sdk` and `hamlet_native`;
- `sdk.toml`: the toolchain, the target, and the values the gate compares,
  as `hamlet --native-abi` of that build prints them.

Building with one:

- `scripts/new-native-plugin.sh <name> <folder>` makes the plugin in that
  folder, a workspace of its own with no dependencies (the files of
  `templates/native-plugin/outside` replace their namesakes).
- `HAMLET_SDK=<kit> ./build.sh [folder] [release]` runs
  `cargo +<toolchain> rustc --lib --target <target>` with
  `-L dependency=<kit>/deps` and an `--extern` for each line of `externs`,
  and puts the manifest and the library in the folder. It takes seconds.
  The plugin is built again from nothing when the kit's `sdk.toml` is not
  the one it was last built with, as cargo does not see the kit's files.

What must match:

- The kit is of the build that loads the plugin. Make it again whenever
  that Hamlet is built again; a kit of another build is refused by the gate
  (a debug kit's plugin in a release Hamlet: its profile differs). Compare
  the kit's `sdk.toml` with `hamlet --native-abi` of the Hamlet to see
  which.
- The compiler is the kit's. Another one refuses the kit's crates when the
  plugin is compiled (`E0514`, compiled by an incompatible version of
  rustc).
- The plugin depends on neither the engine nor the SDK through cargo. A
  crate of its own from a registry is another crate than any in the kit:
  fine inside the plugin, never for a value it hands to Hamlet.

What the plugin says to the gate is the kit's own `hamlet_native`, so its
profile is the kit's whatever the plugin was built with. A kit made before
an edit of the SDK, for a Hamlet built again with the same lock, features
and compiler, makes plugins the gate refuses for their `SDK sources`.
Editors do not see the kit's crates, as the plugin's `Cargo.toml` names
none. On macOS a release kit is 1.1 GB (1.3 GB with the libraries of the
whole app), and about 0.3 GB as the release's archive.

## Its own copy of the engine

A library is a `cdylib`: it holds its own copy of every crate it uses,
`std` and Bevy among them. The gate makes the types the same on both
sides; the statics are not, as each copy has its own:

- the task pools: a library that reaches them through `ComputeTaskPool`,
  `AsyncComputeTaskPool` or `IoTaskPool` finds its copy's, which nothing
  set up, while the app's systems it adds are run by the host's;
- the tracing dispatcher: the library's `info!` and `warn!` go to its own,
  which nothing set, so its log lines are lost unless it sets one;
- the panic hook and the panic runtime: a panic is caught on the side
  that raised it or not at all (that is why `export!` catches the
  entry's), and the host's hook does not see the library's;
- the interners: a label with fields interned on one side is another label
  to the other. A label with none, such as `Update` or the render app's,
  is the same on both.

Memory crosses in both directions (a `Box` of a system, a `String` in a
resource), and is freed by the other side's code. Both sides must use the
`System` allocator, which is Rust's own when no `#[global_allocator]` is
set: Hamlet sets none, and a native plugin must set none either.

Building Bevy as one shared library that both sides link against would
give one copy of each static. Hamlet does not offer it: that library would
have to ship with every Hamlet and be the very file each plugin was linked
against, Rust's `dylib` has no stable ABI either, so it adds a file to
match without removing a check, and the app would no longer be one
binary.

## Writing one

```rust
hamlet_native::export!(entry);

fn entry(app: &mut hamlet_native::App) {
    app.init_resource::<Frames>().add_systems(Update, count);
    hamlet_native::declare(app, declare);
}

fn declare(entry: &mut hamlet_sdk::plugin::Entry) {
    entry.panel("spark", "SPARK", Glyph::Book, panel);
}
```

- `entry` gets the real `App` while it is built, before it runs: the
  native host is the last plugin the binary adds, and it opens the
  libraries `grants.toml` records as installed (for the library that is
  there) from its `build`. Resources, systems, observers and plugins added
  there are the app's; the plugins are in its registry, so their `finish`
  and `cleanup` run, and the sub-apps (the render app) are there to reach.
  `Startup` has not run yet. So the levels of drawing past what the host
  draws for a WASM plugin (a render pass, a compute shader, a texture or a
  renderer of its own; see Drawing in [`wasm.md`](wasm.md)) are a native
  plugin's (see Render passes and compute below).
- A plugin the chief installs while Hamlet runs is loaded at once, into an
  `App` of its own that holds the running world: what it adds runs, but
  that app has none of the app's plugins or sub-apps, is never finished,
  and its `Startup` systems never run. Its line on the Plugins page says it
  gets the full app when Hamlet starts again, which is when the next start
  loads it as above.
- `hamlet_native::declare` hands the host the function that adds what goes
  through the SDK's `Entry`: panels, commands, menu lines, keys, context
  items, inspector sections, settings pages, parts of the window's frame.
  Those carry `ContributedBy`
  as everything a plugin adds does, so switching the plugin off despawns
  them and switching it on runs the function again. Anything the entry adds
  to the app directly is not tracked and stays until restart.
- A panic in the entry is caught inside the library (`export!` does it),
  and the plugin is marked failed and not loaded. What the entry added
  before the panic stays and runs until Hamlet restarts; the Plugins page
  and the chat say so. Its record in `grants.toml` is marked `failed`, so
  the next start does not open it and asks the chief to install it again.
  A panic anywhere else in the library is foreign to Hamlet's runtime and
  aborts the process (see below).
- `native/spark/` is the sample: a resource that counts frames and a panel
  that shows the count.

## Render passes and compute

An installed WASM plugin may have the host build a scene and paint it with a
checked shader (`render.scene`, `render.shader`), and no more: a manifest that
asks for `render.compute` is turned down ([`permissions.md`](permissions.md)).
A native plugin needs no capability for it, since its entry has the whole
`App`:

- a material of its own is a Bevy `Material` with its shader, added with
  `MaterialPlugin` in the entry, as `hamlet_vfx` adds its own;
- a compute shader or a render pass is set up on the render sub-app,
  `app.sub_app_mut(RenderApp)`: its pipelines in the render world, its
  systems in the `Render` schedule and its node in the render graph, as any
  Bevy plugin does;
- what it draws in the world uses the realm's own camera and entities, which
  it reaches as the app does.

Such work is done only when Hamlet starts with the library loaded: a plugin
installed while Hamlet runs gets an `App` with no sub-apps until the next
start (see Writing one above). Nothing checks or limits what it puts on the
GPU, and the chief's full-access answer is all that lets it.

## When the app stops while a library is new

Before it opens a library, the host writes the file `loading` in the home,
with the plugin's id on a line. The id goes when the entry has returned and
the app has run `clean_frames` frames since (`[guard]` in
`assets/native.toml`, 60), because a library can also fail in the systems its
entry added. A `loading` found at the next start means the process died
before those plugins were trusted, and:

- that start is in **safe mode**: no native library is opened;
- each plugin named is listed with a note saying Hamlet stopped before it was
  trusted, and a removal petition for it is raised in Needs You (when
  several libraries load in one start, all of them are named, as any could
  be the one);
- the file stays, so each start is in safe mode, until the plugins are
  removed (which takes their lines) or the chief deletes the file `loading`
  by hand. A line that names a plugin no longer installed is dropped at
  start.

What the guard covers, from the moment the library is opened:

- its initializers and its entry;
- its systems, observers and anything else it added, for the first
  `clean_frames` frames of the app after the entry returned, a panicking
  entry's too (about two seconds headless, about one in the window);
- a crash of any kind: a panic in one of its systems aborts the process, as
  a signal ends it, and none of them lets the marker go.

An exit of the app by itself (the window closed, Quit, Ctrl-C headless)
before the clean frames trusts the libraries still waiting, as nothing
stopped it.

A library that crashes after its clean frames is the app's code, and the
marker does not see it. A count does: the file `unclean` in the home holds
how many runs in a row opened a library and did not exit by themselves. A
run adds one before it opens its first library, and an exit by itself
removes the file. When a start finds `unclean_starts` (`[guard]`, 2) or
more:

- that start is in safe mode: no native library is opened;
- each plugin is listed with a note saying why, and the chief is told in
  the chat;
- an exit of that run by itself removes the file, so the next start opens
  the libraries again. The one at fault is removed from the Plugins page
  in between.

The chief can also start Hamlet with **`--safe-mode`** to open no native
library in that run. The plugins are listed with a note saying so. Holding
a key at launch does not enter safe mode: the window's events start only
once the app runs, after the libraries are opened, and reading the
keyboard before that takes a call of each system.

Both files are written whole: to a file beside them, synced, then renamed
over them.

## Where it is in the code

| What | Where |
|---|---|
| The contract: `Abi`, `export!`, `declare`, the type hash | `hamlet_native/src/abi.rs`, `build.rs` |
| The hash of the SDK's sources | `hamlet_native/src/sources.rs`, run by `build.rs` |
| Comparing two builds | `hamlet_native/src/gate.rs` |
| The compiler, and the kit a plugin outside the checkout builds with | `rust-toolchain.toml`, `scripts/make-native-sdk.sh`, `scripts/fetch-native-sdk.sh`, `templates/native-plugin/outside/`, `.github/workflows/release.yml` |
| Opening a library, and the private copy it is opened from | `hamlet_native/src/library.rs`, `cache.rs` |
| The folder and its manifest | `hamlet_native/src/install.rs`, `manifest.rs` |
| Opening the granted libraries while the app is built, the scan at start, safe mode, loading the plugin just let in | `hamlet_native/src/load.rs`, `marker.rs`, `guard.rs`, `assets/native.toml` |
| Removing | `hamlet_native/src/remove.rs` |
| What the SDK knows of a native plugin: `FullAccess`, `NotLoaded`, `Declared`, the hash in a record, the petition text | `hamlet_sdk/src/plugin/access.rs`, `capability/record.rs`, `petition.rs` |
| The NATIVE line on the Plugins page | `plugins/settings/src/plugins/list.rs` |

The binary's `native` feature (on by default) adds `hamlet_native::plugin`,
after every other plugin; `cargo run --no-default-features` never opens a
library, nor does a run with `--safe-mode`.

The test that builds the sample and loads it is ignored by default:
`cargo test -p hamlet_native --test sample -- --ignored`.
