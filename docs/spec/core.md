# The core: one agent

An agent, its house, what it is told, the tools it works with, what they may
reach, and the conversations it keeps. The society around it is in
[`hamlet.md`](hamlet.md).

## The agent and its house

**C8.** An agent has a house of its own: a workspace holding its notes
(`NOTES.md`) and its skills (one folder each, with a `SKILL.md`), an inbox
for what a chief hands it, its character, and its conversations.

**C9.** An agent is its number in its hamlet's record, kept for its whole
life: through removal, merging and restoring. A later agent of the same name
has another number, and inherits nothing of the earlier one.

**C10.** A name of an agent is lowercase letters and hyphens, at most 24 of
them. A name a model writes in another form is put in that form
(`FelineBiologist` becomes `feline-biologist`). `mayor`, `founder` and the
librarian's name are no agent's.

## What an agent is told

**C11.** Before every model call, the agent's system prompt is built again
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
| The hamlet | Who lives there, how it is run, the latest decisions (H17) | The record |
| The realm | The other hamlets | The record |
| The budget | What the hamlet and the agent have spent (H8) | The record |

A part changed by anyone counts from the agent's next step. The budget's
words are fixed for the length of a turn, so the prompt does not change with
every call.

**C12.** The default charter belongs to the app, and a home that never
edited its charter gets each later default. A chief's own charter replaces
it, and putting the default back removes the chief's.

**C13.** An agent cannot write its own character, by any tool. The mayor may
give one when it creates an agent, and has no tool that changes one after.

**C14.** A chief can read an agent's whole prompt as it would be sent now.

**C15.** A tool that fails to load is named in the prompt of every agent
that would have had it.

## Tools

**C16.** Every agent has these tools:

| Tool | Does |
|---|---|
| Shell | Runs a command in the workspace, or in the hamlet's folder when it has one. Says whether the command needs the network |
| Read | Gives a text file with line numbers, up to 400 lines at a time |
| Search | Finds a regular expression in the files under a folder |
| Find | Lists a folder, or finds files by name under it |
| Edit | Replaces one exact occurrence of a text in a file; with an empty text to replace and no file, creates the file |

The hamlet's own tools, offered by role, are in [`hamlet.md`](hamlet.md).

**C17.** A chief can add tools by hand: a program with a manifest, in a
folder of the home that agents can read and never write. Each is offered to
every agent, and runs under the same sandbox and the same gate as the shell.

## What tools reach

**C18.** Tools run in a sandbox. An agent's tools write only in its own
workspace and in the folder its hamlet works on. A command can read the
rest of the machine, so a project's own tools run, but never the home, and
never writes `.git/hooks` or `.git/config`. Where no sandbox exists, tools
run only when the chief starts Hamlet saying so (`--insecure`).

**C19.** Tools have no network unless a chief turns it on for every agent or
grants it to one (H11). A command that needs the network says so, and is
held for a chief while its agent has none.

**C20.** A hamlet works on at most one folder at a time, and the folders it
worked on before are remembered. Every agent of the hamlet then runs its
commands there and may edit its files, and nothing of Hamlet's is put in it.
The agent's notes, skills and conversations stay in its house.

**C21.** A hamlet that works on a folder is in one of two modes:

- **Manual**, the mode it starts in: each edit and each command waits for a
  chief. Reading is free.
- **Auto**: agents edit and run without asking, and the sandbox is the
  guard.

**C7.** A hamlet answers to its chiefs: the human who founded it, or, for a
hamlet founded by a vote, the chiefs of the hamlet that voted. Every tool
call of its agents passes a gate before it runs, which lets it run, refuses
it, or holds it for a chief (H11).

## Conversations

**C22.** Every conversation is with one agent and is kept. An agent has a
conversation with each chief who talks to it, one with each agent that
writes it letters, and one with each agent that asks it something and
waits (H30).

**C23.** A conversation answers one message at a time. Messages that arrive
while it works wait in order.

**C24.** A chief can start a new conversation with an agent, which keeps the
old one in history, and go back to any earlier one.

**C25.** A chief can stop an agent's running turn, which also drops what it
had queued. The conversation stays whole: the turn shows as stopped.

**C26.** A chief can carry on a turn that failed or was stopped, from where
it ended, as a new task.

**C27.** A chief can take a conversation back to before one of its
messages. What waited for a chief from the part taken out lapses.

**C28.** A conversation that would fill more than 80% of its model's window
is summarised, and the summary takes its place; a message not yet answered
is kept as it is. A reply cut off for lack of room has the conversation
summarised and is asked for once more. With no room even then, the turn
fails, and says to start a new conversation or choose a model that holds
more.

**C29.** A model that answers with nothing fails the turn, and says to send
the message again.

**C5.** An agent reflects: after every [10] turns of its conversations with
chiefs (0 never), when a chief asks it to, and, for turns it has not
reflected on, when the server stops in an orderly way. It is asked to keep
in its notes what is worth remembering about its users and its lessons, and
to save or improve a skill for anything likely to come up again. An agent
other than the mayor is also asked, in one line, whether anything about how
its hamlet is run got in its way.

## What is recorded and shown

**C30.** A turn is shown as it happens: its text and reasoning, each tool
call with its name, its arguments cut to 200 characters and, for an edit,
the change, then the call's end with its exit code, its time and the first
line of a failure, and the notices meant only for people.

**C31.** Every model call records the model, where it was served, the input,
cached, output and reasoning tokens, the provider's own charge, the time to
the first token, the whole time, and why it ended.

**C32.** No credential appears in a conversation as it is kept or shown.
