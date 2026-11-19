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
| `command` | The program and its arguments. Required. |
| `env` | Its environment, on top of the sandbox's base one. |
| `inherit_env` | Names copied from the environment of the server, so a token need not be written in the file. |
| `sandbox.fs_ro`, `fs_rw` | Paths it may read and write, relative to its own folder. |
| `sandbox.net` | Whether it reaches the network. |
| `sandbox.limits` | `cpu_secs` and `max_procs`; a server runs for as long as it is listed, so its CPU limit is thirty days unless set. |
| `timeout_secs` | How long a call may take; 120 when left out. |

A server's name is 1 to 24 lowercase letters, digits and `-`. A key the
entry does not know, or an entry that does not fit, is named in the prompt,
and the other entries load. The file is read at server start and again when
it changes: a server whose entry changed restarts, unless only
`timeout_secs` changed, and one no longer listed stops. A file that is not
JSON, such as one saved halfway through an edit, is named in the prompt and
changes no server.

The `mcp` folder is in the realm, out of every agent's reach, so the tokens
in the list are too. Each server has its own folder, `mcp/<name>/`, made
when it first starts: its working folder and its `HOME`, and the one place
it writes besides `fs_rw`. It never writes the list. A program installed
outside the system's folders, such as one under a version manager's folder
in the chief's home, needs that folder in `fs_ro`.

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
server offers at most 128 tools. A tool whose arguments' schema does not
compile or is longer than 16 KiB is left out, and named in the prompt by
the name a model would call it by. A description is cut at 4096
characters.

The client tells a server it offers nothing: sampling, roots and
elicitation are refused, and a call that asks for input fails.

## A call

1. The gate classifies it as an added tool, as it does a tool added by
   hand: it holds for the chief when the server declares the network and
   the agent has no network standing, and when the chat holds commands.
2. The call is marked begun before it runs, so a restart answers that it
   was interrupted instead of running it twice; a server's `readOnlyHint`
   is its own word and decides nothing.
3. A server runs with the reach its entry declares, so a call ruled to run
   without the network, to a server that has it, fails without being sent.
   Arguments that do not fit the tool's schema fail too.
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
