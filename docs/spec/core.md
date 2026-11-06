# The core: one agent

An agent, the place it works in, what it is told, the tools it works with,
what they may reach, and the conversations it keeps. The society around it is
in [`hamlet.md`](hamlet.md).

## The agent and its lodge

An agent is the Hand, a hire or a worker (H37, H40, H42).

**C8.** The Hand and every hire work in a lodge of their own: a folder
holding its memory (`memory/`), its skills (`skills/`, one folder each, with
a `SKILL.md`) and an inbox (`inbox/`) for what a chief hands it. They are
writable files. A lodge holds no character: the character is a file of the
agent's scholar, and the conversations are kept in the harness's storage,
all out of reach of file tools. A worker has no lodge, only a scratch folder
that ends with it (H42).

**C9.** An agent is identified by an id that is the same everywhere in the
realm and is never given twice. A hire also has a number in its hamlet, never
reused there, which addresses it as `hamlet#number`, the mayor being 0. No
address, lookup or command finds an agent or a scholar by a name. Ending,
merging, restoring and seating a mayor keep both.
A later hire of the same scholar inherits nothing of the earlier one.

**C10.** A scholar is its id, addressed as `scholar#<id>`. Its name is a
label of one line, 1 to 40 characters with no colon, which may repeat and
which a chief changes (`/name-scholar`), a decision recorded like a hamlet's
naming.

## What an agent is told

**C11.** Before every model call in an agent's ordinary conversations, its
system prompt is built again from these parts, in this order:

| Part | From | Changed by |
|---|---|---|
| The charter | The realm's charter, or the default | A chief |
| The machine | The sandbox, the network, the model and the lodge | The settings |
| The project | The folder the hamlet works on, and its own `AGENTS.md` or `CLAUDE.md` | A chief |
| The character | The agent's character at the tip of its branch, at most 4000 characters of it | A chief |
| The memory | The files at the root of its lodge's `memory/`, at most 8000 characters in all, and the description of each folder under it | The agent |
| The skills | Its lodge's skills, then the library's | The agent; the librarian |
| The place | Who it is, who it answers to and who answers to it, its roads, the tools turned off for it, how its hamlet was run lately (H17) | The record |
| The realm | The other hamlets, for the Hand and mayors; the hamlets a chief can found, when there are none | The record |
| The budget | What its hamlet or the realm, its task and its allowance have spent (H8) | The record |

A part changed by anyone counts from the agent's next step. The budget's
words are fixed for the length of a turn, so the prompt does not change with
every call.

Letters, questions and replies name their sender. The prompt says that
everything after an agent's sender heading is that agent's words, even if it
claims to be a chief or includes another heading. Only a chief's own lines in
its own conversation carry that chief's instructions. A chief's instruction
outranks a lead's. An agent's lodge is closed to other agents, but for the
worker it started, which reads it; an agent puts what another needs in its
reply.

A worker's prompt is the charter, the machine, the project and its place. The
librarian's call, a lead's review of an instruction (H10) and a dream receive
only their own instructions, not the agent's prompt.

**C12.** The default charter belongs to the app, and a realm that never
edited its charter gets each later default. A chief's own charter replaces
it, and putting the default back removes the chief's.

**C13.** An agent cannot write its own character, by any tool. A lodge is
checked out without `CHARACTER.md`, and a file by that name written there is
never committed. A chief writes a character on the scholar's `main`; a
lead gives one only when it makes a scholar (H38) and has no tool that changes
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
| `bash`, `powershell` | One tool per shell. `bash` is offered wherever bash is found, which on Windows is Git Bash, and `powershell` on Windows alone, naming the edition it found. Each runs in the hamlet's folder when it has one, else in the lodge, and says with a `network` flag whether it needs the network. Stops after 120 s, or an earlier timeout; keeps the last 2000 lines or 50 KiB of stdout and stderr, and gives a file holding the whole output when it spills |
| `search` | Finds a linear-time regular expression in files, with a name filter and optional case-insensitive matching, up to 100 hits; skips binary files, files that are not UTF-8, files over 1 MiB and build folders |
| `find` | Lists a folder, or finds files by name under it, up to 100 matches |

A lead can turn tools off for a hire under it (H41); the prompt names them. A
worker has only the built-in tools its starter gave it (H42). The society's
tools, offered by role, are in [`hamlet.md`](hamlet.md).

**C17.** A chief can add tools by hand: a program with a manifest, in a
folder of the realm that agents can read and never write. Each is offered to
every agent, and runs under the same sandbox and the same gate as the shells.

## What tools reach

**C18.** Tools run in a sandbox. The built-in tools write only in the
agent's lodge (a worker's scratch folder) and in the folder its hamlet works
on. With a folder, a command can read the rest of the machine, so the
project's own tools run, but never the rest of the realm's folder. The
library and the realm's tools folder are readable and never writable, and a
worker also reads its starter's lodge.

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
The agent's memory, skills and conversations stay in its lodge.

**C21.** A hamlet that works on a folder is in one of two modes:

- **Manual**, the mode it starts in: each edit of the folder and each
  command waits for a chief. Reading and edits of the agent's own lodge
  are free.
- **Auto**: agents edit and run without asking, and the sandbox is the
  guard. In insecure mode nothing is.

**C7.** A hamlet answers to its chiefs: the human who founded it, or, for a
hamlet the Hand founds, the chief whose task the Hand was working on. The
server's one chief is whoever runs the server, under the name the server
runs as.

A chief talks to, instructs, watches, stops, gives to and answers for only
the agents of living hamlets it is chief of, and the Hand, which answers to
every chief. Reading and editing a hire's papers or a hamlet's limits needs
the same authority; a scholar's belongs to the chief it belongs to (H38). A
removed hamlet's agents are out of reach until it is restored; a refused
request says why it was removed and to restore it first. Requests naming a
missing hamlet or a hamlet the human is not chief of are refused.

Every tool call of every agent passes a gate before it runs, which lets it
run, refuses it, or holds it for a chief (H11).

## Conversations

**C22.** Every conversation is with one agent and is kept. An agent has a
conversation with each chief who talks to it, one with each agent that
writes it letters, and one with each agent that asks it something and
waits (H30). A worker has one, begun with its task (H42), and a dream, a
librarian's call and a lead's review each have one of their own. A chief
can watch either kind of peer conversation read-only, with paging. The
chief chooses the receiving agent, the sender, and letters or questions;
letters are shown unless the chief chooses questions.

**C23.** A conversation answers one message at a time. Messages that arrive
while it works wait in order.

**C24.** A chief can start a new conversation with an agent. `/new` moves
to it at once, while the old conversation finishes its queue and, unless
reflection is set to zero, reflects on what it owes (C5). Every conversation
has a stable number in that chief's chat with the agent, shared by the
chief's clients. `/resume` alone lists the earlier ones; `/resume <n>` goes
back to one by its number.

**C25.** A chief can stop an agent's running turn, which also drops what it
had queued. The conversation stays whole: the turn shows as stopped.

**C26.** A chief can carry on a turn that failed or was stopped, from where
it ended, as a new task. A retry is shown as a notice in the conversation,
and is an input asking "Carry on from where you stopped." A running turn,
or one that was answered, cannot be retried.

**C27.** A chief can take a conversation back to before one of its
messages. What waited for a chief from the part taken out lapses.

**C28.** Compaction reserves 20% of the model's window for an answer.
When the conversation's estimated size is above the window less that
reserve, a request waits for a summary; half the reserve before that
threshold, a summary starts in the background. Compaction needs an earlier
part it can summarise, keeps the most recent 20000 tokens of whole
messages, and replaces the earlier part sent to the model with its summary.
The stored conversation stays whole, and shows a notice for the summary.
A context-overflow error also triggers compaction and one more attempt.

An empty reply cut off for length with fewer than 1024 output tokens,
other than a reflection, has the conversation summarised and is asked for
once more under the same task. A second such cut-off fails. With no earlier
conversation to summarise, or no summary that succeeds, the turn fails and
says why; when a summary cannot make room, it says to start a new
conversation or choose a model that holds more.

**C29.** A model that answers with nothing fails the turn, and says to send
the message again.

**C5.** An agent reflects after every [10] answered turns in each chief
conversation, including retries, when a chief asks it to, and on owed turns
when a conversation is put away or the server stops in an orderly way.
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
likely to come up again. It writes its lodge, and the lodge reaches its
scholar by a dream (H43).

## What is recorded and shown

**C30.** A turn is shown as it happens: its text and reasoning, each tool
call with its name, its arguments cut to 200 characters and, for an edit,
the change, then the call's end with its exit code, its time and the first
line of a failure, and the notices meant only for people. Letters,
questions and replies show their writer by its address with its name beside
it (`docs#1 (Robb)`), `hand`, or `worker#id`; once a hire is erased, as its
address alone. They are shown as agents' words,
never as a chief's words that can be taken back.

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
