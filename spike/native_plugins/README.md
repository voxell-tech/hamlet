# Native plugins spike

Checks that a plugin built by its own `cargo` call can share the app's
Bevy as one Rust `dylib`. macOS only. The findings are in
`../hamlet_backlog/proposals/native-plugins.md`.

- `sdk/`: `hamlet_sdk`, the shared library, and `app`, which loads every
  library in `HAMLET_SPIKE_PLUGINS`. `app_static` is the same scene with
  Bevy linked in.
- `plugin/`: a plugin as its author keeps it, with its own workspace.
- `build_plugin.sh plugin build`: builds it as Hamlet would, in a build
  workspace laid out like `sdk/`.
- `package.sh`: lays out a release in `dist/`.
- `mismatch.sh [variant...]`: builds the plugin with one thing changed and
  reports whether it still loads.

```sh
(cd sdk && cargo build --release) && ./package.sh
./build_plugin.sh plugin build
mkdir -p /tmp/plugins && cp build/target/release/libpulse.dylib /tmp/plugins/
HAMLET_SPIKE_PLUGINS=/tmp/plugins HAMLET_SPIKE_FRAMES=120 dist/app
```
