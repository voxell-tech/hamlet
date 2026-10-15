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
the state of the realm (hamlets, agents, roads, budgets, what waits) and its
changes, the live events of turns (C30), and conversations on request. A
window that joins late or connects again starts from the state as it is
now.

**S6.** Everything a chief does goes to the server: typed lines and
commands, answers to what waits, stopping, taking a conversation back, the
hand's deeds in the world, and edits to the charter, characters, notes,
limits and settings.

## Credentials

**S7.** The server keeps every credential (model keys, subscription logins,
git tokens) in one file of the home that only its owner can read, written
whole on each change. No credential is ever in the home's storage, in a
conversation (C32), or in a sandbox.

**S8.** A chief can reach any model provider pi-ai supports with an API key.
A chief can also sign in with a subscription where the provider offers that
to open-source apps: "Sign in with ChatGPT". Hamlet signs in as itself, under
its own name and an installation ID of its own, and the server refreshes the
login on its own.

## The folder and git

**S9.** The folder a hamlet works on (C20) is a clone of a git repository,
which a chief gives by its URL and the server keeps in the home.

**S10.** Agents use ordinary git in the clone. Its remote is the server, and
the sandbox holds only a credential of the hamlet's own, which nothing but
the server accepts and which reaches only the hamlet's repository. The
server refuses deleting branches and pushing tags, lets any other branch be
pushed, and adds the chief's real token on the way to the git host. Force
pushes and the protection of particular branches are for rules the chief
sets on the git host. A chief's own push takes the same path.

**S11.** An agent's commands reach the server's git endpoint even while the
agent has no network.

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

| Command | Does |
|---|---|
| `server` | Serves the home; stopping it closes in order, and a second stop signal exits at once |
| `chat [--to <agent>]` | Chats with the mayor or the named agent and answers what waits, with no window; end of input waits for that client's submissions and leaves the server running |
| `dev [--port <n>] [--faux <script.json>] [--home <dir>]` | Serves a scratch home and opens its window; closing the window leaves the server running, and stopping the command closes both in order |
| `window` | Opens a window connected to a running server |
| `edit <charter\|character\|notes\|limits\|settings>` | Opens the chief's document in `$EDITOR` and saves it through the server |
| `login <provider> [--key]` | Gives a key, or follows an offered sign-in |
| `models [try [model]\|refresh]` | Lists models, tries a one-word hello, or refreshes the list |

**S14.** No test contacts a model server or the internet, except those a
developer asks for by name.
