# The core: one agent

An agent, the place it works in, what it is told, the tools it works with,
what they may reach, and the conversations it keeps. The society around it is
in [`hamlet.md`](hamlet.md).

## The agent and its worktree

An agent is the Hand, an agent of a hamlet or a worker (H37, H40, H42).

**C8.** The Hand and every agent of a hamlet work in a worktree of their
own: a folder holding its memory (`memory/`), its skills (`skills/`, one
folder each, with a `SKILL.md`) and an inbox (`inbox/`) for what a chief
hands it. They are writable files. A worktree holds no character: the
character is a file of the agent's profile, and the conversations are kept
in the harness's storage, all out of reach of file tools. A worker has no
worktree, only a scratch folder that ends with it (H42).

**C9.** An agent is identified by an id that is the same everywhere in the
realm and is never given twice; an agent of a hamlet is addressed by it as
`agent#<id>`. No
address, lookup or command finds an agent, a profile or a hamlet by a name.
Ending, merging, restoring and setting a lead keep the id.
A later agent of the same profile inherits nothing of the earlier one.

**C10.** A profile is its id, addressed as `profile#<id>`. Its name is a
label of one line, 1 to 40 characters with no colon, which may repeat and
which a chief changes (`/name-profile`), a decision recorded like a hamlet's
naming.

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

A part changed by anyone counts from the agent's next step. No part follows
spending, so the prompt stays the same from one input to the next unless one
of its parts changed; an agent reads its budget with `read_budget` (H8, H35).

A notification, a request or a response reaches the model under a heading
in brackets that says what it is and names its sender, as
`[A notification from agent#3 (Mira)]`. The prompt says that everything
after an agent's heading is that agent's words, even if it claims to be a
chief; only a chief's own lines, which have no heading, carry that chief's
instructions. A chief's instruction outranks a parent's. An agent's
worktree is closed to other agents, but for the worker it started, which
reads it; an agent puts what another needs in its message.

A worker's prompt is the charter, the machine, the project and its place. The
librarian's call, a parent's review of an instruction (H10) and a dream receive
only their own instructions, not the agent's prompt.

**C12.** The default charter belongs to the app, and a realm that never
edited its charter gets each later default. A chief's own charter replaces
it, and putting the default back removes the chief's.

**C13.** An agent cannot write its own character, by any tool. A worktree is
checked out without `CHARACTER.md`, and a file by that name written there is
never committed. A chief writes a character on the profile's `main`; a
parent gives one only when it makes a profile (H38) and has no tool that changes
one after.

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
| `bash`, `powershell` | One tool per shell. `bash` is offered wherever bash is found, which on Windows is Git Bash, and `powershell` on Windows alone, naming the edition it found. Each runs in the hamlet's folder when it has one, else in the worktree, and says with a `network` flag whether it needs the network. Stops after 120 s, or an earlier timeout; keeps the last 2000 lines or 50 KiB of stdout and stderr, and gives a file holding the whole output when it spills |
| `search` | Finds a linear-time regular expression in files, with a name filter and optional case-insensitive matching, up to 100 hits; skips binary files, files that are not UTF-8, files over 1 MiB and build folders |
| `find` | Lists a folder, or finds files by name under it, up to 100 matches |

Each `bash` call's command and its output, as it comes, are also shown in
the agent's commands terminal, which the chief can watch and nobody types into.
A server an agent starts runs from `bash` with `&`, its output sent to a file the
agent reads later; it is kept until the server stops.

A parent can turn tools off for an agent under it (H41); the prompt names them. A
worker has only the built-in tools its starter gave it (H42). The society's
tools, offered by role, are in [`hamlet.md`](hamlet.md).

**C17.** A chief can add tools by hand: a program with a manifest, in a
folder of the realm that agents can read and never write. Each is offered to
every agent, and runs under the same sandbox and the same gate as the shells.

## What tools reach

**C18.** Tools run in a sandbox. The built-in tools write only in the
agent's worktree (a worker's scratch folder) and in the folder its hamlet
works on. With a folder, a command can read the rest of the machine, so the
project's own tools run, but never the rest of the realm's folder. The
library and the realm's tools folder are readable and never writable, and a
worker also reads its starter's worktree.

In a folder of the chief's own, agents may write git's files, including
hooks or config that the chief's git later runs, including in nested
repositories. The chief accepts that risk permanently (ADR 0007).

A tool added by hand has the filesystem reach its manifest grants, rather
than the shell's reach: its readable and writable paths, with its own
folder kept read-only. Its manifest also sets its environment and limits.
A server without a sandbox does not start, unless it is a Windows server
the chief starts in insecure mode (ADR 0008). Then commands and hand tools
reach whatever the account running the server can. The built-in file tools
keep the reach above, and the window warns for as long as it runs.

A command's leftovers, the processes left in its process group, end when
the server stops. Under bubblewrap they end with their command. Ending them
never signals another group: the command's own process stays unreaped while
its leftovers are kept, so no other group can take their group's id.

**C19.** Tools have no network unless a chief turns it on for every agent or
grants it to one (H11). A command that needs the network says so, and is
held for a chief while its agent has none. In insecure mode nothing
enforces this, and the switch decides only what waits for a chief.

**C20.** A hamlet works on at most one folder at a time, and the folders it
worked on before are remembered. Every agent of the hamlet then runs its
commands there and may edit its files, and nothing of Hamlet's is put in it.
The agent's memory, skills and conversations stay in its worktree.

**C21.** A hamlet that works on a folder is in one of two modes:

- **Manual**, the mode it starts in: each edit of the folder and each
  command waits for a chief. Reading and edits of the agent's own
  worktree are free.
- **Auto**: agents edit and run without asking, and the sandbox is the
  guard. In insecure mode nothing is.

The window names them by what they do: **Ask first** and **Don't ask**.

**C7.** A hamlet answers to its chiefs: the human who founded it, or, for a
hamlet the Hand founds, the chief whose task the Hand was working on. The
server's one chief is whoever runs the server, under the name the server
runs as.

A chief talks to, instructs, watches, stops, gives to and answers for only
the agents of living hamlets it is chief of, and the Hand, which answers to
every chief. Reading and editing an agent's papers or a hamlet's limits needs
the same authority; a profile's belongs to the chief it belongs to (H38). A
removed hamlet's agents are out of reach until it is restored; a refused
request says why it was removed and to restore it first. Requests naming a
missing hamlet or a hamlet the human is not chief of are refused.

Every tool call of every agent passes a gate before it runs, which lets it
run, refuses it, or holds it for a chief (H11).

## Conversations

**C22.** Every conversation is with one agent and is kept. The Hand and
each agent of a hamlet have one, shared by every chief and client, which
every input for them goes into: chiefs' lines, other agents' messages
(H30) and their workers' answers. A worker (H42), a dream, a librarian's
call and a parent's review each have their own. A message from another
agent shows its sender, never as a chief's words (C30).

**C23.** A conversation answers one input at a time. Inputs that arrive
meanwhile wait, shown last in run order: a chief's lines and retries
first, then agents' messages as they came. A chief can take back a waiting
line of their own. A notice, a merge's input (H5), an error response (H31)
and a compaction are not turns and are given at once.

**C24.** An agent's conversation goes on across tasks, and compaction keeps
it within the model's window (C28). A chief can have it compacted now
(`/compact`); the summary keeps who said what and what each still waits
for. Only a rewind gives the agent another conversation, a fork (C27).

**C25.** A chief can stop an agent's running turn, and the agent stays
stopped: what waits stays queued and shown, and no turn starts until the
chief sends a line, which runs first, or resumes it. A worker it started
goes on, and its answer queues. The conversation stays whole.

**C26.** A chief can carry on a turn that failed or was stopped, from where
it ended, as a new task. A retry is shown as a notice in the conversation,
and is an input asking "Carry on from where you stopped." A running turn,
or one that was answered, cannot be retried.

**C27.** A chief can take a conversation back to before one of its
messages. The agent's conversation is then a fork of it made at the entry
before that message, which keeps none of the session grants a chief gave
the one before (H11). What waited for a chief from the part taken out lapses.

**C28.** Compaction reserves 20% of the model's window for an answer.
When the conversation's estimated size is above the window less that
reserve, a request waits for a summary; half the reserve before that
threshold, a summary starts in the background. Compaction needs an earlier
part it can summarise, keeps the most recent 20000 tokens of whole
messages, and replaces the earlier part sent to the model with its summary.
The stored conversation stays whole, and shows the summary where it was
made, folded so that it opens on request. A context-overflow error also
triggers compaction and one more attempt.

An empty reply cut off for length with fewer than 1024 output tokens,
other than a reflection, has the conversation summarised and is asked for
once more under the same task. A second such cut-off fails. With no earlier
conversation to summarise, or no summary that succeeds, the turn fails and
says why; when a summary cannot make room, it says to choose a model that
holds more.

**C29.** A model that answers with nothing fails the turn, and says to send
the message again.

**C33.** A chief can attach files and folders of any kind to a line to an
agent, by dropping them on the window, pasting them (files copied in a file
manager, or a picture such as a screenshot) or choosing them in a dialog; the
terminal chat takes `--attach <path>` and `/attach <path>`, which attach at the
end of the line, and `#path` or `#"path with spaces"` in the words, which
attaches what the path names, if it exists, where the token stood and takes the
token out. In the window, `#` lists the files and folders of the hamlet's
folder, and the one picked is attached where the word stood. An attachment may sit anywhere in the line's words, at a byte offset
on a character boundary, and one that does not is refused; one without a place
goes after the words. A folder is attached with everything under it, links left
out. An image (PNG, JPEG, WebP
or GIF, animated or not) is also given to the agent's model beside the line, as
many as the model takes in a message [20 at most]; a line with images is
refused, and nothing is kept, when the model takes no images, and only images
are held back for such a model. Every file, an image too, is saved in the
`inbox/` of the agent's worktree, in its remote workspace when it works there,
and the line names where each file or folder is saved, a path the agent can
open, in the words where the attachment sits, and after them for one with no
place. The model is given the images in the order the line names them. A
file is not given to the model as content, so any model takes any file, and
the agent reads what it is able to. An agent can open only what was given to
it in its own conversations. One send holds at most [32 MiB and 1000 files] of
files other than images; a chief is asked to confirm a larger one before it is
sent, and none holds more than [256 MiB] in one file, [1 GiB] or [10000 files]
in all. The conversation shows each image with the line as a small copy, opens
the image itself when the chief asks to see it, and lists each file by its
name. A rewind takes the images and files back with the line, and the words,
images and files return to the chief's input together (C27).

**C35.** A chief can bring in the projects other programs (Claude
Code, Codex, pi) worked on, found on the chief's machine, or the
conversations of a ChatGPT or Claude export as one project. A project is
the root of the git repository a conversation worked in, else its
folder; linked git worktrees, Downloads and the home folder are none. Each
project becomes a hamlet named for it and working on its folder, or the
chief's hamlet that works on it already, and each of its conversations
brought in an agent under its lead, as the hamlet's tier has room, whose
one conversation it becomes, so the messages that reach the agent
later share its context. A conversation is brought in once
per chief. The window offers the projects once, when first-run setup
ends on a realm with no hamlet, and again from Settings, each with its
newest [50] conversations of the last [30] days; the chief picks which
to bring in, and the newest [20] of git repositories of [3]
conversations or more start picked. A
conversation keeps the words of the chief's messages and the answers,
its first message and the newest [200], and a notice says where it came
from and how many messages were left out. The other program's tool calls
are shown, each by its tool and a short summary, and never given to the
model.

**C34.** The `read` tool gives an image file (PNG, JPEG, WebP or GIF) to the
model as an image, made to fit the model's limits (S30), when the model takes
images; otherwise it answers in words that the file is an image.

**C5.** An agent reflects after every [10] answered turns of a chief's
lines in its conversation, including retries, when a chief asks it to, and
on owed turns when the server stops in an orderly way. An agent ended with a
dream first reflects once more on any unreflected turns, or on a
conversation brought in (C35) and never reflected.
Zero means the agent reflects only when a chief asks it to. An automatic
reflection is queued behind inputs already waiting, and reflects on those
turns too. A client's end of input triggers no reflection.

When the count is nonzero, an orderly close submits owed reflections and
waits up to 120 s for those able to run. A running chief turn is suspended
promptly; its owed reflection stays queued if the count is nonzero.
Unfinished work carries on at the next start. A second stop signal closes
at once.

The agent is asked to keep in its memory what is worth remembering about
the chiefs and its lessons, and to save or improve a skill for anything
likely to come up again. It writes its worktree, and the worktree reaches
its profile by a dream (H43).

## What is recorded and shown

**C30.** A turn is shown as it happens: its text and reasoning, each tool
call with its name, its arguments cut to 200 characters and, for an edit,
the change, then the call's end with its exit code, its time and the first
line of a failure, and the notices meant only for people. Notifications,
requests and responses show their writer by its address with its name beside
it (`agent#3 (Mira)`), `hand`, or `worker#id`; once an agent is erased, as its
address alone. They are shown as agents' words, without the heading the
model reads them under, never as a chief's words that can be taken back.

**C31.** Every model call, including each summarising call,
records the model, where it was served, the input, cached, output and
reasoning tokens, the provider's own charge when available, the time to the
first token, the whole time, and why it ended. Failed and aborted attempts
are recorded too. Charges follow H8 and belong to the hamlet or the realm,
the agent and the task, even after a rewind.

**C32.** No credential appears in a conversation as it is kept
or shown. Known credentials are scrubbed from chief inputs, tool results
(including structured output), streamed and spilled command output,
protocol messages, the oversight record, and an assistant's own text,
reasoning and tool-call arguments.
