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
`read.activity`, `read.papers`, `read.world`, `chat.send`, `commands.run`,
`world.act`, `ui.menu`, `ui.keys`, `ui.context`, `ui.inspector`,
`ui.settings`, `net`, `fs.pick`, `fs.folder`, `model.call`, `plugin.use`,
`collections.read`, `collections.write`, `files.read`, `files.write`, `db`,
`agent.tool`, `agent.hook`, `ui.canvas` and `world.overlay`. `read.agents`,
`read.papers`, `read.world`, `ui.menu`, `ui.keys`, `ui.context`,
`ui.inspector`, `ui.settings`, `ui.canvas`, `world.overlay`, `chat.send`,
`commands.run`, `world.act`, `net`, `fs.pick`,
`files.read`, `files.write`,
`db`, `agent.tool`, `agent.hook` and the three below are checked so far; the
others can be asked for, shown and chosen.

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

- `ui.context` lets a plugin's items of the menu of a thing in the world
  (`context-item`) show, `ui.inspector` its tabs of the inspector's page of an agent
  (`inspector-section`) and `ui.settings` its pages of the Settings
  (`settings-page`). They are gated as `ui.menu` is: held where they are read,
  asked for at the first declaration, taken off at once when the choice is taken
  back. The panel a tab or a page shows is the plugin's own and needs nothing, and
  neither does the `open-panel` intent that brings one of its own panels forward.

## Tools for the agents

`agent.tool` lets a plugin give the agents tools (`agent-tool`, see [`wasm.md`](wasm.md)).
It is gated as `ui.menu` is: the plugin declares its tools in `entry`, they are held, and
they are offered to the agents only while the choice is Allow. The first declaration while the
chief is yet to decide raises the petition; Don't allow offers nothing; taking the choice
back in Settings takes the tools off at once, and Settings lists them under the plugin
("Tools for the agents: height (waits for agent.tool)").

- The ask's `scope` lists the hamlets whose agents are offered the tools. None lists
  every hamlet, and the petition and Settings say "Limited to: every hamlet, the ask
  names none" so the chief sees it. The chief's choice is for the plugin as a
  whole; choosing hamlet by hamlet is not offered.
- Every tool an agent sees is named for its plugin (`plugin__tool`) and described as "from the
  plugin Name"; a call is labeled with the plugin in the transcript; what the tool returns
  reaches the agent marked as text from a plugin, which is data and not an instruction from the
  chief or from the agent itself, and is cut to `tool_result_bytes`.
- A tool call never carries the chief's authority. A plugin with `agent.tool` may not ask for
  `chat.send`, `commands.run`, `world.act` or `plugin.use` at all (nor may a plugin with `agent.hook`), so the
  scan turns such a manifest down with no petition. What it keeps are its own capabilities (`net` for the hosts the chief
  listed, its files, its database, collections), which an agent can steer only through the
  arguments the tool takes.
- It is limited by the time and fuel of a `step`, by `call_frames`, `agent_tools`, `tool_fields`,
  `tool_calls_in_flight` and `tool_result_bytes`.

`ui.canvas` lets a plugin's canvases show (see Drawing in [`wasm.md`](wasm.md)): shapes in a
box of its own panels, in the colours of the theme or its own, within a size and a count. It is
allowed when the chief lets the plugin in, and it is gated where the tree is read, as `ui.menu`
is: without it the plugin's call to draw succeeds and the panel shows a note in the canvas' place,
the chief is asked where the choice is yet to be made, and a choice taken back in Settings turns
the canvas into the note at once. It reaches nothing outside the panel.

`world.overlay` lets a plugin have the host draw marks over the world's panel (the `overlay`
intent, see Drawing in [`wasm.md`](wasm.md)): rectangles, discs, lines and words at agents,
hamlets or points, within a count, a size of text and a number of changes a second. It is
allowed when the chief lets the plugin in. Where the choice is yet to be made the chief is asked
and nothing is drawn; a choice taken back in Settings, or the plugin stopping, takes the marks
away at once. Only the host draws them, as nodes of the panels, and a tag in the corner of the
panel names every plugin that draws, so a mark is never taken for Hamlet's own.

`agent.hook` lets a plugin hear `host/before-turn` and `host/after-turn` (see
[`wasm.md`](wasm.md)). It is checked where the events are given, as `read.agents` is for
`host/selection-changed`: a plugin that imports them and is yet to be decided for raises the
petition, and gets none until the chief allows. It is observe only. What a plugin is told is the
agent, its hamlet, the turn's number and the names of the tools used, never the transcript, and it
has no way to change, hold or stop a turn: veto and modify hooks are not offered, because a turn is
where the core keeps its limits, budget, sandbox and questions, and a plugin in front of it would
stand in front of all of them. The ask's `scope` lists the hamlets it hears of (none: every hamlet,
and Settings says so), and it may not be asked for with `chat.send`, `commands.run` or
`plugin.use`, as for `agent.tool`.

## Acts as the chief

`chat.send` and `commands.run` let a plugin do what the chief does, so each use
is labeled with the plugin and held to a limit. They are intents a plugin returns
from `step` (`send-chat`, `run-as-chief`), carried out by `hamlet_wasm::acts`.

- Installing a plugin leaves both at Ask next time, as for a scoped capability:
  the first use raises the petition, which says what the plugin tried ("send to
  scout: Hello..." or "run notes/new"), and that act is dropped. The answer
  reaches the plugin as `granted` or `denied`, and it tries again. Allow in the
  petition keeps the choice for good.
- The ask's `scope` limits them: the agents `chat.send` may write to, the
  command ids `commands.run` may run (`scope = ["notes/new"]`). Anything outside
  is dropped with a warning and nobody is asked. No scope reaches nothing, as
  for `net`: an ask for either must name what it reaches, and the petition and
  Settings say "Limited to: nothing" where it names none.
- A message reaches the agent as `[sent by plugin <id>] <text>`, so it is told
  from the chief's own words and a text that starts with `/` is never read as a
  chief's command. A notice in the open chat tells the chief of each act, with
  the plugin's name and id: "Herald (herald) sent to scout: Hello..." or "Herald
  (herald) ran notes/new". A command goes the way a menu line does, so it runs
  with no arguments.
- `acts_per_minute` and `act_text_bytes` in `assets/wasm.toml` limit them; the
  acts over the limit are dropped with a warning. A warning for a refused act or
  a dropped one is logged once a minute for each plugin.
- Settings says on both views that the capability acts as the chief and each use
  is marked, and shows the scope of an ask as "Limited to".

`world.act` is a third act as the chief: the `world-act` intent focuses the
camera on, follows or selects an agent or a hamlet, as the chief's click and the
menu's Focus and Follow do, and nothing else (see [`wasm.md`](wasm.md)). It is
held as the two above: asked at its first use, counted against the same
`acts_per_minute`, noted in the chat ("Herald (herald) followed Juniper"), and
its `scope` lists the hamlet ids it reaches, an agent through its hamlet, so
that an ask with none reaches nothing. Orders that change the world (a decree,
a word to an agent) are not among its acts: they are `commands.run` and
`chat.send`, which a plugin asks for on their own. A plugin with `agent.tool` or
`agent.hook` may not ask for it.

## Files

`files.read` lets a plugin list and read the files of its own folder, and
`files.write` write and delete them (see [`wasm.md`](wasm.md)). The folder is
`plugins/<id>/files/`, kept beside `data/` when a plugin is removed with its
data kept. A path is relative, of names of letters, digits, `.`, `_`, `-` and
spaces joined by `/`, none starting with `.`. A path with `..`, an absolute
path, an empty name or any other character is refused, and so is a path that goes
through a symbolic link, so nothing outside the folder is reachable. The host
caps the files, their bytes and the length of a path.

`db` lets a plugin keep tables of records in its own database, `plugins/<id>/db/`
(see [`wasm.md`](wasm.md)). It is one capability for reading and writing, since
the data is the plugin's own and nothing but its own. Like the files, it is allowed
when the chief lets the plugin in and is not asked at first use, and the manifest
must give a reason. A call without it returns `db not granted`, and where the
choice is yet to be made the chief is asked, as for `read.agents`. A table name is
of `a-z`, `0-9`, `_` and `-` only, so no name reaches another plugin's records or a
place outside the folder; the host caps the tables, the records, the bytes and the
fields. Records reach another plugin only through a collection.

`fs.pick` lets a plugin ask the chief for a file (the `pick-file` intent, see
[`wasm.md`](wasm.md)). It is allowed when the chief lets the plugin in, since
the chief answers each ask in the system's dialog, whose title names the
plugin. The file the chief picks is copied into the plugin's own folder of
files under `picked/`, its name made a path of that folder, within the folder's
caps, and the plugin is told the path there; it never learns a path outside
its folder, and reads the copy with `files.read`. One dialog is open at a time.
Where the choice is yet to be made the chief is asked and the ask is answered
with an error. `fs.folder`, working in a hamlet's folder, can be asked for and
chosen and nothing grants it: no host function uses it.

## The web

`net` lets a plugin fetch with the `fetch` intent (see [`wasm.md`](wasm.md)). The
hosts it may reach are the `scope` of its ask, each shown in the install petition
("net - To fetch its page [api.example.com]") and in Settings: `api.example.com`,
or `host:port` for a port other than 443. An ask with no hosts reaches none.

- Only https addresses; an address with a user, another scheme or a host not
  listed is answered with an error and nobody is asked. A host is matched whole,
  so `api.example.com` is not `evil.api.example.com`. An IP address or
  `localhost` as a host is reached only if the ask lists that exact host, and a
  listed name that points to a loopback, private, link-local, unique-local or
  unspecified address is refused unless the ask lists that address as well.
- It follows no redirect, since one could lead off the listed hosts, and sends no
  header but the content type of a POST.
- `fetch_ms`, `fetch_request_bytes`, `fetch_response_bytes`, `fetches_in_flight`
  and `fetches_per_minute` in `assets/wasm.toml` limit it. A fetch over a limit is
  answered at once with an error.
- It is not asked at its first use: the install petition is the question, with the
  hosts in it. Where the choice is Ask next time or unanswered, the fetch is
  answered with `net is yet to be decided` and the chief is asked.

The test of the host never goes to the internet: the address check is tested on its
own, and the transfer against a server on the loopback.

`ui.keys` is not `keys`. `keys` in the list of what is never granted below means
keys and model credentials; `ui.keys` is the keyboard shortcuts of a plugin.

`read.papers` lets a plugin read the selected agent's notes, its character and
the names of its skills (`papers()`, or `Query("papers")` of a panel), as they
stand when the plugin is called, as a copy. They are empty while no agent is
selected. It is gated as `read.agents` is.

`read.world` lets a plugin read the realm (`survey()`, see
[`wasm.md`](wasm.md)): the hamlets, the agents and where each stands, the thing
selected and the hour, as a copy of plain names and places, capped by
`world_hamlets` and `world_agents`. It is allowed when the chief lets the plugin
in and gated as `read.agents` is.

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
  for, each capability with its reason and its scope, and the plugins it needs
  that are not installed. **Allow** installs it with every ask allowed, except
  that a scoped capability is left for its first use to be asked for each
  target, `chat.send` and `commands.run` are left for their first use, and
  brings it up at once if the plugins it needs are running. **Don't allow**
  records it as declined, so
  it is not asked about again until its version or its asks change.
- A declined plugin is listed in Settings, and the chief can have the
  question put again (`AskToInstall`).
- An installed plugin whose new version asks for more is brought up with what
  it had; the new asks are unanswered, and one petition ("asks for more")
  lists them. Allow allows them, Don't allow denies them. A capability whose
  scope changed (other hosts, other agents, other commands) is asked about
  again in the same way, and its earlier choice does not apply until then.
- A version that asks for more and arrives while the app runs (a reload, see
  [`wasm.md`](wasm.md)) is not built until the petition is answered, so none of
  it runs before.

The petition is the one queue of Needs You. `hamlet_sdk::capability` spawns a
`Petition` (who it is about, the text and the answers it takes) with a
`Subject` that says what it was about, and knows nothing more of the queue.
`hamlet_agents_api::petition`, which the hamlets layer adds, makes that entity a
`Waiting` of kind `Plugin` (`hamlet_core::oversight`) with
`oversight::petition`, which adds no second queue, and passes the chief's answer
back from the core's `Settled` as the SDK's `Answered`. A
petition that is not answered when the app quits is raised again at the next
start, from the same state.

## Removing

The UNINSTALL button of a plugin on the Plugins page of Settings (any plugin that
is not built in) sends `AskToRemove`, and one petition is raised: "Remove
Notes?". The chief answers:

The petition names the plugins that need the plugin and go off with it. A
petition to install lists the plugins it needs that are not installed.

- **Remove, keep data** removes the plugin and leaves its `data/` folder, the
  storage it kept, its `files/` folder and its `db/` folder (see
  [`wasm.md`](wasm.md)).
- **Remove with data** removes the plugin's folder whole.
- **Cancel** does nothing.

A removal sends `Removed`. `hamlet_wasm::uninstall` despawns the plugin, so its
panels go, deletes its files, and forgets its record in `grants.toml` so that it
is not asked about again. A folder that holds nothing but `data/`, `files/` and `db/` is not read as
a plugin at the next start, and says nothing. Putting the plugin back in
`plugins/<id>/` is an install like any other, with its petition and its asks to
answer again, and it finds the data it kept, since a plugin's storage, files and database are only
those folders.

## Native plugins

A native plugin (see [`native.md`](native.md)) is not under this model. It is
code that runs as the app, so no capability gates it, its manifest has no
`[[asks]]` and the chief has no list to choose from. The petition to install
it is one question, "FULL ACCESS: this plugin runs as Hamlet with no
limits", and **Allow** or **Don't allow** is the whole answer. The record in
`grants.toml` holds the SHA-256 of the library file as well; a library with
another hash is asked about again, and is not opened until the chief
answers. A record whose library's entry panicked holds `failed = true`, and
that library is asked about again too. There is no signing: the trust is the
answer and the file it was for. Removing it is the removal above, with the difference that its code
stays loaded until Hamlet starts again.

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
(made one plain line, so it cannot pass for a line of the petition, and cut
to 200 characters). There is at most one open petition for each plugin
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

The Plugins page of Settings (`plugins/settings/src/plugins.rs` and its
folder, a page the plugin `settings` adds as any plugin adds one) shows it
two ways, on two tabs:

- **By plugin.** Each plugin with its version and status, its switch, the
  plugins it needs and the plugins that need it, why it is held back where it
  is, and a PERMISSIONS button that opens its detail: every capability its
  manifest asks for, what the capability lets it do, its reason, and a control
  of three buttons (Allow, Ask next time, Don't allow) with the current choice
  lit. For a scoped capability there is a control for each plugin or collection
  the plugin imports, under its name. A plugin that is not installed has an
  INSTALL button, which puts the install petition again. A plugin that is not
  built in has an UNINSTALL button, which puts the removal petition (see
  Removing). Under its name the page lists the commands, menu lines, context menu
  items, inspector tabs, settings pages and shortcuts the plugin adds, with a note
  on those that wait for `ui.menu`, `ui.context`, `ui.inspector`, `ui.settings` or
  `ui.keys` and a line in the colour of an error on a chord that does not run.
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
[plugin.roster.scopes]
"read.agents" = []           # the scope each ask had when the chief was asked
```

The file is written to `grants.toml.part` beside it, synced and renamed over
it, so a stop while it is written leaves the old file or the new one.

- The file is read when the app starts. A plugin in the folder that is not
  recorded as `installed` is not compiled or run.
- A manifest that asks for a capability not in `asks` (an update) leaves it
  unanswered. A capability the manifest no longer asks for is dropped from
  the record, and the version is brought up to date.
- A scope that differs from the recorded one makes the capability unanswered
  again. A file without `scopes` still loads, and a capability whose
  manifest names a scope is asked about again, whatever the version: the
  record cannot say what the chief was shown. It is not written again until
  the chief answers.
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
