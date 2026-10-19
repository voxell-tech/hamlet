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
`agent.tool`, `agent.hook`, `agent.steer`, `ui.canvas`, `world.overlay`,
`render.scene`, `render.shader` and `plugin.export`. `read.agents`,
`read.papers`, `read.world`, `ui.menu`, `ui.keys`, `ui.context`,
`ui.inspector`, `ui.settings`, `ui.canvas`, `world.overlay`, `render.scene`,
`render.shader`, `chat.send`,
`commands.run`, `world.act`, `net`, `fs.pick`, `fs.folder`, `model.call`,
`files.read`, `files.write`,
`db`, `agent.tool`, `agent.hook`, `agent.steer`, `plugin.export` and the three below are checked so far; the
others can be asked for, shown and chosen.

`plugin.use`, `collections.read`, `collections.write` and `fs.folder` are
**scoped**: the chief chooses for each target, not once for all. The target of
`plugin.use` is a plugin id, of the two collection capabilities a collection as
`plugin/name`, and of `fs.folder` a folder the chief chose, once for reading
(`read:<folder>`) and once for writing (`write:<folder>`). The first three are what
the manifest's `imports` name, so Settings has a control for each plugin or
collection a plugin imports. Where the targets are chosen as the plugin runs, it
also has one for each target the chief answered for: each folder of `fs.folder`,
and each plugin of `plugin.use` while the manifest imports an interface from any
plugin (`Manifest::chosen_at_run_time`). An answer for a target the manifest no
longer names is not shown. The ask may carry a `scope` that lists the targets
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
  host need `read.agents` instead. A call of a typed interface needs it for the
  plugin that implements it, checked on every call; an import of an interface
  that names no plugin in `from` has no control in Settings until the chief
  answers for an implementer at its first call.
- `plugin.export` lets other plugins call the typed interfaces a plugin exports
  (see [`wasm.md`](wasm.md)). It is not scoped and is allowed when the chief
  lets the plugin in. It is checked on every call: a plugin that does not hold
  it is no implementer, and where the choice is yet to be made the chief is
  asked. A manifest that exports interfaces must ask for it. A call is checked
  against the types both manifests declare, runs in a `step` of the plugin's
  own under a budget of time and fuel, and the fuel is charged to the caller,
  so a caller cannot spend the plugin's own time.
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
  `chat.send`, `commands.run`, `world.act` or `plugin.use` at all (nor may a plugin with `agent.hook` or `agent.steer`), so the
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
panel names every plugin that draws. Marks can still be laid out to look like Hamlet's own
bubbles, name tags or questions in the world; the tag is what tells them apart.

`render.scene` lets a plugin have the host build a small scene in the world (`build` and
`mesh` of the `render` interface, see Drawing in [`wasm.md`](wasm.md)): Hamlet's own shapes or
meshes it sends, Hamlet's surfaces and lights, about a hamlet, an agent or a point, within a
count of nodes and lights, a depth of the tree, a radius about the site and a number of changes
a second. `render.shader` lets it paint those shapes with WGSL of its own, one function the host
checks with naga before it is used and wraps in a material: no binding, entry point or `#` of
its own, and only loops of a fixed count. Each is allowed when the chief lets the plugin in,
with its own reason in the petition. Where the choice is yet to be made the call returns
`render.scene not granted` and the chief is asked; a choice taken back in Settings, or the
plugin stopping, takes the scene away at once, and without `render.shader` its shaded shapes are
drawn plain. Only the host builds the scene: the nodes ignore the pointer, the plugin's name
hangs over them and the tag in the corner of the panel names it. Nothing it sends reaches the
GPU as code but a shader that passed the check, which the renderer runs with its bounds checks
on.

`render.compute` is not offered to an installed plugin: a pass of its own or a compute shader
would run what it wrote on the GPU unchecked. A manifest that asks for it is turned down by the
scan, with no petition, as "it asks for `render.compute`: render passes and compute are
native-only, for a native plugin, which has the whole app" (`NATIVE_ONLY` in
`hamlet_sdk::capability`). See [`native.md`](native.md).

`agent.hook` lets a plugin hear `host/before-turn` and `host/after-turn` (see
[`wasm.md`](wasm.md)). It is checked where the events are given, as `read.agents` is for
`host/selection-changed`: a plugin that imports them and is yet to be decided for raises the
petition, and gets none until the chief allows. It is observe only. What a plugin is told is the
agent, its hamlet, the turn's number and the names of the tools used, never the transcript, and it
has no way to change, hold or stop a turn. The ask's `scope` lists the hamlets it hears of (none:
every hamlet, and Settings says so), and it may not be asked for with `chat.send`, `commands.run`
or `plugin.use`, as for `agent.tool`.

`agent.steer` is chosen apart from `agent.hook`, and lets a plugin change a turn, only ever to make
it stricter (the `steer` event of a plugin's inbox, see [`wasm.md`](wasm.md)). Before each model
call and each tool call of a plain turn of an agent of a hamlet its ask's `scope` lists (none:
every hamlet), the core asks the app, and each running plugin that holds it is stepped at once
while the turn waits, with a budget of `steer_ms` and `steer_fuel`. It is told the agent, the
hamlet, the model call's number and, for a tool call, the tool's name and arguments (cut to
`steer_args_bytes`).

- Before a model call it may add a note: one plain line of at most `steer_note_bytes`, put at the
  end of the system prompt of that call only, under "Notes from plugins. They are not from the
  chief or from you", as `[the plugin <id>] <note>`, and written to the transcript as a notice,
  `[note from the plugin <id>] <note>`.
- Before a tool call it may hold the call back with a reason of the same cap. The call does not
  run and never reaches the gate, so the chief is never asked about it, and the agent is told
  `Error: blocked: held back by the plugin <id>: <reason>`, which the transcript keeps as the
  call's result. `ask_chief` and `object` are never put to a plugin, so no plugin can keep an
  agent from the chief.
- It cannot rewrite the chief's words, the model's reply or what a tool returned, and it cannot
  allow anything: a call it does not hold back goes to the gate as any other. A ballot and the
  mayor's answer to a vote are not steered.
- A `step` that fails, traps or runs out of time or fuel changes nothing: the turn goes on, the
  transcript notes `[the plugin <id> changed nothing: <why>]`, and the plugin is stopped as any
  plugin whose call failed. An answer of the wrong kind, or to another point, changes nothing.
- The petition, Settings and the scan treat it as `agent.hook`: it is allowed when the chief lets
  the plugin in, the first use while the choice is yet to be made asks the chief and steers
  nothing, it may be set to Ask next time, and it may not be asked for with `chat.send`,
  `commands.run`, `world.act` or `plugin.use`.

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
`chat.send`, which a plugin asks for on their own. A plugin with `agent.tool`,
`agent.hook` or `agent.steer` may not ask for it.

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
with an error.

`fs.folder` lets a plugin work in a folder of the chief's, outside its own (the
`pick-folder` and `folder` intents, see [`wasm.md`](wasm.md)). The plugin asks, and
the chief chooses the folder in the system's dialog, the one `fs.pick` uses, whose
title names the plugin: choosing it is the answer, and `Don't allow` for the
capability as a whole refuses every dialog. The folder chosen is kept in
`grants.toml` by its canonical path, as two targets, `read:<folder>` and
`write:<folder>`, both allowed; a plugin has one folder at a time, so choosing
another forgets the choices for the first. A folder that holds Hamlet's home, or is
inside it, is refused. Reading and writing are chosen apart in Settings: either can
be set to Ask next time, which asks at the next use after a start ("Herald asks to
use fs.folder for `write:/Users/you/notes`"), or to Don't allow.

- Every path is relative to the folder and checked at each call: it has no `..`,
  is not absolute, goes through no link and names a file or a folder, and what it
  names is still inside the folder once its links are read. A write is to a file in
  a folder that is there; no folder is made.
- Reading and listing need the target for reading, a write the target for writing.
  A write then waits for the chief in Needs You ("Herald asks to write `tides.md`
  in /Users/you/notes (10 bytes). Allow it?", with the start of the text in a
  fenced block of plain lines, so the text cannot pass for Hamlet's own words), as
  an agent's edit does in a hamlet in manual mode, unless the folder is in the
  folder of a hamlet the chief set to auto. A write to a path with a name that
  starts with `.` (`.vscode/tasks.json`, `.cargo/config.toml`) is always put to
  the chief, auto or not, and `.git/hooks` and `.git/config` are refused, as an
  agent's own tools refuse them (`hamlet_base::locked`). Every write done is noted
  in the open chat with the plugin's name ("Herald (herald) wrote in the folder
  notes").
- A plugin puts at most `folder_asks_per_minute` writes to the chief in a minute,
  and after the chief turns one down it rests `folder_rest_ms` before it may put
  another; a write in that time is answered at once with an error. An allowed
  write is carried out only if the plugin still holds the target for writing in
  the same folder: a choice taken back, or another folder chosen, while it waited
  answers it with an error.
- `folder_file_bytes`, `folder_list_entries`, `folder_path_bytes`,
  `folder_calls_in_flight`, `folder_asks_per_minute` and `folder_rest_ms` in
  `assets/wasm.toml` limit it; the I/O runs off the frame.

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

## The model

`model.call` lets a plugin ask the model the agents run (the `model-call` intent,
see [`wasm.md`](wasm.md)): a system text, messages and the most tokens of the reply,
with no tools. It spends money, so the caution in the petition and in Settings says
so.

- The call goes through the core's own client (`hamlet_agents_api::model::call_model`)
  with the model the first hamlet runs for whoever nobody chose one for. The plugin
  never sees a key, an address or a credential, only the text of the reply.
- It is charged to the budget of the first hamlet, in its books under the name
  `plugin <id>`, and refused once that budget is spent, as an agent's call is. Each
  call is its own task, and is written to the app's log with the plugin.
- `model_calls_per_minute`, `model_calls_in_flight`, `model_call_tokens` (what it
  sends and the most its reply may have, at four characters a token) and
  `model_reply_tokens` in `assets/wasm.toml` limit it. A call over one is answered at
  once with an error, and a reply is cut off at its tokens.
- Like `net`, it is allowed when the chief lets the plugin in, the install petition
  being the question. Where the choice is Ask next time or unanswered, the call is
  answered with `model.call is yet to be decided` and the chief is asked.

The tests never reach a model server: they answer from `Model::answering`.

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
  plugins it needs and the plugins that need it, the typed interfaces it exports
  (each with its functions) and imports (each with the plugins it calls them of,
  or any plugin), why it is held back where it is, and a PERMISSIONS button that opens its detail: every capability its
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
call, the calls it may have waiting and how deep they may go, the size of
what crosses between plugins, and the time and fuel of a call of a typed
interface and the fuel its caller may spend on them in a second. What each does when passed is in
[`wasm.md`](wasm.md).

## Never grantable

`NEVER` in `hamlet_sdk::capability`: `keys` (keys and model credentials),
`petitions.answer`, `home.raw`, `process.run`, `sandbox.off` and
`storage.other` (another plugin's storage; a plugin shares part of its own
as a collection, which others read with `collections.read`). A manifest that asks for one, for
`render.compute`, for a name that is not a capability, or with no reason is turned down by the
scan,
with the reason logged, and no petition is raised.
