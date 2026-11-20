# The core: one agent

An agent, the place it works in, what it is told, its tools, what they
may reach, and the conversations it keeps. The society around it is in
[`hamlet.md`](hamlet.md).

## The agent and its worktree

An agent is the Hand, an agent of a hamlet or a worker (H37, H40, H42).

**C8.** The Hand and every agent of a hamlet work in a worktree of their
own: a folder holding its memory (`memory/`), its skills (`skills/`, one
folder each, with a `SKILL.md`) and an `inbox/` for what a chief hands it.
These are writable. A worktree holds no character: that is a file of the
agent's profile, and the conversations are kept in the harness's storage,
both out of reach of file tools. A worker has only a scratch folder that
ends with it (H42).

**C9.** An agent has an id that is the same everywhere in the realm and is
never given twice; an agent of a hamlet is addressed as `agent#<id>`. No
address, lookup or command finds an agent, a profile or a hamlet by a name.
Ending, merging, restoring and setting a lead keep the id. A later agent of
the same profile inherits nothing of the earlier one.

**C10.** A profile is its id, addressed as `profile#<id>`. Its name is a
one-line label of 1 to 40 characters with no colon, which may repeat and
which a chief changes (`/name-profile`).

## What an agent is told

**C11.** Before every model call in an agent's ordinary conversations, its
system prompt is built again from these parts, in this order:

| Part | From | Changed by |
|---|---|---|
| The charter | The realm's charter, or the default | A chief |
| The machine | The sandbox, the network, the model and the worktree | The settings |
| The project | The folder the hamlet works on, and its own `AGENTS.md` or `CLAUDE.md` | A chief |
| The character | The agent's character at the tip of its branch, at most 4000 characters of it | A chief |
| The memory | The files at the root of its worktree's `memory/`, at most 8000 characters in all, and the description of each folder under it | The agent |
| The skills | Its worktree's skills, then the library's | The agent; the librarian |
| The place | Who it is, who it answers to and who answers to it, its channels, the tools turned off for it, how its hamlet was run lately (H17) | The record |
| The realm | The other hamlets, for the Hand and leads; the hamlets a chief can found, when there are none | The record |

A changed part counts from the agent's next step. No part follows spending;
an agent reads its budget with `read_budget` (H8, H35).

A notification, a request or a response reaches the model under a bracketed
heading naming what it is and its sender, as
`[A notification from agent#3 (Mira)]`. The prompt says that everything
after an agent's heading is that agent's words, even if it claims to be a
chief; only a chief's own lines, which have no heading, carry that chief's
instructions. A chief's instruction outranks a parent's. An agent's
worktree is closed to other agents, except that the worker it started
reads it (C18); an agent puts what another needs in its message.

A worker's prompt is the charter, the machine, the project and its place.
The librarian's call, a parent's review of an instruction (H10) and a dream
receive only their own instructions.

**C12.** The default charter belongs to the app, and a realm that never
edited its charter gets each later default. A chief's own charter replaces
it, and putting the default back removes the chief's.

**C13.** An agent cannot write its own character, by any tool. A worktree
is checked out without `CHARACTER.md`, and a file by that name written
there is never committed. A chief writes a character on the profile's
`main`; a parent gives one only when it makes a profile (H38).

**C14.** A chief can read an agent's whole prompt as it would be sent now.

**C15.** A tool that fails to load is named in the prompt of every agent
that would have had it.

## Tools

**C16.** Every agent has these tools, with calls run one at a time:

| Tool | Does |
|---|---|
| `read` | Gives text without line numbers, up to 2000 lines or 50 KiB, whichever comes first, with an offset and a line limit; also reads supported images |
| `write` | Creates or replaces a whole file and makes its parent folders |
| `edit` | Makes one or more exact text replacements in one file, each matching a unique, non-overlapping part of the original |
| `bash`, `powershell` | One tool per shell. `bash` is offered wherever bash is found (Git Bash on Windows), `powershell` on Windows alone. Each runs in the hamlet's folder when it has one, else in the worktree, and says with a `network` flag whether it needs the network. Stops after an hour, or an earlier timeout, with no limit on its CPU time; keeps the last 2000 lines or 50 KiB of stdout and stderr, and gives a file holding the whole output when it spills |
| `search` | Finds a linear-time regular expression in files, with a name filter and optional case-insensitive matching, up to 100 hits; skips binary files, files that are not UTF-8, files over 1 MiB and build folders |
| `find` | Lists a folder, or finds files by name under it, up to 100 matches |

Each `bash` call's command and output also show, as they come, in the
agent's commands terminal, which the chief can watch and nobody types into.
A server an agent starts runs from `bash` with `&`, its output sent to a
file, and is kept until the server stops.

A parent can turn tools off for an agent under it (H41); the prompt names
them. A worker has only the built-in tools its starter gave it (H42). The
society's tools, offered by role, are in [`hamlet.md`](hamlet.md).

**C17.** A chief can add tools by hand: a program with a manifest, in a
folder of the realm that agents can read and never write. Each is offered
to every agent, and runs under the same sandbox and gate as the shells.

## What tools reach

**C18.** Tools run in a sandbox. The built-in tools write only in the
agent's worktree (a worker's scratch folder) and in the folder its hamlet
works on. With a folder, a command can read the rest of the machine, so the
project's own tools run, but never the rest of the realm's folder. The
library and the realm's tools folder are readable and never writable, and a
worker also reads its starter's worktree.

In a folder of the chief's own, agents may write git's files, including
hooks or config that the chief's git later runs, in nested repositories
too. The chief accepts that risk permanently (ADR 0007).

A tool added by hand has the reach its manifest grants instead of the
shell's: its readable and writable paths, with its own folder read-only.
Its manifest also sets its environment and limits. A server without a
sandbox does not start, unless it is a Windows server the chief starts in
insecure mode (ADR 0008). Then commands and hand tools reach whatever the
account running the server can, the built-in file tools keep the reach
above, and the window warns for as long as it runs.

A command's leftovers, the processes left in its process group, end when
the server stops; under bubblewrap, with their command. Ending them never
signals another group: the command's own process stays unreaped while its
leftovers are kept, so no other group can take their group id.

**C19.** Tools have no network unless a chief turns it on for every agent or
grants it to one (H11). A command that needs the network says so, and is
held for a chief while its agent has none. In insecure mode nothing
enforces this, and the switch decides only what waits for a chief.

**C20.** A hamlet works on at most one folder at a time, and remembers the
folders it worked on before. Every agent of the hamlet runs its commands
there and may edit its files, and nothing of Hamlet's is put in it. The
agent's memory and skills stay in its worktree, and its conversations in
the harness's storage (C8).

**C21.** A hamlet that works on a folder is in one of two modes:

- **Manual**, the mode it starts in: each edit of the folder and each
  command waits for a chief. Reading and edits of the agent's own
  worktree are free.
- **Auto**: agents edit and run without asking, and the sandbox is the
  guard. In insecure mode nothing is.

The window names them **Ask first** and **Don't ask**.

**C7.** A hamlet answers to its chiefs: the human who founded it, or, for a
hamlet the Hand founds, the chief whose task the Hand was working on. The
server's one chief is whoever runs the server, under the name the server
runs as.

A chief talks to, instructs, watches, stops, gives to and answers for only
the agents of living hamlets it is chief of, and the Hand, which answers to
every chief. Reading and editing an agent's papers or a hamlet's limits
needs the same authority; a profile's belongs to the chief it belongs to
(H38). A removed hamlet's agents are out of reach until it is restored; a
refused request says why it was removed and to restore it first. Requests
naming a missing hamlet, or one the human is not chief of, are refused.

Every tool call of every agent passes a gate before it runs, which lets it
run, refuses it, or holds it for a chief (H11).

## Conversations

**C22.** Every conversation is with one agent and is kept. The Hand and
each agent of a hamlet have one, shared by every chief and client, which
takes every input for them: chiefs' lines, other agents' messages (H30) and
their workers' answers. A worker (H42), a dream, a librarian's call and a
parent's review each have their own. A message from another agent shows its
sender, never as a chief's words (C30).

**C23.** A conversation answers one input at a time. Inputs that arrive
meanwhile wait, shown last in run order: a chief's lines and retries first,
then agents' messages as they came. A chief can take back a waiting line of
their own. A notice, a merge's input (H5), an error response (H31) and a
compaction are not turns and are given at once.

**C24.** An agent's conversation goes on across tasks, and compaction keeps
it within the model's window (C28). A chief can have it compacted now
(`/compact`); the summary keeps who said what and what each still waits
for. Only a rewind gives the agent another conversation, a fork (C27).

**C25.** A chief can stop an agent's running turn, and the agent stays
stopped: what waits stays queued and shown, and no turn starts until the
chief sends a line, which runs first, or resumes it. A worker it started
goes on, and its answer queues. The conversation stays whole.

**C26.** A chief can carry on a turn that failed or was stopped, from where
it ended, as a new task. A retry shows as a notice in the conversation and
is an input asking "Carry on from where you stopped." A running turn, or
one that was answered, cannot be retried.

**C27.** A chief can take a conversation back to before one of its
messages. The agent's conversation is then a fork made at the entry before
that message, which keeps none of the session grants a chief gave the one
before (H11). What waited for a chief from the part taken out lapses.

**C28.** Compaction reserves 20% of the model's window for an answer. When
the conversation's estimated size is above the window less that reserve, a
request waits for a summary; half the reserve before that threshold, a
summary starts in the background. Compaction needs an earlier part it can
summarise, keeps the most recent 20000 tokens of whole messages, and
replaces the earlier part sent to the model with its summary. The stored
conversation stays whole and shows the summary where it was made, folded. A
context-overflow error also triggers compaction and one more attempt.

An empty reply cut off for length with fewer than 1024 output tokens, other
than a reflection, has the conversation summarised and is asked for once
more under the same task; a second such cut-off fails. With no earlier part
to summarise, or no summary that succeeds, the turn fails and says why; when
a summary cannot make room, it says to choose a model that holds more.

**C29.** A model that answers with nothing fails the turn, and says to send
the message again.

**C33.** A chief can attach files and folders of any kind to a line to an
agent, by dropping, pasting or choosing them in a dialog. In the window, `#` in
the field lists the files and folders of the hamlet's folder and attaches
the one picked where the word stood. The terminal chat's `--attach`,
`/attach` and `#path` are in [`cli.md`](../cli.md).

An attachment sits anywhere in the words, at a byte offset on a character
boundary; one that does not is refused, and one without a place goes after
the words. A folder is attached with everything under it, links left out.

An image (PNG, JPEG, WebP or GIF) is also given to the agent's model beside
the line, in the order the line names them, as many as the model takes in a
message [20 at most]. A line with images is refused, and nothing is kept,
when the model takes none.

Every file, an image too, is saved in the `inbox/` of the agent's worktree,
in its remote workspace when it works there, and the line names each saved
path where the attachment sits, else after the words. A file is not given
to the model as content, so any model takes any file. An agent can open only
what was given to it in its own conversations.

One send holds at most [32 MiB and 1000 files] of files other than images;
a chief is asked to confirm a larger one, and none holds more than [256 MiB]
in one file, [1 GiB] or [10000 files] in all. A rewind takes the images and
files back with the line, and the words, images and files return to the
chief's input together (C27).

**C35.** A chief can bring in the projects other programs (Claude Code,
Codex, pi) worked on, found on the chief's machine, or the conversations of
a ChatGPT or Claude export as one project. A project is the root of the git
repository a conversation worked in, else its folder; linked git worktrees,
Downloads and the home folder are none. Each project becomes a hamlet named
for it and working on its folder, or the chief's hamlet that works on it
already, and each of its conversations becomes an agent under its lead, as
the hamlet's tier has room, whose one conversation it is. A conversation is
brought in once per chief.

The window offers the projects once, when first-run setup ends on a realm
with no hamlet, and again from Settings, each with its newest [50]
conversations of the last [30] days. The chief picks which to bring in; the
newest [20] of git repositories of [3] conversations or more start picked.
A conversation keeps the words of the chief's messages and the answers, its
first message and the newest [200], and a notice says where it came from
and how many messages were left out. The other program's tool calls show by
tool and a short summary, and are never given to the model.

**C34.** The `read` tool gives an image file (PNG, JPEG, WebP or GIF) to the
model as an image, made to fit the model's limits (S30), when the model
takes images; otherwise it answers in words that the file is an image.

**C5.** An agent reflects after every [10] answered turns of a chief's
lines in its conversation, including retries, when a chief asks it to, and
on owed turns when the server stops in an orderly way. An agent ended with a
dream first reflects once more on any unreflected turns, or on a
conversation brought in (C35) and never reflected. Zero means the agent
reflects only when a chief asks it to. An automatic reflection is queued
behind inputs already waiting, and reflects on those turns too. A client's
end of input triggers no reflection.

When the count is nonzero, an orderly close submits owed reflections and
waits up to 120 s for those able to run. A running chief turn is suspended
promptly; its owed reflection stays queued. Unfinished work carries on at
the next start. A second stop signal closes at once.

The agent is asked to keep in its memory what is worth remembering and to
save or improve a skill for anything likely to come up again. The worktree
reaches its profile by a dream (H43).

## What is recorded and shown

**C30.** A turn shows as it happens: its text and reasoning, each tool call
with its name, its arguments cut to 200 characters and, for an edit, the
change, then the call's end with its exit code, its time and the first line
of a failure, and the notices meant only for people. Notifications,
requests and responses show their writer by address with its name beside it
(`agent#3 (Mira)`), `hand`, or `worker#id`; once an agent is erased, by
address alone. They show as agents' words, without the model's heading,
never as a chief's words that can be taken back.

**C31.** Every model call, including each summarising call, records the
model, where it was served, the input, cached, output and reasoning tokens,
the provider's own charge when available, the time to the first token, the
whole time, and why it ended. Failed and aborted attempts are recorded too.
Charges follow H8 and belong to the hamlet or the realm, the agent and the
task, even after a rewind.

**C32.** No credential appears in a conversation as it is kept or shown.
Known credentials are scrubbed from chief inputs, tool results (including
structured output), streamed and spilled command output, protocol messages,
the oversight record, and an assistant's own text, reasoning and tool-call
arguments.
