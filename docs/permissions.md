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
`read.activity`, `read.papers`, `chat.send`, `commands.run`, `ui.menu`, `ui.keys`,
`net`, `fs.pick`, `fs.folder`, `model.call`, `plugin.use`, `collections.read` and
`collections.write`. `read.agents`, `read.papers`, `ui.menu`, `ui.keys` and the
three below are checked so far; the others can be asked for, shown and chosen.

`plugin.use`, `collections.read` and `collections.write` are **scoped**: the
chief chooses for each target, not once for all. The target of `plugin.use` is a
plugin id, and of the other two a collection as `plugin/name`. They are what the
manifest's `imports` name, so Settings has a control for each plugin or
collection a plugin imports. The ask may carry a `scope` that lists the targets
it can be chosen for (`scope = ["notes"]`); a target outside it is refused. An
ask with no scope is limited only by the imports. The chief's choice for the
capability as a whole stays at Ask next time, or is Don't allow, which refuses
every target.

- `plugin.use` lets a plugin run a command of another plugin, hear its events
  and have them given to it (see [`wasm.md`](wasm.md)). The first use of a
  plugin it has no choice for raises the petition, "Clipper asks to use
  plugin.use for `notes`. Allow it?", and the call is answered with the error
  `plugin.use not granted`; the answer reaches the plugin as `granted` or
  `denied` of `plugin.use:notes`. An event is given to a plugin only while it
  holds `plugin.use` for the plugin that publishes it, and the events of the
  host need `read.agents` instead.
- `collections.read` lets a plugin read a collection another plugin shares, and
  `collections.write` change it, which the sharing plugin must also say it is
  `writable`.

- `ui.menu` lets a plugin's menu lines (`menu-item`) show, and `ui.keys` its
  shortcuts (`key`). They are not checked at the call but where the lines and keys
  are read: the plugin declares them in `entry`, they are held, and they take effect
  only while the choice is Allow. The first declaration while the chief is yet to
  decide raises the petition ("Notes asks to use ui.menu. Allow it?"); Don't allow
  shows nothing; setting it to Ask next time or Don't allow in Settings takes the
  lines and keys off at once, and Allow brings them back with no restart. A built-in
  plugin needs neither. Two plugins that bind the same chord both lose it, and a
  chord a built-in shortcut uses is never the plugin's; Settings says so under the
  plugin.

`ui.keys` is not `keys`. `keys` in the list of what is never granted below means
keys and model credentials; `ui.keys` is the keyboard shortcuts of a plugin.

`read.papers` lets a plugin read the selected agent's notes, its character and
the names of its skills (`papers()`, or `Query("papers")` of a panel), as they
stand when the plugin is called, as a copy. They are empty while no agent is
selected. It is gated as `read.agents` is.

Storage needs no capability: every plugin may keep a capped store of its own
(see [`wasm.md`](wasm.md)), and no capability reaches another plugin's, which is
`storage.other` of the never-granted ones.

## Installing

The scan reads every folder of `<home>/plugins/` (see
[`wasm.md`](wasm.md)). A plugin that `grants.toml` records as installed is
brought up. For any other, the host spawns an entity marked `Uninstalled`
that holds the place of its files and nothing else: its component is not
compiled and its panel is not read until the chief says yes.

- A plugin the chief was never asked about, or that changed since it was
  declined, raises one petition: "Install Roster 0.1.0?", with what it asks
  for, each capability with its reason, and the plugins it needs that are not
  installed. **Allow** installs it with every ask allowed, except that a
  scoped capability is left for its first use to be asked for each target, and
  brings it up at once if the plugins it needs are running. **Don't allow**
  records it as declined, so
  it is not asked about again until its version or its asks change.
- A declined plugin is listed in Settings, and the chief can have the
  question put again (`AskToInstall`).
- An installed plugin whose new version asks for more is brought up with what
  it had; the new asks are unanswered, and one petition ("asks for more")
  lists them. Allow allows them, Don't allow denies them.
- A version that asks for more and arrives while the app runs (a reload, see
  [`wasm.md`](wasm.md)) is not built until the petition is answered, so none of
  it runs before.

The petition is the one queue of Needs You: a `Waiting` of kind `Plugin`
(`hamlet_core::oversight`), raised with `oversight::petition`, which adds no
second queue. The chief's answer reaches `hamlet_sdk::capability` as the
`Settled` event, and a `Subject` on the petition says what it was about. A
petition that is not answered when the app quits is raised again at the next
start, from the same state.

## Removing

The UNINSTALL button of a plugin on the Plugins page of Settings (any plugin that
is not built in) sends `AskToRemove`, and one petition is raised: "Remove
Notes?". The chief answers:

The petition names the plugins that need the plugin and go off with it. A
petition to install lists the plugins it needs that are not installed.

- **Remove, keep data** removes the plugin and leaves its `data/` folder, the
  storage it kept (see [`wasm.md`](wasm.md)).
- **Remove with data** removes the plugin's folder whole.
- **Cancel** does nothing.

A removal sends `Removed`. `hamlet_wasm::uninstall` despawns the plugin, so its
panels go, deletes its files, and forgets its record in `grants.toml` so that it
is not asked about again. A folder that holds nothing but `data/` is not read as
a plugin at the next start, and says nothing. Putting the plugin back in
`plugins/<id>/` is an install like any other, with its petition and its asks to
answer again, and it finds the data it kept, since a plugin's storage is only
that folder.

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
and capability (and target, for a scoped one), however often the plugin calls. A data plugin's `Query` goes
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

- **By plugin.** Each plugin with its version and status, its switch, the
  plugins it needs and the plugins that need it, why it is held back where it
  is, and a PERMISSIONS button that opens its detail: every capability its
  manifest asks for, what the capability lets it do, its reason, and a control
  of three buttons (Allow, Ask next time, Don't allow) with the current choice
  lit. For a scoped capability there is a control for each plugin or collection
  the plugin imports, under its name. A plugin that is not installed has an
  INSTALL button, which puts the install petition again. A plugin that is not
  built in has an UNINSTALL button, which puts the removal petition (see
  Removing). Under its name the page lists the commands, menu lines and shortcuts
  the plugin adds, with a note on those that wait for `ui.menu` or `ui.keys` and a
  line in the colour of an error on a chord that does not run.
- **By capability.** Each capability that some plugin asks for, with the
  plugins that ask for it, each with the same control, or one for each target
  of a scoped capability.

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
"plugin.use:notes" = "allow" # the choice for one target of a scoped capability
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

## Limits no grant changes

`assets/wasm.toml` bounds every plugin alike, whatever it is allowed: the time
and memory of a call, the log, the panels, the nodes, depth and text of a
tree, the events it is given in a second, the intents it returns from a
call, the calls it may have waiting and how deep they may go, and the size of
what crosses between plugins. What each does when passed is in
[`wasm.md`](wasm.md).

## Never grantable

`NEVER` in `hamlet_sdk::capability`: `keys` (keys and model credentials),
`petitions.answer`, `home.raw`, `process.run`, `sandbox.off` and
`storage.other` (another plugin's storage; a plugin shares part of its own
as a collection, which others read with `collections.read`). A manifest that asks for one, for a
name that is not a capability, or with no reason is turned down by the scan,
with the reason logged, and no petition is raised.
