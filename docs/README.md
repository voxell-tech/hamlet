# The docs

How Hamlet is built, as it stands. `spec/` holds the design and `adr/`
the decisions that are hard to reverse.

## Start here

1. [`../README.md`](../README.md): what Hamlet is and how to run it
   ([scratch run](../README.md#try-it-without-installing)).
2. [`tour.md`](tour.md): the map of the code.
3. [`../AGENTS.md`](../AGENTS.md): the checks and the rules of working
   here.

## The parts

| Page | What it explains |
|---|---|
| [`realm.md`](realm.md) | The realm on disk |
| [`cli.md`](cli.md) | The `hamlet` command |
| [`sandbox.md`](sandbox.md) | What an agent's tools can reach |
| [`mcp.md`](mcp.md) | The MCP servers whose tools agents are offered |
| [`prompt.md`](prompt.md) | What an agent is told every turn |
| [`islands.md`](islands.md) | The islands, the harbor and the ground |
| [`robots.md`](robots.md) | How the robots move |
| [`working-alone.md`](working-alone.md) | A robot in its yard |
| [`chief.md`](chief.md) | The Hearth, the mascot and Wickstep |
| [`sound.md`](sound.md) | The sounds and the music |
| [`ui-warmth.md`](ui-warmth.md) | How the panels glow |
| [`society.md`](society.md) | The Hand, profiles, hamlets, channels, dreams, the library |
| [`harness.md`](harness.md) | Runtimes and the runtime protocol |
| [`protocol.md`](protocol.md) | The client protocol |
| [`chat.md`](chat.md) | An agent's conversation |
| [`icons.md`](icons.md) | The panels' icons |
| [`cursors.md`](cursors.md) | The pointer |
| [`releases.md`](releases.md) | Packaging, releases and self-update |

## The rules

| Page | What it sets |
|---|---|
| [`bevy.md`](bevy.md) | Which Bevy tool fits which job |
| [`code_convention.md`](code_convention.md) | How code is written |
| [`comment_convention.md`](comment_convention.md) | How comments are written |

A page here explains something that exists: no status line, no "not
yet" list, no screenshots. Add new pages to the tables.
