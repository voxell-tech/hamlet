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
`HAMLET_NODE` selects Node. Node is installed separately; the release
archive includes the pi package and its production dependencies.

Bare `hamlet` starts the home's server in the background, or reuses its
running server, and opens a window. The home is `HAMLET_HOME`, else
`~/.hamlet`; the background server appends output to `<home>/server.log`.
Closing the window or terminal stops no agent. `hamlet stop` stops the
server, and `hamlet status` reports it. The packaged macOS app runs this
CLI entry, so opening it also starts or reuses the server. Running
`hamlet_app` directly starts only the window.

| Command | Does |
|---|---|
| `hamlet [--insecure]` | Serves the home in the background or reuses its server, and opens a window |
| `hamlet stop` | Stops the home's server and waits for it to close in order |
| `hamlet status` | Reports whether the home's server runs, with its address, process ID, start time and log path |
| `hamlet server [--port <n>] [--faux <script.json>] [--insecure]` | Starts a server on a home; a faux script answers offline for tests and demos |
| `hamlet chat [--to <agent\|name@hamlet\|mayor@hamlet>]` | Chats with the mayor or the named agent and answers what waits, in the terminal |
| `hamlet dev [--port <n>] [--faux <script.json>] [--home <dir>] [--insecure]` | Serves a scratch home on a free port unless given one, and opens its window |
| `hamlet window` | Opens a window connected to a running server |
| `hamlet edit <charter\|character\|notes\|limits\|settings>` | Opens the chief's document in `$EDITOR` and saves through the server; limits and settings are JSON |
| `hamlet login <provider> [--key]` | Gives a key or follows an offered sign-in |
| `hamlet models [try [model]\|refresh]` | Lists models, tries a one-word hello, or refreshes the list |

`hamlet server` listens on `127.0.0.1` at port 7313 unless given another
port. Bare `hamlet` starts a server on a free port. Local clients find its
URL through `server.json` and its token through `credentials.json`;
elsewhere they use `HAMLET_SERVER` and `HAMLET_TOKEN`, through an SSH
tunnel. The server runs for one chief, the one its home was founded for.
A new home takes its chief's name from `HAMLET_USER`, else `USER`, and is
refused when neither is set. Closing a client stops no work; end of input
waits for that client's submissions and triggers no reflection.

Windows has no sandbox yet, so a Windows server starts only in insecure
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

The server advertises supported deeds in `state.rs`; `chats/say.rs`,
`society.rs` and `realm/deeds.rs` dispatch them, including roads, removal,
merging, restoration, votes, founding and closing hamlets, the record and
library. `/undo` reverses the latest reversible
removal or merge. The server parses commands; the terminal handles only
`/agent` and `/hamlet` as address changes. `/agent` finds a living agent
anywhere in the realm, and `/hamlet` reaches that hamlet's mayor. The CLI
replaces its realm view on whole states as well as applying changes.
`hamlet edit limits --hamlet <id>` edits another hamlet's limits.
See [`society.md`](society.md) for the society and realm's surface.

## The window

`hamlet dev` chooses its home from `--home`, then `HAMLET_HOME`, then a
per-user temporary folder kept between runs. It refuses `~/.hamlet` and
sets `HAMLET_VIEW=<home>/window`. Closing its window keeps the server
running; Ctrl-C stops both. `dev` builds the checkout's server and window
and uses Cargo's reported executable paths, unless `HAMLET_SERVER_BIN`
or `HAMLET_APP` selects the corresponding existing binary.
`hamlet window` finds the app by `HAMLET_APP`, then beside the CLI,
then PATH. It passes the server address and token through the environment,
never argv. Players still open an app called Hamlet; only its executable is
named `hamlet_app`. Release archives contain all three binaries and pi.
On macOS, Finder may not inherit the shell's Node PATH: run the bundle's
`Contents/MacOS/hamlet` from a terminal with `HAMLET_NODE` set if needed.

The window has no home claim and runs no agent. It finds a local server
through the home's `server.json` and connection token, or uses
`HAMLET_SERVER` and `HAMLET_TOKEN`. Its own size, layout, chief's look and
places, and the sign-in to a Hamlet account with its vault's data key (`bazaar-token`),
live on its machine under `HAMLET_VIEW`, else the OS's config folder
for Hamlet. It reads them once at launch: a missing file is the default,
and one that does not parse stops it with an error naming the file, which
it leaves as it is. Local links and pictures resolve there too: existing absolute
paths or paths under the received work folder, never the server's houses.
Charter and character edits use local temporary files, with each save sent
through `edit`. Closing polls the editor once more and waits for save
receipts. Unconfirmed saves keep the window open with recovery paths and
the choice to close with copies; they are not silently repeated.
