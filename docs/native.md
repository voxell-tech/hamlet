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
the library was built exactly as the host was. A library exports three
symbols (the `hamlet_native::export!` macro writes them):

| Symbol | Type | What |
|---|---|---|
| `hamlet_native_abi_version` | `extern "C" fn() -> u32` | The version of this contract. Read first; a library of another one is turned down before anything else of it is read |
| `hamlet_native_abi` | `extern "C" fn() -> Abi` | What the library was built with, below |
| `hamlet_native_entry` | `fn(&mut App)` | Called once, with the app |

`Abi` is `repr(C)` and holds, in this order:

1. the contract version;
2. the Hamlet version (`CARGO_PKG_VERSION` of `hamlet_native`);
3. the compiler (`rustc -vV`, on one line);
4. the profile (`debug` or `release`);
5. a fingerprint of the engine crates: a hash of the `Cargo.lock` entries of
   `bevy`, `bevy_internal`, `bevy_app`, `bevy_ecs`, `bevy_fynix` and
   `hamlet_sdk`, with their versions, sources, checksums and dependency
   lists (so a different set of features changes it). `build.rs` finds the
   lock above the target folder, or the file `HAMLET_LOCK` names, and fails
   the build when there is none; a lock with none of those packages is
   hashed whole;
6. a hash of the identities (`TypeId`) of `App`, `World`, the SDK's `Entry`
   and `Declared`.

The host reads the library's values and compares them with its own in that
order, and refuses at the first difference, saying which field, the host's
value and the library's, in the plugin's line on the Plugins page and in the
log. The last field is what the others cannot see: a crate built from other
sources, with other features or other settings is a different type to the
host, whatever it looks like. The gate has no way of knowing how a library
was built beyond these, and a library that lies about them is a library the
chief gave full access.

Opening the file (`dlopen`) runs the library's initializers before any check
is possible; that is why only a library the chief let in is opened.

## Building one so that it passes

The library has to be built **in the checkout's workspace**, with the
toolchain and the settings the host was built with:

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
alone enables fewer, the crates are then not the host's crates, and the
gate says so. The host is the app built by `cargo build` or `cargo run`,
which asks for the same. Cargo gives a crate outside the workspace a
different identity from the same crate inside it, so a library built in a
workspace of its own is refused too. (A crate published to a registry has
the same identity everywhere, and that is how a library built outside the
checkout would pass; Hamlet's crates are built from the checkout.)

`native/` is in the workspace but not in the default members, so
`cargo build`, `cargo test` and `cargo clippy` leave its libraries alone.

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

- `entry` gets the real app's world: resources, systems, observers and
  plugins added there are the app's. It runs inside `Startup` (or when the
  chief installs the plugin), so add systems to `Update` or later, not to
  `Startup`, whose schedule is being run.
- `hamlet_native::declare` hands the host the function that adds what goes
  through the SDK's `Entry`: panels, commands, menu lines, keys, context
  items, inspector sections, settings pages. Those carry `ContributedBy`
  as everything a plugin adds does, so switching the plugin off despawns
  them and switching it on runs the function again. Anything the entry adds
  to the app directly is not tracked and stays until restart.
- A panic in the entry is caught; the plugin is marked failed with the
  reason. A crash is not (see below).
- `native/spark/` is the sample: a resource that counts frames and a panel
  that shows the count.

## When the app stops while a library loads

Before it opens a library, the host writes the file `loading` in the home,
holding the plugin's id, and removes it when the entry returns. A `loading`
found at the next start means the process died in that library, and:

- that start is in **safe mode**: no native library is opened;
- the plugin is listed with a note saying Hamlet stopped while it loaded,
  and a removal petition for it is raised in Needs You;
- the file stays, so each start is in safe mode, until the plugin is removed
  (which deletes it) or the chief deletes the file `loading` by hand. A
  `loading` that names a plugin no longer installed is deleted at start.

A library that crashes later, after its entry returned, is not guarded: it
is the app's code.

## Where it is in the code

| What | Where |
|---|---|
| The contract: `Abi`, `export!`, `declare`, the type hash | `hamlet_native/src/abi.rs`, `build.rs` |
| Comparing two builds | `hamlet_native/src/gate.rs` |
| Opening a library, and the private copy it is opened from | `hamlet_native/src/library.rs`, `cache.rs` |
| The folder and its manifest | `hamlet_native/src/install.rs`, `manifest.rs` |
| The scan at start, safe mode, loading the plugin just let in | `hamlet_native/src/load.rs`, `marker.rs` |
| Removing | `hamlet_native/src/remove.rs` |
| What the SDK knows of a native plugin: `FullAccess`, `NotLoaded`, `Declared`, the hash in a record, the petition text | `hamlet_sdk/src/plugin/access.rs`, `capability/record.rs`, `petition.rs` |
| The NATIVE line on the Plugins page | `plugins/settings/src/plugins/list.rs` |

The binary's `native` feature (on by default) adds `hamlet_native::plugin`;
`cargo run --no-default-features` never opens a library.

The test that builds the sample and loads it is ignored by default:
`cargo test -p hamlet_native --test sample -- --ignored`.
