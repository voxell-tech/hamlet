# What an agent is told

The system prompt built before every model request, where each part comes
from, and how the chief reads and edits it. The server builds it;
`hamlet_core` is only the retained Rust runtime.

## The parts, in order

| Part | Where it comes from | Who changes it |
|---|---|---|
| The charter | The home's stored charter, else `server/assets/charter.md` | A chief |
| The machine | The sandbox, network, model, house and any tools that failed to load | The settings |
| The project | The hamlet's work folder, with its `AGENTS.md` or `CLAUDE.md` | A chief |
| The character | The agent's stored character, at most 4000 characters | A chief; the mayor once at creation |
| The notes | `NOTES.md` in its house, at most 8000 characters | The agent |
| The skills | Its house's skills, descriptions and usage | The agent |
| The library | Skills shared by the home, beginning with the starters | The librarian |
| The hamlet | Its role, agents, roads and recent decisions | The record |
| The realm | Other hamlets, their mayors and pending agreements, or the agent's roads out | The realm's record |
| The budget | The hamlet's spend and the task's caps | The ledger and limits |

`server/src/prompt.ts` builds the first seven parts. `prompt/brief.ts`
adds the hamlet, `realm/founding.ts` the realm, and `spend/budget.ts` the budget.
The budget's words stay fixed for a turn; the other parts are built again
before each request. A realm with one hamlet has no realm section.

An errand is a one-off durable conversation. Ballots and the mayor's answer
use the agent's own prompt. Library advice, skill judging, merged notes and
the mayor's instruction review use only their supplied instructions.
A ballot has only `cast_ballot`; other errands have no tools.

## The charter and character

Neither is a file an agent can write. Both live in the home's storage and
are edited through the protocol. An unedited charter uses the shipped
default, so it follows later defaults. BACK TO THE DEFAULT clears the
chief's replacement after asking first.

The mayor may give a character in `create_agent`. It has no tool that
changes one afterwards, its own included. Houses are closed to other
agents, including the mayor; it asks an agent for what it needs.

Settings has the Charter page. The inspector's CHARACTER tab shows the
selected agent's character. EDIT opens a local temporary Markdown copy,
and each save sends an `edit` request to the server. Character filenames
name the hamlet and agent, so kept copies can be identified.

Closing the window polls the editor once more. While saves are
unconfirmed it keeps the window open and shows the recovery paths, with
KEEP OPEN and CLOSE WITH COPIES. A confirmed save allows the close to
finish. A refused or uncertain save is not silently repeated; inspect the
server document before applying a kept copy again. Closing stops no agent.

## The whole prompt

Prompt in an agent's right-click menu sends `read prompt`. The server uses
the same prompt sections as a model request, and the inspector displays
the returned text. The window reads no house or home files to build it.
