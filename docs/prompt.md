# What an agent is told

The system prompt built before every model request, where each part comes
from, and how the chief reads and edits it. The agent builds the prompt
before each request; the realm renders the sections that come from its
record.

## The parts, in order

| Part | Where it comes from | Who changes it |
|---|---|---|
| The charter | The realm's stored charter, else `crates/hamlet_server/assets/charter.md` | A chief |
| The machine | The sandbox, network, model, the agent's filesystem and any tools that failed to load | The settings |
| The project | The hamlet's work folder, with its `AGENTS.md` or `CLAUDE.md`, and whether each edit and command waits for a chief (manual mode) | A chief |
| The character | `CHARACTER.md` at the tip of the hire's branch (the Hand's, at its profile's), at most 4000 characters | A chief |
| The memory | The files at the root of `memory/` in the agent's filesystem, at most 8000 characters in all, then each folder's `description:` from its `MEMORY.md` | The agent |
| The skills | The skills in the agent's filesystem, then the library's, with descriptions and how to run them | The agent; the librarian |
| The place | Who the agent is, whom it answers to, the hires under it, the roads it can write along, the tools its lead turned off, and how its hamlet was run lately | The record |
| The realm | For the Hand and mayors, the other hamlets, their mayors and hires; a realm with none says so and that a chief or the Hand can found one | The realm's record |
| The profiles | For the Hand and a lead, the living profiles it may hire, each as `profile#<id> (<name>)`, since no name finds one | The city |

`hosts/node/src/agent/prompt.ts` composes the sections from the agent's
copies of the realm's documents, its filesystem, the library's shelves and the
workspace's tool list. `crates/hamlet_actors/src/realm/prompt.rs` renders
the place, the realm and the profiles, which the agent reads with one
`read_place` per request. The parts are built
again before each request. None of them follows spending, so the prompt of one
input matches the next's, and the provider's prompt cache holds across them;
an agent reads what it may spend with the `read_budget` tool.

A worker's prompt is the charter, the machine, the project and its place. It
has no character, memory or skills of its own. An errand is a one-off
conversation of its own: the librarian's answer, the librarian's judging of a
skill and a lead's review of an instruction use only their supplied
instructions, and have no tools. A dream uses its own instructions and has the
built-in tools alone.

## The charter and character

The charter is a document of the realm's, edited through the protocol. An
unedited charter uses the shipped default, so it follows later defaults. BACK
TO THE DEFAULT clears the chief's replacement after asking first.

A character is a file, `CHARACTER.md`, in the profile's repository. A hire's
filesystem is filled without it, the agent's own write tool refuses a file by
that name there, and none is ever committed. A chief edits it on the profile's
`main`; a lead gives one only when it makes a profile with `create_profile`. A
hire's prompt reads it at the tip of its own branch, which began at `main` or
at its pin.

Settings has the Charter page. CHARACTER in the Hub's details sheet shows the
agent's profile's character. EDIT opens a local temporary Markdown copy, and
each save sends an `edit` request to the server. Character filenames name the
profile, so kept copies can be identified. `hamlet edit character <profile#id>`
does the same from a terminal.

Closing the window polls the editor once more. While saves are unconfirmed it
keeps the window open and shows the recovery paths, with KEEP OPEN and CLOSE
WITH COPIES. A confirmed save allows the close to finish. A refused or
uncertain save is not silently repeated; inspect the server document before
applying a kept copy again. Closing stops no agent.

## Memory

An agent keeps its memory in its filesystem, as files. Files at the root of
`memory/` go into every prompt; a folder shows only the `description:` line at
the top of its `MEMORY.md` until the agent reads it. Every memory file opens
with such a front matter. A chief can read and edit a hire's memory or its
profile's: `hamlet edit memory <profile#id|hire#id> [path]`, or MEMORY in
the Hub. What a hire keeps stays on its branch until a dream brings it home
to the profile's `main` (see [`society.md`](society.md)).

## The whole prompt

Prompt in an agent's details sends `read prompt`. The agent answers with
the same prompt sections as a model request, and the Hub displays the returned
text. The window reads no agent's filesystem or realm files to build it.
