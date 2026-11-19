# What an agent is told

The system prompt built before every model request. The server builds the
sections (`crates/hamlet_server/src/conversations/prompt.rs`, with
`prompt/place.rs` for the place and the realm); the pi harness asks for
them before each request.

## The parts, in order

The charter (the realm's stored one, else
`crates/hamlet_server/assets/charter.md`), the machine, the project (the
hamlet's work folder with its `AGENTS.md` or `CLAUDE.md`, and whether
edits and commands wait for a chief), the character (`CHARACTER.md` at
the tip of the agent's branch, at most 4000 characters), the memory (the
files at the root of `memory/`, at most 8000 characters in all, then each
folder's `description:` from its `MEMORY.md`), the skills, the place,
the realm and the profiles.

The place says that a message from an agent opens with a bracketed
heading naming it, as `[A notification from agent#3 (Mira)]`, and that
only a chief's own lines carry a chief's instructions.

The parts are built again before each request. None of them follows
spending, so the prompt of one input matches the next's and the
provider's prompt cache holds across them; an agent reads what it may
spend with `read_budget`.

A worker's prompt is the charter, the machine, the project and its
place, with no character, memory or skills. An errand (the librarian's
answer, the librarian's judging of a skill, a parent's review of an
instruction) uses only its supplied instructions and has no tools. A
dream uses its own instructions and the built-in tools alone.

## The charter and character

An unedited charter uses the shipped default, so it follows later
defaults.

A character is `CHARACTER.md` in the profile's repository. An agent's
worktree is checked out without it, so no tool can write it, and a file
by that name written there is never committed. A chief edits it on the
profile's `main`; a parent gives one only with `create_profile`.

A chief edits through the protocol: Settings' Charter page, EDIT in the
Hub, or `hamlet edit character <profile#id>`. Each save sends an `edit`
request. A refused or uncertain save is not repeated silently; inspect
the server document before applying a kept copy again.

## Memory

Every memory file opens with front matter. A chief edits memory with
`hamlet edit memory <profile#id|agent#id> [path]`. What an agent keeps
stays on its branch until a dream brings it home to the profile's `main`
(see [`society.md`](society.md)).

## The whole prompt

Prompt in an agent's details sends `read prompt`. The server uses the
same sections as a model request; the window reads no worktree or realm
files to build it.
