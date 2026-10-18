# The `hamlet` command

The Rust crate `hamlet_cli` supplies `hamlet`. It manages the server and
the window as separate processes and uses [protocol v0](protocol.md) for
everything it asks the server. With the Node version in `server/.nvmrc`,
run `npm ci --prefix server`. From the repository root:

```sh
HAMLET_HOME=/tmp/hamlet-try cargo run -p hamlet_cli -- dev
```

This builds the window and starts the server as a separate Node process.
`cargo install --path crates/hamlet_cli` puts the command on PATH.
`HAMLET_SERVER_DIR` selects a server package outside the source checkout;
`HAMLET_NODE` selects Node. Neither Node nor the server is bundled in the CLI.

Bare `hamlet` starts the home's server in the background, or reuses its
running server, and opens a window. The home is `HAMLET_HOME`, else
`~/.hamlet`; the background server appends output to `<home>/server.log`.
Closing the window or terminal stops no agent. `hamlet stop` stops the
server, and `hamlet status` reports it. Opening the app from Finder starts
no server.

| Command | Does |
|---|---|
| `hamlet` | Serves the home in the background or reuses its server, and opens a window |
| `hamlet stop` | Stops the home's server and waits for it to close in order |
| `hamlet status` | Reports whether the home's server runs, with its address, process ID, start time and log path |
| `hamlet server [--port <n>] [--faux <script.json>]` | Starts a server on a home; a faux script answers offline for tests and demos |
| `hamlet chat [--to <agent\|name@hamlet\|mayor@hamlet>]` | Chats with the mayor or the named agent and answers what waits, in the terminal |
| `hamlet dev [--port <n>] [--faux <script.json>] [--home <dir>]` | Serves a scratch home on a free port unless given one, and opens its window |
| `hamlet window` | Opens a window connected to a running server |
| `hamlet edit <charter\|character\|notes\|limits\|settings>` | Opens the chief's document in `$EDITOR` and saves through the server; limits and settings are JSON |
| `hamlet login <provider> [--key]` | Gives a key or follows an offered sign-in |
| `hamlet models [try [model]\|refresh]` | Lists models, tries a one-word hello, or refreshes the list |

`hamlet server` listens on `127.0.0.1` at port 7313 unless given another
port. Bare `hamlet` starts a server on a free port. Local clients find its
URL through `server.json` and its token through `credentials.json`;
elsewhere they use `HAMLET_SERVER` and `HAMLET_TOKEN`, through an SSH
tunnel. The server runs for one chief, named by `HAMLET_USER`, else
`USER`, else `user`. Closing a client stops no work; end of input waits for
that client's submissions and triggers no reflection.

## Typed commands

The command table in `server/src/commands.ts` lists every supported deed,
including roads, removal, merging, restoration, votes, founding and closing
hamlets, the record and the library. `/undo` reverses the latest reversible
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
running; Ctrl-C stops both. `dev` builds the checkout's window and uses
Cargo's reported executable path, unless `HAMLET_APP` selects an existing
window. `hamlet window` finds the app by `HAMLET_APP`, then beside the CLI,
then PATH. It passes the server address and token through the environment,
never argv. Players still open an app called Hamlet; only its executable is
named `hamlet-app`. Release archives contain the window, not the server.

The window has no home claim and runs no agent. It finds a local server
through the home's `server.json` and connection token, or uses
`HAMLET_SERVER` and `HAMLET_TOKEN`. Its own size, layout, chief's look and
places live on its machine under `HAMLET_VIEW`, else the OS's config folder
for Hamlet. Local links and pictures resolve there too: existing absolute
paths or paths under the received work folder, never the server's houses.
Charter and character edits use local temporary files, with each save sent
through `edit`. Closing polls the editor once more and waits for save
receipts. Unconfirmed saves keep the window open with recovery paths and
the choice to close with copies; they are not silently repeated.
