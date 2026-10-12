# What an agent is told

The system prompt an agent begins every turn with: where each part comes
from, who may change it, and how the chief reads and edits it.

## The parts, in the order they are sent

| Part | Where it comes from | Who changes it |
|---|---|---|
| The charter | The home's `charter.md`, or `assets/charter.md` when that is missing or empty | The chief |
| The machine | The sandbox, the network, the model and the workspace | The settings |
| The project | The folder the hamlet works on, with its own `AGENTS.md` or `CLAUDE.md` | The chief |
| The character | `CHARACTER.md` in the agent's house | The chief. The mayor once, when it creates the agent |
| The notes | `workspace/NOTES.md` in its house | The agent |
| The skills | `workspace/skills/` in its house | The agent |
| The library | The skills every agent shares | The librarian |
| The hamlet | Who is in it and how it is run | The record |
| The realm | The other hamlets | The record |

`Told` in `hamlet_core/src/agent/told.rs` puts the parts together for
every step of a turn. The files are read again each time, so a file
changed by hand counts from the agent's next step.

## The charter

The words every agent's prompt opens with. The default is compiled into
the app. A home that keeps a `charter.md` with words in it is told that
one instead. Nothing copies the default into a home on its own, so a home
that never edited it gets each later default.

Settings has a Charter page. EDIT writes the default into the home's
`charter.md` if the home has none and opens that file in the system's
editor. BACK TO THE DEFAULT asks first and removes the home's file.

## The character

One agent's own text: how it behaves and speaks. It is `CHARACTER.md` in
the agent's house, beside `workspace/` and outside it. At most 4000
characters of it are sent.

An agent cannot write its own character. Its commands and its `edit` tool
can write only in its workspace and the project folder, and a hamlet
cannot work on a folder that holds Hamlet's home. With `--insecure` there
is no sandbox, and a command can write any file the chief can.

The mayor may give a character in `create_agent`. It has no tool that
changes one afterwards, its own included.

The file moves with the house: into the archive when the agent is removed
or merged away, and back out when it is restored.

The Inspector's CHARACTER tab shows it for the selected agent. EDIT opens
the file.

## The whole prompt

Prompt, in the right-click menu of an agent, opens the agent's whole
prompt as it would be sent now. `session::prompt_of` builds it from the
same `Told` the loop uses.
