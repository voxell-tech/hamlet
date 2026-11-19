# The `hamlet` command

The Rust crate `hamlet_cli` supplies `hamlet`. It manages the server and
the window as separate processes and uses [the client protocol](protocol.md)
for everything it asks the server. For local work, from the repository
root (see AGENTS.md for the Node setup):

```sh
HAMLET_HOME=/tmp/hamlet-try cargo run -p hamlet_cli -- dev
```

`dev` builds the server and window; the server starts its Node pi
harness as a child process. `cargo xtask install` installs all three
binaries; `hamlet_cli` alone has no server or window.

The CLI selects the server by `HAMLET_SERVER_BIN`, then a sibling
`hamlet_server`, then PATH. The server selects pi's `src/main.ts` by
`HAMLET_PI_HARNESS`, then the one shipped beside it, then its build
checkout's, and refuses to start when that entry does not exist.
`HAMLET_NODE` selects Node. A release leaves out the optional packages
that carry Claude's program, and pi downloads them into the realm when
Claude is first signed in to or called; `HAMLET_CLAUDE_EXECUTABLE` selects another
program.

Bare `hamlet` starts the realm's server in the background, or reuses its
running server, and opens a window. The realm's folder is `HAMLET_HOME`,
else `~/.hamlet`; the server appends output to `<folder>/server.log`.
Closing the window or terminal stops no agent; `hamlet stop` stops the
server. An installed app opens `hamlet_launch`, which does the same
(see [releases](releases.md#the-launcher) and
[updates](releases.md#updates)).

| Command | Does |
|---|---|
| `hamlet [--insecure]` | Serves the realm in the background or reuses its server, and opens a window |
| `hamlet stop` | Stops the realm's server in order |
| `hamlet status` | Reports whether it runs |
| `hamlet server [--port <n>] [--faux <script.json>] [--insecure]` | Starts a server on a realm; a faux script answers offline for tests and demos |
| `hamlet chat [--to <hand\|agent#<id>\|lead@<hamlet>>] [--attach <path>]...` | Chats in the terminal; each `--attach` goes with the first line |
| `hamlet dev [--port <n>] [--faux <script.json>] [--home <dir>] [--insecure] [--hot]` | Serves a scratch realm and opens its window; `--hot` reloads saved shaders and tuning files |
| `hamlet window` | Opens a window connected to a running server |
| `hamlet import [--file <export>] [--all] [--sessions]` | Lists projects other programs worked on; `--file` reads a ChatGPT or Claude export |
| `hamlet import [--file <export>] [--all] --suggested \| <project or conversation id>...` | Brings in projects, each as a hamlet with an agent per conversation, or one conversation by id (C35) |
| `hamlet edit <charter\|limits\|settings> [--hamlet <id>]` | Opens the chief's document in `$EDITOR`; also `edit character <profile#id>` |
| `hamlet edit memory <profile#id\|agent#<id>> [path]` | Opens a memory file, `notes.md` by default |
| `hamlet login <provider> [--key]` | Gives a key or follows a sign-in; `hamlet login claude` prints a link to open on the server's machine |
| `hamlet models [try [model]\|refresh]` | Lists models, tries one, or refreshes the list |
| `hamlet models images <model> [off\|[--width <px>] [--height <px>] [--mb <n>] [--per-line <n>]]` | Says a custom endpoint's model takes images |

Import finds Claude Code's conversations under `CLAUDE_CONFIG_DIR` or
`~/.claude`, Codex's under `CODEX_HOME` or `~/.codex`, and pi's under
`PI_CODING_AGENT_SESSION_DIR`, else `PI_CODING_AGENT_DIR`, else
`~/.pi/agent`.

`hamlet server` listens on `127.0.0.1` at port 7313 unless given another
port; bare `hamlet` uses a free port. Remote clients use `HAMLET_SERVER`
and `HAMLET_TOKEN` through an SSH tunnel.

Windows has no sandbox, so a Windows server starts only with
`--insecure`, and agents' commands then reach whatever the account can
(C18). Linux and macOS refuse the flag. On Windows, `hamlet stop` ends
the server's process, and the job it runs in ends pi and every command.

## Typed commands

The server advertises its actions in the state it sends and
`crates/hamlet_server/src/clients/commands.rs` carries them out; see
[`society.md`](society.md) and [`spec/hamlet.md`](spec/hamlet.md#chiefs).
The terminal handles only `/hand`, `/hamlet <hamlet>`, `/agent
<address>`, `/attach` and `/unsend`.

`/attach <path>` attaches an image, a file or a folder to the next line
(C33). More than 32 MiB or 1000 files, images aside, asks `y/N` first.
In a line of words, `#path` or `#"path with spaces"` attaches the same
way; a path that does not exist stays in the words, and `#3`, a wait, is
never a path. A line that begins with `/` has no such tokens.

## The window

`hamlet dev` chooses its realm folder from `--home`, then `HAMLET_HOME`,
then a per-user temporary folder. It refuses `~/.hamlet` and sets
`HAMLET_VIEW=<folder>/window`. Closing its window keeps the server
running; Ctrl-C stops both. `HAMLET_SERVER_BIN` or `HAMLET_APP` skips a build.
`hamlet window` finds the app by `HAMLET_APP`, then beside the CLI, then
PATH, and passes the server address and token through the environment,
never argv.

The window's layout and appearance live on its own machine
under `HAMLET_VIEW`, else the OS's config folder, read once at launch; a
file that does not parse stops it with an error naming the file. Local
links and pictures resolve there, never into the server's realm folder.
