# MCP servers

A chief gives agents the tools of [Model Context Protocol](https://modelcontextprotocol.io)
servers by listing them in `mcp/servers.json` in the realm (C36). The server
is the MCP client, through the `rmcp` crate; pi sees each tool as one more
tool of the catalogue and calls it through the same `operation` message as
`read`, so the runtime protocol does not change
([ADR 0017](adr/0017-mcp-servers-run-on-the-server-under-the-sandbox-and-never-ask-it-for-anything.md)).

```text
pi --operation(mcp__github__create_issue)--> hamlet_server --MCP over stdio--> MCP server
                                                  |                            (bubblewrap or Seatbelt,
                                       gate, marker, scrub                      the reach its entry declares)
```

The code is `tools/mcp.rs` in `hamlet_server`: `config` reads the list,
`session` keeps each server running, and `result` makes what the model is
given.

## The list

```json
{
  "servers": {
    "github": {
      "command": ["/usr/local/bin/github-mcp-server", "stdio"],
      "env": { "GITHUB_PERSONAL_ACCESS_TOKEN": "..." },
      "sandbox": { "net": true },
      "timeout_secs": 120
    }
  }
}
```

| Key | Meaning |
|---|---|
| `command` | The program and its arguments. |
| `url` | The address of a server reached over streamable HTTP instead of a program: https, or http on this machine. |
| `headers` | For `url`: the headers each request carries, such as `Authorization`; secrets as `env`'s values are. |
| `env` | Its environment, on top of the sandbox's base one. |
| `inherit_env` | Names copied from the environment of the server, so a token need not be written in the file. |
| `sandbox.fs_ro`, `fs_rw` | Paths it may read and write, relative to its own folder. |
| `sandbox.net` | Whether it reaches the network. |
| `sandbox.limits` | `max_procs`. A server runs for as long as it is listed, so it has no CPU limit of its own, only the machine's. |
| `timeout_secs` | How long a call may take; 120 when left out. |
| `allow` | Tools, by the server's names, whose calls run without asking, though the server does not say they only read; `*` for every tool. |
| `ask` | Tools whose calls always wait for a chief, though the server says they only read; `*` for every tool. |

An entry has a `command` or a `url`. A server reached by a url runs
nowhere here, so it takes no `env`, `inherit_env` or `sandbox`. Its url
holds no name or password, which a client would be shown; a header or a
sign-in carries a credential. A request follows a redirect only within the
url's origin, so no header reaches another, and a change of the url's
origin drops the headers whose values it does not give again.

A server's name is 1 to 24 lowercase letters, digits and `-`. A key the
entry does not know, or an entry that does not fit, is named in the prompt,
and the other entries load. The file is read at server start and again when
it changes: a server whose entry changed restarts, its tools still offered
meanwhile, unless only `timeout_secs`, `allow` or `ask` changed, and one no
longer listed stops. A file that is not JSON, such as one saved halfway
through an edit, or that holds anything but `servers`, is named in the
prompt and changes no server.

The `mcp` folder is in the realm, out of every agent's reach, so the tokens
in the list are too. Each server has its own folder, `mcp/<name>/`, made
when it first starts: its working folder and its `HOME`, and the one place
it writes besides `fs_rw`. It never writes the list, and no `fs_ro` or
`fs_rw` may hold the realm's `mcp` folder or reach into it past its own
folder, where the others' tokens are. A program installed
outside the system's folders, such as one under a version manager's folder
in the chief's home, needs that folder in `fs_ro`.

## Changing it

The list is the chief's file, and also changes through the server, which
writes one entry at a time and leaves the rest of the file as written:

- Settings, MCP Servers, shows each server, how it is and its tools, and
  adds, changes, restarts and removes one; its network is a switch.
- `hamlet mcp` does the same from a terminal: `list`, `tools <name>`,
  `add <name> [flags] -- <command>...`, `edit <name>` in `$EDITOR`,
  `remove <name>`, `restart <name>`.
- `hamlet mcp import`, or Find in Other Programs in Settings, lists the
  servers Claude Code, Claude Desktop, Cursor and Codex list on the
  server's machine, and adds those chosen, or every one with `--all`. Each
  is given the network, as it had there; a program named by its name alone
  is found on `PATH`, and one installed in the home folder gets its
  install folder in `fs_ro`. A `${NAME}` value, and a name in Codex's
  `env_vars`, becomes `inherit_env`. A server reached by a url is added
  with its headers, and Codex's `bearer_token_env_var` as an
  `Authorization` header. One on the older SSE transport, or with a `cwd`
  of its own, is listed and not added, and so is one this list would not
  load, with why. Two that differ under one name are named apart with
  `-2`, `-3`.

No client is sent a value of a server's `env`, only its names; a change
that leaves a value out keeps the one the entry has. A restart keeps the
server's tools offered while it starts again.

A server reached by a url that signs in with OAuth is signed in to with
Sign In in Settings, or `hamlet mcp sign-in <name>`. The server registers
itself as a client of the server's authorization server, and the chief
signs in on the page it gives; the code comes back to a loopback on the
server's machine, or, when the browser is on another machine and its last
page does not load, in that page's address pasted back
(`hamlet mcp sign-in <name> <address>`). The tokens are kept in
`mcp/.signed-in/<name>/`, a file for each url, so a server whose url
changes signs in again. They are readable by the server's account alone,
out of every program server's reach, refreshed there, taken out of the
results of the server's calls, and removed with the server. A server that
wants a sign-in stops with that reason.

An agent can ask for a server with `request_mcp_server`, naming the
variables it needs, never their values. The chief sees its command and
reach, and allows or denies; allowed, the entry is added with each value
empty for the chief to give, and the agent is told in its chat either
way.

## What runs

`session` starts the program under its policy, connects the client to its
standard input and output, and lists its tools through every page. A
server whose initialization or listing does not answer within 30 seconds
is stopped. When the process ends it starts again after 2 seconds,
doubling to 5 minutes, and back to 2 once a session lasted a minute. Its
error output goes to the server's log, without its secrets, and the last
lines of it are logged when it stops.

While a server starts again, the tools it last listed stay offered, so a
restart changes no agent's tools and no prompt; a call meanwhile fails at
once with why the server stopped. A server that has never listed its
tools is named in the prompt of every agent instead, with why it did not
start. `notifications/tools/list_changed` lists the tools again.

A tool of server `github` called `create_issue` is `mcp__github__create_issue`.
A name with other characters, or longer than 64, is changed or shortened
with a hash of the original, so two tools never share a name. No other
tool's name begins with `mcp__`: a tool added by hand cannot take one. A
server offers at most 128 tools. Every agent of a hamlet is offered every
tool, and a chief or its parent turns tools off for it as any other tool,
with `/tools <agent> -mcp__github` or `set_tools`: `mcp__<server>` names
every tool of that server. A tool whose arguments' schema does not
compile or is longer than 16 KiB is left out, and named in the prompt by
the name a model would call it by. A description is cut at 4096
characters.

The client tells a server it offers nothing: sampling, roots and
elicitation are refused, and a call that asks for input fails.

## A call

1. The gate classifies it as an added tool. A server's network is the
   chief's, given when it was listed, so no call to it holds for the
   network, and none is refused for it. A tool that may change something
   outside the realm holds in every mode: one whose server does not mark
   it `readOnlyHint`, unless `allow` names it, or one `ask` names. A name
   in `ask` or `allow` decides before `*`. The chief may allow the tool
   for the rest of the conversation. Any other call holds when the chat
   holds commands.
2. The call is marked begun before it runs, so a restart answers that it
   was interrupted instead of running it twice, even for a tool that says
   it only reads.
3. Arguments that do not fit the tool's schema fail without being sent.
4. It is sent as `tools/call`. A call that outlasts `timeout_secs`, or whose
   turn is stopped, is cancelled with `notifications/cancelled`.
5. The result's text is joined, the credentials the realm holds and the
   server's own secrets are taken out, and it is cut as pi cuts a read
   (2000 lines or 50 KB). An image is given to a model that takes images,
   fitted to its limits; audio and binary resources are named and left out.
   `isError` makes the result an error.

The server's secrets are the values of its `env` and `inherit_env` of 8
characters or more; a shorter value is no credential, and taking it out
would take out common words.

A tool's result is text from a program the chief chose and can carry
instructions meant for the model. It is data like a file's content; the
gate and each agent's tool list are what limit what follows.
