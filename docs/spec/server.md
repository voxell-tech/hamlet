# The server: where agents run

Agents run on a server and keep working whether or not a window is open; the
window is how chiefs see and steer them. The decisions behind these
requirements are in [`docs/adr/`](../adr/).

## The server and the window

**S1.** Agents run on a server, apart from the window. Closing every window
stops no agent; stopping the server stops them all.

**S2.** One server keeps one realm, and a realm is claimed by one server at a
time. The harnesses a server runs are part of its claim: each opens only
its own store, only while that server runs.

**S21.** A harness goes on working only while its server runs: one whose
connection to the server closes stops its work and exits, so no agent runs
without its server.

**S3.** Nothing an agent does is shown before it is kept. A server that
stops mid-turn, in an orderly way or not, carries the work on from where it
stopped when it opens again. A tool call cut off in the middle runs again
only if running it twice is safe; otherwise the agent is told it was cut
off, with the output it had.

**S4.** A window connects to a server by its address and a token, the same
on one machine as across a network. A server refuses a connection without
the token.

**S5.** A window shows what the server sends and holds no rules of its own:
the state of the realm (the Hand, hamlets, scholars, living and ended
agents, roads, budgets, the shared library, removed hamlets and what waits)
and its changes, the live events of turns (C30), and conversations on
request, including read-only letters and questions between agents (C22). A
window that joins late or connects again starts from the state as it is now.
A window always matches what the server last sent, including a skill leaving
the library, a hamlet closing or coming back, and an agent leaving.

**S6.** Everything a chief does goes to the server: typed lines and
commands, answers to what waits, stopping, taking a conversation back, where
a chief sets a town or a house down in the world, and edits to the charter,
characters, memory, limits and settings.

**S15.** Local chat links and pictures resolve on the window's machine,
from existing absolute paths or paths under the received work folder.
A path inside the server's realm folder is never a link, even when
reached through a symbolic link.

## Credentials

**S7.** The server keeps its credentials (model keys, subscription logins,
the connection token, the remote workspace's token) in one file of the
realm that only its owner can read, written whole on each change. No
credential the server keeps is ever in a store of the realm, in a
conversation (C32), or in a sandbox. The chief's own
credentials on the machine are covered by S16.

**S8.** A chief can reach any model provider pi-ai supports with an API key.
A chief can also sign in with a subscription where the provider offers that
to open-source apps: "Sign in with ChatGPT". Hamlet signs in as itself, under
its own name and an installation ID of its own, and the server refreshes the
login on its own.

## Where tools run

**S12.** Tools reach files and processes only through one interface, so
where they run can change without the agent noticing. On a chief's own
machine they run in the sandbox of C18: bubblewrap on Linux, Seatbelt on
macOS. Windows has only insecure mode for now, and Linux and macOS have no
insecure mode.

## The realm's records and effects

**S26.** Everything the server keeps about who exists, who answers to whom,
roads, conversations, waits and spend is in one database, and a scholar's
knowledge is in git (ADR 0011). Every change to the database is one
transaction and is checked in one place before it is made; a change that is
refused leaves nothing. What the clients see again after a change is what that
transaction touched.

**S27.** A change that needs an effect after it commits, such as making a
repository, checking out or committing a lodge, merging or erasing, keeps the
effect as a job in the same transaction. Jobs run after the commit, on a
thread of their own, and again after a crash until they are done, and each is
safe to run twice (ADR 0012). A repository or a lodge is changed only by the
server.

**S28.** The client protocol, the runtime protocol, the workspace protocol and
the realm's database each carry a version in semver, and all stay at 0.1.0
until Hamlet's first release (ADR 0013). A peer, or a database, is taken on
only when its version is compatible with the server's by semver's caret; one
that is not is refused in words.

## Harnesses

An agent's conversations run in a harness, apart from the server that
keeps the rules (ADR 0001).

**S17.** An agent has a harness for its new conversations. A conversation
stays with the harness, and the version of it, it began in, for as long as
it is kept. Changing an agent's harness changes only its later
conversations, and keeps its identity, lodge, roads and permissions.

**S18.** The server allows every tool call and every change to the record
when it is carried out, under the permissions, waits, limits and budget as
they are then. A permission refused, taken back or lapsed by then allows
nothing, even for a call a harness began before a restart. Taking a
permission back undoes nothing it already allowed.

**S19.** A change to the record made by a tool call, and its charge, happen
once however often the call runs; running it again gives back its first
result. The same call asked for again with different arguments is refused.

**S20.** A model call's usage is charged once, however often its harness
reports it. Work that the budget decides does not go on while usage a
harness recorded is not yet charged.

**S22.** A harness that fails stops only the conversations it runs. The
rest of the realm goes on, and work waiting on an answer from those
conversations waits. They carry on from where they stopped when the
harness runs again (S3).

**S23.** A typed line, letter, question, reply, reflection, worker's task or
dream is taken into its conversation once, in order, however often it is
delivered. A stopped turn stays stopped through a restart: nothing it had
queued is taken in again.

**S24.** A harness is used only if the server's permissions and budget
decide every tool call and model call it makes. What a harness cannot do,
such as taking a conversation back, is refused in words for a chief, never
done another way.

**S25.** A part of a realm that is missing, or belongs to another realm, is
an error, never replaced by an empty one.

## Working on Hamlet

**S13.** The `hamlet` command is a Rust binary in `crates/hamlet_cli`,
run with `cargo run -p hamlet_cli -- <command>` or installed with
`cargo xtask install`. It manages the server and window as separate
processes and uses the client protocol for terminal operations.
The server needs Node for the pi harness. `HAMLET_HOME` names the realm's
folder; development uses a scratch folder.
The window's executable is `hamlet_app`; the app is called Hamlet.
Bare `hamlet` serves the realm in the background, or reuses its running
server, and opens a window. Closing the window or terminal stops no agent.
`hamlet stop` stops the server; `hamlet status` reports it.

| Command | Does |
|---|---|
| `hamlet` | Serves the realm in the background or reuses its server, and opens a window |
| `stop` | Stops the realm's server and waits for it to close in order |
| `status` | Reports whether the realm's server runs |
| `server` | Serves the realm; stopping it closes in order, and a second stop signal exits at once |
| `chat [--to <hand\|hamlet#number\|mayor@hamlet>]` | Chats with the Hand or the named agent and answers what waits, with no window; end of input waits for that client's submissions and leaves the server running |
| `dev [--port <n>] [--faux <script.json>] [--home <dir>]` | Builds the checkout's server and window unless `HAMLET_SERVER_BIN` and `HAMLET_APP` select them respectively, serves a scratch realm and opens its window; closing the window leaves the server running, and stopping the command closes both in order |
| `window` | Opens a window connected to a running server |
| `edit <charter\|limits\|settings>`, `edit character <scholar#id>`, `edit memory <scholar#id\|hamlet#number> [path]` | Opens the chief's document in `$EDITOR` and saves it through the server |
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
ADR 0007). Scholars' repositories are another matter: the server alone
writes them, and no git program is run on them (ADR 0011).
