# The `hamlet` command

The Rust crate `hamlet_cli` supplies `hamlet`. It manages the server and
the window as separate processes and uses [the client protocol](protocol.md) for
everything it asks the server. With the Node version in `harnesses/pi/.nvmrc`,
run `npm ci --prefix harnesses/pi`. From the repository root:

```sh
HAMLET_HOME=/tmp/hamlet-try cargo run -p hamlet_cli -- dev
```

This builds the server and window. The server starts its Node pi
harness as a child process. `cargo xtask install` installs all three binaries;
installing only `hamlet_cli` does not install the server or window.

The CLI selects `HAMLET_SERVER_BIN`, then a sibling `hamlet_server`, then
PATH. The server selects `HAMLET_PI_HARNESS` (the path to `src/main.ts`),
then sibling `pi/src/main.ts`, then the macOS app's
`Contents/Resources/pi/src/main.ts`, then the harness in its build checkout,
and refuses to start when the entry it selects does not exist.
`HAMLET_NODE` selects Node, else the `node` a release ships beside the
server, else Node on `PATH`. A release includes Node, the pi package and
its production dependencies, but not the optional ones that carry
Anthropic's Claude program, so pi offers Claude only where `npm ci`
installed them. `HAMLET_CLAUDE_EXECUTABLE` selects another Claude
program; pi still needs those packages to drive it. From source, Node is
installed separately.

Bare `hamlet` starts the realm's server in the background, or reuses its
running server, and opens a window. The realm's folder is `HAMLET_HOME`, else
`~/.hamlet`; the background server appends output to `<folder>/server.log`.
Closing the window or terminal stops no agent. `hamlet stop` stops the
server, and `hamlet status` reports it. An installed app opens
`hamlet_launch`, which does the same with no terminal: on Linux and
macOS the window then takes its process, and a failure to start is shown
in a dialog as well (see [releases](releases.md#the-launcher)).
`hamlet_launch --update <file>` stops the home's server and puts that
update in place before it opens Hamlet; the window runs it to update
([releases](releases.md#updates)). Running `hamlet_app` directly
starts only the window.

| Command | Does |
|---|---|
| `hamlet [--insecure]` | Serves the realm in the background or reuses its server, and opens a window |
| `hamlet stop` | Stops the realm's server and waits for it to close in order |
| `hamlet status` | Reports whether the realm's server runs, with its address, process ID, start time and log path |
| `hamlet server [--port <n>] [--faux <script.json>] [--insecure]` | Starts a server on a realm; a faux script answers offline for tests and demos |
| `hamlet chat [--to <hand\|hire#<id>\|mayor@<hamlet>>] [--attach <path>]...` | Chats with the Hand or the named agent and answers what waits, in the terminal; each `--attach` (an image, a file or a folder) goes with the first line |
| `hamlet dev [--port <n>] [--faux <script.json>] [--home <dir>] [--insecure] [--hot]` | Serves a scratch realm on a free port unless given one, and opens its window; `--hot` builds a window that reloads saved shaders and tuning files |
| `hamlet window` | Opens a window connected to a running server |
| `hamlet edit <charter\|limits\|settings> [--hamlet <id>]` | Opens the chief's document in `$EDITOR` and saves through the server; limits and settings are JSON, and `--hamlet` selects a hamlet's limits in place of the realm's |
| `hamlet edit character <scholar#id>` | Opens a scholar's character; a hire's address works too, for its scholar's |
| `hamlet edit memory <scholar#id\|hire#<id>> [path]` | Opens a memory file of a scholar's `main` or a hire's lodge, `notes.md` under `memory/` unless a path is given |
| `hamlet login <provider> [--key]` | Gives a key or follows an offered sign-in; `hamlet login claude` prints the link of Claude's own sign-in, to open in a browser on the server's machine |
| `hamlet models [try [model]\|refresh]` | Lists models with whether each takes images, tries a one-word hello, or refreshes the list |
| `hamlet models images <model> [off\|[--width <px>] [--height <px>] [--mb <n>] [--per-line <n>]]` | Says a model takes images, as one of a custom endpoint needs, within the limits given and the defaults for the rest; `off` clears it back to what the endpoint lists |

`hamlet server` listens on `127.0.0.1` at port 7313 unless given another
port. Bare `hamlet` starts a server on a free port. Local clients find its
URL through `server.json` and its token through `credentials.json`;
elsewhere they use `HAMLET_SERVER` and `HAMLET_TOKEN`, through an SSH
tunnel. The server runs for one chief, named for the account that runs it.
Closing a client stops no work; end of input waits for that client's
submissions and triggers no reflection.

Windows has no sandbox, so a Windows server starts only in insecure
mode, which the chief asks for with `--insecure` on `hamlet`,
`hamlet server` or `hamlet dev`. Agents' commands then reach whatever the
account running the server can (C18). Linux and macOS refuse the flag
before they start anything. `server.json` names the running server's
confinement, and bare `hamlet`, which reuses whatever server runs, prints
"Attached to a server in insecure mode." when it reuses one in that mode.
On Windows, `hamlet stop` ends the server's process, since Windows has no
signal another console can send a background process. The job the server
runs in then ends pi and every command with it.

## Typed commands

The server advertises its supported deeds in the state it sends, and
`commands.rs` carries them out: hamlets and hires, the tree and its roads,
versions of a scholar, the library, the record, the budget and what waits.
The server parses commands; the terminal handles only `/hand`, `/hamlet` and
`/agent` as address changes, and `/attach`. `/hand` talks to the Hand,
`/hamlet <hamlet>` to that hamlet's mayor, and `/agent` finds an agent
anywhere in the realm by its address (`hand`, `hire#<id>`, `mayor@<hamlet>`,
`worker#<id>`). The CLI replaces its realm view on whole states as well as
applying changes. See [`society.md`](society.md) for the society and realm's
surface, and the command table in [`spec/hamlet.md`](spec/hamlet.md#chiefs).

`/attach <path>` uploads an image, a file or a folder over a connection of its
own and attaches it to the next line (C33). A folder goes with everything under
it, links left out. What is attached and over 32 MiB or 1000 files, images
aside, asks `y/N` before it is sent; the end of input answers no. Attachments
still held when input ends go in a line of their own. In a line of words, in
the terminal or piped in, `@path` or `@"path with spaces"` at the start of the
line or after whitespace attaches what the path names where the token stood,
and the token leaves the words, with the space after it when it stood alone. A
path that does not exist leaves the token in the words, so `@someone` and
`a@b.com` stay as they are; a token that ends a word with `.`, `,`, `;`, `:`,
`!`, `?` or `)` names the path before the mark. Paths are read from the folder
`hamlet chat` runs in. A path that cannot be attached stops the line from
being said. A line that begins with `/` is no line of words, and its tokens
stay. `--attach` and `/attach` attach at the end.

## The window

`hamlet dev` chooses its realm folder from `--home`, then `HAMLET_HOME`, then a
per-user temporary folder kept between runs. It refuses `~/.hamlet` and
sets `HAMLET_VIEW=<folder>/window`. Closing its window keeps the server
running; Ctrl-C stops both. `dev` builds the checkout's server and window
and uses Cargo's reported executable paths, unless `HAMLET_SERVER_BIN`
or `HAMLET_APP` selects the corresponding existing binary.
`hamlet window` finds the app by `HAMLET_APP`, then beside the CLI,
then PATH. It passes the server address and token through the environment,
never argv. Players still open an app called Hamlet; only its executable is
named `hamlet_app`. Releases contain the four binaries, Node and pi.

The window has no realm claim and runs no agent. It finds a local server
through the realm's `server.json` and connection token, or uses
`HAMLET_SERVER` and `HAMLET_TOKEN`. Its own size, layout, appearance, chief's look
and places live on its machine under `HAMLET_VIEW`, else the OS's config folder
for Hamlet. It reads them once at launch: a missing file is the default,
and one that does not parse stops it with an error naming the file, which
it leaves as it is. Local links and pictures resolve there too: existing absolute
paths or paths under the received work folder, never the server's realm folder.
Charter and character edits use local temporary files, with each save sent
through `edit`. Closing polls the editor once more and waits for save
receipts. Unconfirmed saves keep the window open with recovery paths and
the choice to close with copies; they are not silently repeated.
