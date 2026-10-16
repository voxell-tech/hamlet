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
| [`sdk.md`](sdk.md) | What a feature of the window is built with, how to add a panel, and where plugins meet |
| [`ui-warmth.md`](ui-warmth.md) | How the panels glow and answer the hand |
| [`chat.md`](chat.md) | What an agent's chat holds, in what order, and how it is drawn as a group chat |
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
"not yet" list and no screenshots: what is unfinished, and a design for
something not built, is kept outside this repository. Add the new page to
the table above.
