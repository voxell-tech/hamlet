# Permissions

What an installed plugin may do beyond drawing its own panel. The model is
the one phones use: a manifest names the most a plugin may ask for, each ask
with a reason; the chief chooses for each; a call that is not allowed fails.

## The model

- The manifest's `[[asks]]` name a `capability`, a `reason` (required) and
  optionally a `scope`. Anything the manifest does not name is impossible for
  that plugin.
- Each named capability has one choice: **Allow**, **Ask next time** or
  **Don't allow**. A capability with no choice yet is unanswered and behaves
  as Ask next time.
- The verdict a host function checks is `Granted`, a component on the
  plugin's entity rebuilt from `Choices` and `Asks` whenever either is put
  in. A host function reads it on each call, so a change applies to the next
  call.

## Capabilities

`hamlet_sdk::capability::Capability`: `read.agents`, `read.chat`,
`read.activity`, `read.papers`, `chat.send`, `commands.run`, `ui.menu`, `net`,
`fs.pick`, `fs.folder`, `model.call` and `plugin.use`. Only `read.agents` is
checked by a host function so far; the others can be asked for, shown and
chosen.

## Installing

The scan reads every folder of `<home>/plugins/` (see
[`wasm.md`](wasm.md)). A plugin that `grants.toml` records as installed is
brought up. For any other, the host spawns an entity marked `Uninstalled`
that holds the place of its files and nothing else: its component is not
compiled and its panel is not read until the chief says yes.

- A plugin the chief was never asked about, or that changed since it was
  declined, raises one petition: "Install Roster 0.1.0?", with what it asks
  for, each capability with its reason. **Allow** installs it with every ask
  allowed and brings it up at once. **Don't allow** records it as declined, so
  it is not asked about again until its version or its asks change.
- A declined plugin is listed in Settings, and the chief can have the
  question put again (`AskToInstall`).
- An installed plugin whose new version asks for more is brought up with what
  it had; the new asks are unanswered, and one petition ("asks for more")
  lists them. Allow allows them, Don't allow denies them.

The petition is the one queue of Needs You: a `Waiting` of kind `Plugin`
(`hamlet_core::oversight`), raised with `oversight::petition`, which adds no
second queue. The chief's answer reaches `hamlet_sdk::capability` as the
`Settled` event, and a `Subject` on the petition says what it was about. A
petition that is not answered when the app quits is raised again at the next
start, from the same state.

## First use

Only a capability the manifest names can be used or asked for. A host
function that needs one, and `request`, look at the plugin's verdict:

| Verdict | A gated call | `request` |
|---|---|---|
| Allowed | works | `granted` |
| Denied | returns its error | `denied` |
| Ask (unanswered or Ask next time) | returns its error, and the chief is asked | `pending`, and the chief is asked |
| Not named in the manifest | returns its error | `denied`, and nobody is asked |

The petition is raised after the call, by the SDK's `Wanted` event. It reads
"Roster asks to use read.agents. Allow it?", with what the capability lets a
plugin do, the manifest's reason and, from `request`, the plugin's own reason
(cut to 200 characters). There is at most one open petition for each plugin
and capability, however often the plugin calls. A data plugin's `Query` goes
through the same gate.

The chief answers:

- **Allow** sets the choice to Allow.
- **Ask next time** allows the capability for the rest of this run and keeps
  the choice at Ask next time, so the first use after the next launch asks
  again. The allowance is not written to `grants.toml`.
- **Don't allow** sets the choice to Don't allow.

An answer to a petition that came from `request` reaches the plugin's inbox
as `granted(capability)` (Allow, or Ask next time) or `denied(capability)`.
A gated call that raised the petition gets no message: the plugin tries again.

In Settings, setting Ask next time on a capability takes back what was
allowed, for this run too: the next use asks.

## Settings

The Plugins page of Settings (`hamlet_ui/src/screens/settings/plugins.rs`
and its folder) shows it two ways, on two tabs:

- **By plugin.** Each plugin with its version and status, its switch, and a
  PERMISSIONS button that opens its detail: every capability its manifest
  asks for, what the capability lets it do, its reason, and a control of
  three buttons (Allow, Ask next time, Don't allow) with the current choice
  lit. A plugin that is not installed has an INSTALL button, which puts the
  install petition again.
- **By capability.** Each capability that some plugin asks for, with the
  plugins that ask for it, each with the same control.

A press sends `Decide`, which replaces the plugin's `Choices` and `Granted`
and writes `grants.toml` before the next frame, so the plugin's next call
sees it and a revoked capability returns its error at once. Each view reads
a small `PartialEq` resource that a system keeps (`Listed`, `Details`,
`Held`).

## Where the choices are kept

One file for every plugin, `<home>/plugins/grants.toml`, written by
`hamlet_sdk::capability::Store` whenever a choice changes:

```toml
[plugin.roster]
state = "installed"          # or "declined"
version = "0.1.0"            # the version the answers were given for
asks = ["read.agents"]       # the capabilities the chief was asked about
[plugin.roster.choices]
"read.agents" = "allow"      # allow, ask-next-time or deny
```

- The file is read when the app starts. A plugin in the folder that is not
  recorded as `installed` is not compiled or run.
- A manifest that asks for a capability not in `asks` (an update) leaves it
  unanswered. A capability the manifest no longer asks for is dropped from
  the record, and the version is brought up to date.
- A `declined` plugin stays declined while its version and asks are the same
  as the ones that were declined.
- A file that cannot be read installs nothing and is not written over.

`Decide` (an event of `hamlet_sdk::capability`) sets one choice, replaces the
plugin's `Choices` and writes the record.

## Never grantable

`NEVER` in `hamlet_sdk::capability`: `keys` (keys and model credentials),
`petitions.answer`, `home.raw`, `process.run`, `sandbox.off` and
`storage.other` (another plugin's storage). A manifest that asks for one, for a
name that is not a capability, or with no reason is turned down by the scan,
with the reason logged, and no petition is raised.
