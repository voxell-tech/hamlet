# The server: where agents run

Agents run on a server and keep working whether or not a window is open; the
window is how chiefs see and steer them. These requirements are new with the
rebuild. The decisions behind them are in [`docs/adr/`](../adr/).

## The server and the window

**S1.** Agents run on a server, apart from the window. Closing every window
stops no agent; stopping the server stops them all.

**S2.** One server keeps one home, and only one process opens a home at a
time.

**S3.** Nothing an agent does is shown before it is kept. A server that
stops mid-turn, in an orderly way or not, carries the work on from where it
stopped when it opens again. A tool call cut off in the middle runs again
only if running it twice is safe; otherwise the agent is told it was cut
off, with the output it had.

**S4.** A window connects to a server by its address and a token, the same
on one machine as across a network. A server refuses a connection without
the token.

**S5.** A window shows what the server sends and holds no rules of its own:
the state of the realm (hamlets, living and departed agents, roads,
budgets, votes, the shared library, closed hamlets and what waits) and its
changes, the live events of turns (C30), and conversations on request,
including read-only letters and questions between agents (C22). A window
that joins late or connects again starts from the state as it is now.
A window always matches what the server last sent, including a skill
leaving the library, a hamlet closing or coming back, and an agent leaving.

**S6.** Everything a chief does goes to the server: typed lines and
commands, answers to what waits, stopping, taking a conversation back, the
hand's deeds in the world, and edits to the charter, characters, notes,
limits and settings.

**S15.** Local chat links and pictures resolve on the window's machine,
from existing absolute paths or paths under the received work folder.
A path inside the server's houses folder is never a link, even when
reached through a symbolic link.

## Credentials

**S7.** The server keeps its credentials (model keys, subscription logins,
the connection token) in one file of the home that only its owner can read,
written whole on each change. No credential the server keeps is ever in the
home's storage, in a conversation (C32), or in a sandbox. The chief's own
credentials on the machine are covered by S16.

**S8.** A chief can reach any model provider pi-ai supports with an API key.
A chief can also sign in with a subscription where the provider offers that
to open-source apps: "Sign in with ChatGPT". Hamlet signs in as itself, under
its own name and an installation ID of its own, and the server refreshes the
login on its own.

## The clone and proxy

**S9. Dropped.** The folder is the chief's own, not a clone Hamlet keeps (ADR 0008).

**S10. Dropped.** Agents use the chief's own git and credentials, not a proxy (ADR 0008).

**S11. Dropped.** Reaching a git host needs the network (ADR 0008).

## Where tools run

**S12.** Tools reach files and processes only through one interface, so
where they run can change without the agent noticing. On a chief's own
machine they run in the sandbox of C18: bubblewrap on Linux, Seatbelt on
macOS.

## Working on Hamlet

**S13.** The `hamlet` command is installed with `npm link` in `server/`,
with the Node version in `.nvmrc`; `npm run hamlet -- <command>` runs it
there too. `HAMLET_HOME` names the home; development uses a scratch folder.
The window's executable is `hamlet-app`; the app is called Hamlet.
Bare `hamlet` serves the home in the background, or reuses its running
server, and opens a window. Closing the window or terminal stops no agent.
`hamlet stop` stops the server; `hamlet status` reports it.

| Command | Does |
|---|---|
| `hamlet` | Serves the home in the background or reuses its server, and opens a window |
| `stop` | Stops the home's server and waits for it to close in order |
| `status` | Reports whether the home's server runs |
| `server` | Serves the home; stopping it closes in order, and a second stop signal exits at once |
| `chat [--to <agent>]` | Chats with the mayor or the named agent and answers what waits, with no window; end of input waits for that client's submissions and leaves the server running |
| `dev [--port <n>] [--faux <script.json>] [--home <dir>]` | Serves a scratch home and opens its window; closing the window leaves the server running, and stopping the command closes both in order |
| `window` | Opens a window connected to a running server |
| `edit <charter\|character\|notes\|limits\|settings>` | Opens the chief's document in `$EDITOR` and saves it through the server |
| `login <provider> [--key]` | Gives a key, or follows an offered sign-in |
| `models [try [model]\|refresh]` | Lists models, tries a one-word hello, or refreshes the list |

**S14.** No test contacts a model server or the internet, except those a
developer asks for by name.

## Git on the chief's machine

**S16.** In the folder (C20), agents use the chief's own git and whatever
credentials the chief's machine has, which their commands can read.
Reaching a git host needs the network like any other command (C19, H11).
Agents may write hooks and config, including in nested repositories; the
chief accepts that these may run under the chief's own git later (C18,
ADR 0008).
