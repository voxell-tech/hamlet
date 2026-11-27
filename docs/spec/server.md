# The server: where agents run

Agents run on a server and keep working whether or not a window is open; the
window is how chiefs see and steer them. How the server keeps the realm, in
actors, is in [`actor-runtime.md`](actor-runtime.md). The decisions behind
these requirements are in [`docs/adr/`](../adr/).

## The server and the window

**S1.** Agents run on a server, apart from the window. Closing every window
stops no agent; stopping the server stops them all.

**S2.** One server keeps one realm, and a realm is claimed by one server at a
time. The Node host a server runs is part of its claim: it opens the agents'
files only while that server runs (A6).

**S21.** The Node host goes on working only while its server runs: when its
link to the server closes, it stops its work and exits, so no agent runs
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
the state of the realm (the Hand, hamlets, profiles, living and ended
agents, roads, budgets, the shared library, removed hamlets and what waits)
and its changes, the live events of turns (C30), and conversations on
request, including read-only letters and questions between agents (C22). A
window that joins late or connects again starts from the state as it is now.
A window always matches what the server last sent, including a skill leaving
the library, a hamlet closing or coming back, and an agent leaving.

**S6.** Everything a chief does goes to the server: typed lines and
commands, answers to what waits, stopping, taking a conversation back, where
a chief sets a hamlet or a house down in the world, and edits to the charter,
characters, memory, limits and settings.

**S15.** Local chat links and pictures resolve on the window's machine,
from existing absolute paths or paths under the received work folder.
A path inside the server's realm folder is never a link, even when
reached through a symbolic link.

**S29.** An image a chief sends is kept once in the realm's folder, under the
SHA-256 of its bytes, however often it is sent. A client sends an image
once, in parts, and then names it by that id on each line; the conversation
keeps ids and never bytes, and a client fetches an image by its id to show
it. Only a PNG, JPEG, WebP or GIF of at most 20 MiB is kept (C33). The
server also keeps a small copy of each image, made once when a client first
asks for it, which the transcript shows; the image itself is read only when a
chief opens it. An image given to an agent is copied into that agent's own
folder, inside its sandbox's reach; the realm's own store of images is never
in any agent's reach. An image no kept input, transcript item or
agent's folder names, and that was last touched a day ago, is removed, which
includes uploads never sent with a line.

A file a chief attaches (C33) is kept the same way in the realm's store, of
any kind and at most 256 MiB, and a client names it by its id and the name the
agent finds it under. Each file named by a line is copied into the `inbox/` of
the agent's filesystem; the realm's store of files is never in any agent's reach. A
file named by no kept input, transcript item or tool result, last touched a day
ago, is removed.

**S30.** The server knows from its listing of a model whether it takes images,
and its limits: the width, height and bytes of one image and how many a message
holds. A chief can say that a model takes images and set its limits, as a model
of a custom endpoint, whose listing says none, needs; the server keeps what the
chief said until they clear it, and a later listing leaves it alone. It refuses
a line with images to a model that takes none, and gives a model each image
only once made to fit within its limits.

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
macOS. Windows has only insecure mode, and Linux and macOS have no insecure
mode (ADR 0008).

## The realm's records and effects

**S26.** What the realm keeps about who exists, who answers to whom, roads,
letters, waits and spend is in the realm actor's file, each agent's
conversations are in that agent's file, and a profile's knowledge is in git
(A1, ADR 0011). Every change to an actor's file is one transaction and is
checked in one place before it is made; a change that is refused leaves
nothing. What the clients see again after a change is what that transaction
touched.

**S27.** A change that needs an effect in another actor, such as making a
branch, filling an agent's filesystem, merging or erasing, begins a row of steps in
the same transaction. The steps run after the commit, and again after a
restart until they are done, and each is safe to run twice (A12). A
profile's repository is changed only by its profile's actor, and an agent's filesystem only
by its agent.

**S28.** The client protocol, the link between the two hosts and the
workspace protocol each carry a version in semver, and all stay at 0.1.0
until Hamlet's first release (ADR 0013). A peer is taken on only when its
version is compatible by semver's caret; one that is not is refused in
words.

## Agents' turns

An agent's conversations run in its agent actor, in the Node host (A1, A2).

**S18.** The realm and the workspace allow every tool call and every change
to the record when it is carried out, under the permissions, waits, limits
and budget as they are then. A permission refused, taken back or lapsed by
then allows nothing, even for a call a turn began before a restart. Taking a
permission back undoes nothing it already allowed.

**S19.** A change to the record made by a tool call, and its charge, happen
once however often the call runs; running it again gives back its first
result. The same call asked for again with different arguments is refused.

**S20.** A model call's usage is charged once, however often its agent
reports it.

**S22.** When the Node host fails, only the agents stop. The rest of the
realm goes on, and a call to an agent fails until the Node host runs again.
The agents' conversations carry on from where they stopped (S3).

**S23.** A typed line, letter, question, reply, reflection, worker's task or
dream is taken into its conversation once, in order, however often it is
delivered. A stopped turn stays stopped through a restart: nothing it had
queued is taken in again.

## Working on Hamlet

**S13.** The `hamlet` command is a Rust binary in `crates/hamlet_cli`,
run with `cargo run -p hamlet_cli -- <command>` or installed with
`cargo xtask install`. It manages the server and window as separate
processes and uses the client protocol for terminal operations.
The server needs Node for the Node host. `HAMLET_HOME` names the realm's
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
| `server [--port <n>] [--faux <script.json>] [--insecure]` | Serves the realm in the foreground; stopping it closes in order |
| `chat [--to <hand\|hire#id\|mayor@hamlet>]` | Chats with the Hand or the named agent and answers what waits, with no window; end of input waits for that client's submissions and leaves the server running |
| `dev [--port <n>] [--faux <script.json>] [--home <dir>] [--hot]` | Builds the checkout's server and window unless `HAMLET_SERVER_BIN` and `HAMLET_APP` select them respectively, serves a scratch realm and opens its window; closing the window leaves the server running, and stopping the command closes both in order; `--hot` builds a window that reloads saved shaders and tuning files |
| `window` | Opens a window connected to a running server |
| `edit <charter\|limits\|settings>` (`limits --hamlet <id>` for a hamlet's), `edit character <profile#id>`, `edit memory <profile#id\|hire#id> [path]` | Opens the chief's document in `$EDITOR` and saves it through the server |
| `login <provider> [--key]` | Gives a key, or follows an offered sign-in |
| `models [try [model]\|refresh]` | Lists models, tries a one-word hello, or refreshes the list |
| `models images <model> [off \| [--width <px>] [--height <px>] [--mb <n>] [--per-line <n>]]` | Says that a model takes images and sets its limits, or clears them (S30) |

**S14.** No test contacts a model server or the internet, except those a
developer asks for by name.

## Git on the chief's machine

**S16.** In the folder (C20), agents use the chief's own git and whatever
credentials the chief's machine has, which their commands can read.
Reaching a git host needs the network like any other command (C19, H11).
Agents may write hooks and config, including in nested repositories; the
chief accepts that these may run under the chief's own git later (C18,
ADR 0007). Profiles' repositories are another matter: the server alone
writes them, and no git program is run on them (ADR 0011).
