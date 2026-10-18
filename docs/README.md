# The docs

How Hamlet is built and how to work on it. The pages here describe the
project as it stands. Three folders hold the rest: `spec/` the design,
`adr/` the decisions that are hard to reverse, and `proposals/` what is
not built. What is left for later is kept outside this repository.

## Start here

1. [`../README.md`](../README.md): what Hamlet is and how to run it.
2. [`tour.md`](tour.md): the map of the code. Where everything is, where to
   change what, the server modules and their interfaces, how a change
   travels from a typed line to the drawn world, and how to see your work
   without a window.
3. [`../AGENTS.md`](../AGENTS.md): the checks to run before a commit and
   the rules of working here.

Run `HAMLET_HOME=/tmp/hamlet-try make dev`. It installs the server's
dependencies, then the Rust CLI builds the window, starts a scratch server
and opens the window with its own scratch view folder.
Closing the window leaves the server running; Ctrl-C stops it. The full
setup is in [`../README.md`](../README.md#running).

Then read the page for the part you are about to touch.

## The parts

| Page | What it explains |
|---|---|
| [`core.md`](core.md) | The Rust runtime the server replaces, retained as a porting reference until step 7; the window's client core is mapped in `tour.md` |
| [`home.md`](home.md) | The retained Rust runtime's home layout; the server's home is described in [`agent-server.md`](proposals/agent-server.md#the-home-on-disk) |
| [`sandbox.md`](sandbox.md) | What an agent's tools can reach: the sandbox, the network, a folder a hamlet works on, manual and auto |
| [`prompt.md`](prompt.md) | What an agent is told every turn: the charter, its character, its notes, and how the chief reads and edits them |
| [`endless-world.md`](endless-world.md) | The land with no edge, the ground everything stands on, founding and moving a town |
| [`robots.md`](robots.md) | How the robots move: their motions, blending, rolling on their wheels, and what one costs |
| [`working-alone.md`](working-alone.md) | What a robot does in its yard while its agent works |
| [`chief.md`](chief.md) | The chief in the world: the Hearth, the mascot and Wickstep |
| [`sound.md`](sound.md) | Every sound and the music, all made in code |
| [`ui-warmth.md`](ui-warmth.md) | How the panels glow and answer the hand |
| [`society.md`](society.md) | Agents, roads, votes, the shared library, departures and the realm, on the server and in the window |
| [`protocol.md`](protocol.md) | Protocol v0: transport, messages, watched conversations and errors |
| [`chat.md`](chat.md) | Watched conversations, paging, drawing, local links and pictures, and rewinds through the server |
| [`icons.md`](icons.md) | The icons of the panels: where they are drawn, the rules of the family, how to add one |
| [`cursors.md`](cursors.md) | The pointer and its modes |
| [`releases.md`](releases.md) | How a release is built and published for the three platforms |

## The rules

| Page | What it sets |
|---|---|
| [`bevy.md`](bevy.md) | What Bevy 0.19 offers, which tool fits which job, and the habits that keep a frame cheap |
| [`code_convention.md`](code_convention.md) | How code is written here: ownership, plugins, tuning values, shaders |
| [`comment_convention.md`](comment_convention.md) | How comments are written here |

## Adding a page

A page here explains something that exists. It has no status line, no
"not yet" list and no screenshots: a design for something not built goes
in `proposals/`, and what is unfinished is kept outside this repository.
Add the new page to the table above.
