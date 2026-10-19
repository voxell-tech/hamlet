# The docs

How Hamlet is built and how to work on it. Everything here describes the
project as it stands. What was intended, and what is left for later or
only proposed, is kept outside this repository.

## Start here

1. [`../README.md`](../README.md): what Hamlet is and how to run it.
2. [`tour.md`](tour.md): the map of the code. Where everything is, where to
   change what, how a change travels from a typed line to the drawn world,
   and how to see your work without a window.
3. [`../AGENTS.md`](../AGENTS.md): the checks to run before a commit and
   the rules of working here.

Then read the page for the part you are about to touch.

## The parts

| Page | What it explains |
|---|---|
| [`core.md`](core.md) | How the records on disk become entities, who writes them and who reads them |
| [`home.md`](home.md) | The home on disk: every file and folder in `~/.hamlet`, the settings and the starter skills |
| [`sandbox.md`](sandbox.md) | What an agent's tools can reach: the sandbox, the network, a folder a hamlet works on, manual and auto |
| [`prompt.md`](prompt.md) | What an agent is told every turn: the charter, its character, its notes, and how the chief reads and edits them |
| [`endless-world.md`](endless-world.md) | The land with no edge, the ground everything stands on, founding and moving a town |
| [`robots.md`](robots.md) | How the robots move: their motions, blending, rolling on their wheels, and what one costs |
| [`working-alone.md`](working-alone.md) | What a robot does in its yard while its agent works |
| [`chief.md`](chief.md) | The chief in the world: the Hearth, the mascot and Wickstep |
| [`sound.md`](sound.md) | Every sound and the music, all made in code |
| [`ui-warmth.md`](ui-warmth.md) | How the panels glow and answer the hand |
| [`chat.md`](chat.md) | What an agent's chat holds, in what order, and how it is drawn as a group chat |
| [`sdk.md`](sdk.md) | What a plugin is built with: the plugin types, `hamlet_sdk` and `hamlet_agents_api`, commands and how a menu line, a key or an item of a context menu runs one, the subject a command is run on, the tabs of the inspector and the pages of the settings, the `plugins/` folder and the `Builtin` group |
| [`wasm.md`](wasm.md) | The host of the installed plugins: the interface, the panel tree, the levels of drawing (a canvas, marks over the world, and what is left to native plugins) and events, reading the world and acting on it as the chief's click, a file or a folder the chief picks, asking the model, storage and state, plugins working together (needs, commands, events and shared collections), the chief's commands, menu lines, context items, inspector tabs, settings pages and keys, tools for the agents, steering their turns, grants, the limits and starting a stopped plugin again, the install folder and how to build a plugin |
| [`native.md`](native.md) | Native plugins, dynamic libraries that run as Hamlet with no limits: the folder and manifest, the full-access question, the gate a library passes to be loaded, building one in the checkout or outside it with an SDK kit made or fetched for a release, what cannot be undone without a restart and the safe mode after a stop while loading, after runs that did not exit or with `--safe-mode` |
| [`authoring.md`](authoring.md) | How to write a plugin: the templates, the build, keeping state and data, the check with pictures, installing it and seeing an edit reload, the manifest, the permissions flow, a menu line and a shortcut, an item of a context menu, a tab of the inspector and a page of the settings, a worked example of two plugins working together, a tool for the agents, watching the agents, drawing on a canvas and over the world, the limits, common mistakes and whether an agent can build in its sandbox |
| [`permissions.md`](permissions.md) | What an installed plugin may be allowed to do: the capabilities (`ui.context`, `ui.inspector`, `ui.settings`, `agent.tool`, `agent.steer`, `fs.folder` and `model.call` among them), the ones chosen for each plugin, collection or folder, the choices, the petitions, removing a plugin and its data, where the choices are kept, what is never granted and the limits |
| [`panels.md`](panels.md) | How a plugin's panel is described: the tree and its nodes (text area and markdown among them), Hamlet's panel language and its components, state, actions and reads, the data plugin kind and the example |
| [`icons.md`](icons.md) | The icons of the panels: where they are drawn, the rules of the family, how to add one |
| [`cursors.md`](cursors.md) | The pointer and its modes |
| [`releases.md`](releases.md) | How a release is built and published for the three platforms, the compiler every job pins, and the SDK kit published beside each app |

## The rules

| Page | What it sets |
|---|---|
| [`bevy.md`](bevy.md) | What Bevy 0.19 offers, which tool fits which job, and the habits that keep a frame cheap |
| [`code_convention.md`](code_convention.md) | How code is written here: ownership, plugins, tuning values, shaders |
| [`comment_convention.md`](comment_convention.md) | How comments are written here |

## Adding a page

A page here explains something that exists. It has no status line, no
"not yet" list and no screenshots: what is unfinished, and a design for
something not built, is kept outside this repository. Add the new page to
the table above.
