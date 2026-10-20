# The engine

`engine/` holds `hamlet_engine`, the core that runs plugins and gives
them services. It stands apart from the app in `crates/`: no engine crate
depends on a crate there, and the vendored fynix is the only code they
share.

## The crates

Each depends only on those above it.

| Folder | Crate | What it is |
|---|---|---|
| `engine/api` | `hamlet_engine_api` | The names the engine and plugins share. serde only, no Bevy |

## The vocabulary

`hamlet_engine_api` is plain data, so a plugin and the engine build the
same types.

- **Ids.** A name is lowercase words of letters, digits and `_`, joined
  by `.`: `hamlet.demo`. A plugin's id is a name. What a plugin adds is
  named within it, and its full id joins the two with `/`:
  `hamlet.demo/notes`. The owner `engine` is the engine's own and no
  plugin may take it.
- **The manifest** (`plugin.toml`): id, name, version, the API version it
  was built against (`API_VERSION`, 0.1.0), and its asks. `check` refuses
  one built against another API, with an empty or unreadable text, or
  asking for the same thing twice.
- **Capabilities.** An ask names a capability, its target and a reason in
  the plugin's words: `net.fetch` a host, `assets.read` a type name,
  `services.call` another plugin's service. Powers that are never granted
  have no name, so no manifest can ask for them.
- **Assets.** An `AssetId` the store gives, and a `TypeName` such as
  `weather.forecast`. The engine never reads inside an asset.
- **The widget tree** a plugin describes a panel with: text, heading,
  icon, button, toggle, field, list, row, column, scroll, divider. It is
  flat, in pre-order, each node with its child count and an id the plugin
  keeps from one tree to the next. `check` refuses counts that do not
  make one tree, a leaf with children, two nodes of one id, and a tree
  past its `TreeLimits`.
- **The wire format** is postcard, through `encode` and `decode`.
  `decode` refuses bytes left over after the value.
