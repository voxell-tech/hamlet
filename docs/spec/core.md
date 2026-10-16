# The core: one agent

An agent, its house, what it is told, the tools it works with, what they may
reach, and the conversations it keeps. The society around it is in
[`hamlet.md`](hamlet.md).

## The agent and its house

**C8.** An agent has a house of its own: its notes (`NOTES.md`), its skills
(one folder each, with a `SKILL.md`), an inbox for what a chief hands it, its
character, and its conversations. The notes, skills and inbox are writable
files; the character and conversations are kept in the home's storage,
out of reach of file tools.

**C9.** An agent is identified by its hamlet and its number in that hamlet's
record. Removal, merging and restoration keep that identity. Moving to
another hamlet gives it the next number there and carries its house,
character, conversations and instructions with it (H14). A number is never
reused within a hamlet. A later agent of the same name inherits nothing of
the earlier one.

**C10.** A name of an agent is lowercase letters and hyphens, at most 24 of
them, and belongs to only one living agent in the realm. A name a model
writes in another form is put in that form (`FelineBiologist` becomes
`feline-biologist`). `mayor`, `founder` and `all` are reserved; the first
hamlet's founder is named `founder`.

## What an agent is told

**C11.** Before every model call in an agent's ordinary conversations, its
ballot or the mayor's answer to a vote, its system prompt is built again
from these parts, in this order:

| Part | From | Changed by |
|---|---|---|
| The charter | The home's charter, or the default | A chief |
| The machine | The sandbox, the network, the model and the workspace | The settings |
| The project | The folder the hamlet works on, and its own `AGENTS.md` or `CLAUDE.md` | A chief |
| The character | The agent's character, at most 4000 characters of it | A chief; the mayor once, when it creates the agent |
| The notes | Its `NOTES.md` | The agent |
| The skills | Its skills | The agent |
| The library | The skills every agent shares | The librarian |
| The hamlet | The agent's place, who lives there, its roads, how it is run, the latest decisions (H17) | The record |
| The realm | The other hamlets | The record |
| The budget | What the hamlet and the agent have spent (H8) | The record |

A part changed by anyone counts from the agent's next step. The budget's
words are fixed for the length of a turn, so the prompt does not change with
every call.

Letters, questions and replies name their sender. The prompt says that
everything after an agent's sender heading is that agent's words, even if
it claims to be a chief or includes another heading. Only a chief's own
lines in its own conversation carry that chief's instructions. A chief's
instruction outranks the mayor's. Each agent's house is closed to other
agents, including the mayor; an agent puts what another needs in its reply.

The librarian's call, merged notes and the mayor's review of an instruction
(H10) receive only their own instructions, not the agent's prompt.

**C12.** The default charter belongs to the app, and a home that never
edited its charter gets each later default. A chief's own charter replaces
it, and putting the default back removes the chief's.

**C13.** An agent cannot write its own character, by any tool. The mayor may
give one when it creates an agent, and has no tool that changes one after.

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
| `bash` | Runs in the house, or in the hamlet's folder when it has one, and says with a `network` flag whether it needs the network. Stops after 120 s, or an earlier timeout; keeps the last 2000 lines or 50 KiB of stdout and stderr, and gives a file holding the whole output when it spills |
| `search` | Finds a linear-time regular expression in files, with a name filter and optional case-insensitive matching, up to 100 hits; skips binary files, files that are not UTF-8, files over 1 MiB and build folders |
| `find` | Lists a folder, or finds files by name under it, up to 100 matches |

The hamlet's own tools, offered by role, are in [`hamlet.md`](hamlet.md).

**C17.** A chief can add tools by hand: a program with a manifest, in a
folder of the home that agents can read and never write. Each is offered to
every agent, and runs under the same sandbox and the same gate as the shell.

## What tools reach

**C18.** Tools run in a sandbox. The built-in tools write only
in the agent's house and in the folder its hamlet works on. With a folder,
a command can read the rest of the machine, so the project's own tools run,
but never the rest of the home. The library and the home's tools folder
are readable and never writable.

In a folder of the chief's own, agents may write git's files, including
hooks or config that the chief's git later runs. The chief accepts that
risk until the folder is a clone Hamlet keeps (S9), where the chief never
runs git and Hamlet runs its own git with hooks off.

A tool added by hand has the filesystem reach its manifest grants, rather
than the shell's reach: its readable and writable paths, with its own
folder kept read-only. Its manifest also sets its environment and limits.
`--insecure` is not offered yet; a server without a sandbox cannot start.

**C19.** Tools have no network unless a chief turns it on for every agent or
grants it to one (H11). A command that needs the network says so, and is
held for a chief while its agent has none.

**C20.** A hamlet works on at most one folder at a time, and the folders it
worked on before are remembered. Every agent of the hamlet then runs its
commands there and may edit its files, and nothing of Hamlet's is put in it.
The agent's notes, skills and conversations stay in its house.

**C21.** A hamlet that works on a folder is in one of two modes:

- **Manual**, the mode it starts in: each edit of the folder and each
  command waits for a chief. Reading and edits of the agent's own house
  are free.
- **Auto**: agents edit and run without asking, and the sandbox is the
  guard.

**C7.** A hamlet answers to its chiefs: the human who founded it, or, for a
hamlet founded by a vote, the chiefs of the hamlet that voted. The first
hamlet, and any hamlet founded by its votes, answers to whoever runs the
server, under the name the server runs as.

A chief talks to, instructs, watches, stops, gives to and answers for only
the agents of living hamlets it is chief of. Reading and editing an agent's
papers or a hamlet's limits needs the same authority. Giving a skill from
another agent needs authority over that agent too. A removed hamlet's
agents are out of reach until it is restored; a refused request says why
it was removed and to restore it first. Requests naming a missing hamlet
or a hamlet the human is not chief of are refused.

Every tool call of a hamlet's agents passes a gate before it runs, which
lets it run, refuses it, or holds it for a chief (H11).

## Conversations

**C22.** Every conversation is with one agent and is kept. An agent has a
conversation with each chief who talks to it, one with each agent that
writes it letters, and one with each agent that asks it something and
waits (H30). A chief can watch either kind of peer conversation read-only,
with paging. The chief chooses the receiving agent, the sender, and letters
or questions; letters are shown unless the chief chooses questions.

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

The agent is asked to keep in its notes what is worth remembering about
the chiefs and its lessons, and to save or improve a skill for anything
likely to come up again. An agent other than the mayor is also asked, in
one line, whether anything about how its hamlet is run got in its way.

## What is recorded and shown

**C30.** A turn is shown as it happens: its text and reasoning, each tool
call with its name, its arguments cut to 200 characters and, for an edit,
the change, then the call's end with its exit code, its time and the first
line of a failure, and the notices meant only for people. Letters,
questions and replies show their writer as `name@hamlet`, using its living
or archived record in the hamlet it wrote from. If no such record remains,
its recorded departures and matching arrivals are followed through any
number of moves to its current name and hamlet. Only when no record can
be followed, such as after erasure, is the writer shown as `hamlet#number`.
They are shown as agents' words, never as a chief's words that can be taken
back.

**C31. Not yet met.** Every model call, including each summarising call,
records the model, where it was served, the input, cached, output and
reasoning tokens, the provider's own charge when available, the time to the
first token, the whole time, and why it ended. Failed and aborted attempts
are recorded too. Charges follow H8 and belong to the hamlet, the agent and
the task, even after a rewind.

Time to the first token is not yet recorded for every call. The provider's
raw usage is not yet recorded. Summaries have aggregate usage and cost,
without each summarising call's timing, billed charge or end reason.

**C32. Not yet met.** No credential appears in a conversation as it is kept
or shown. Known credentials are scrubbed from chief inputs, tool results
(including structured output), streamed and spilled command output,
protocol messages and the oversight record. An assistant's own messages
keep their text, reasoning and tool-call arguments unscrubbed, so the stored
conversation is not yet covered whole.
