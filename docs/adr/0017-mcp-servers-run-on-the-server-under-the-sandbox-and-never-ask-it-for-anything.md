---
status: accepted
---

# MCP servers run on the server under the sandbox and never ask it for anything

The server is the MCP client. Each server the chief lists runs as a
process of the server's machine under the policy its entry declares, and
the tools it lists are offered to pi as ordinary tools of the catalogue.
Pi speaks no MCP, and the runtime protocol does not change. This keeps
ADR 0003: a harness holds no host tools, and every call goes through the
gate, the marker that runs it once, and the sandbox.

A server answers calls; Hamlet answers nothing it asks. Sampling, roots and
elicitation are refused, so a server cannot spend the chief's models, read
folders no policy gave it, or put questions to a chief outside the gate.

The client is the `rmcp` crate, the protocol's own Rust SDK, with only its
client and its transport over a reader and a writer. It negotiates the
protocol's versions, pages, cancels and bounds what a server sends, so
Hamlet keeps only what is its own: the sandbox, the gate, the restarts and
the result a model is given.

## Considered options

- An MCP client in pi. Rejected: the server's processes and the chief's
  credentials would be pi's, outside the sandbox and the gate.
- Starting a server for each call, as a tool added by hand runs. Rejected:
  a server keeps state and takes seconds to start.
- A client written by hand over JSON-RPC. Rejected: it would follow every
  revision of the protocol by hand, and the SDK's client needs few crates
  beyond those the server has.
- The list in the `tools` folder beside the tools added by hand. Rejected:
  agents read that folder, and the list holds tokens.

## Consequences

- An MCP server runs on the machine of the server, not in a remote
  workspace, so it reaches the files of that machine's policy and never the
  worktree's remote copy.
- A tool never runs twice: every MCP call is marked begun, since a server
  may write and its `readOnlyHint` cannot be trusted to make a retry safe.
- A server's `readOnlyHint` only decides whether a call waits for a chief,
  and the chief's entry overrides it both ways: the chief chose the server,
  and a tool that does not say it only reads always waits.
- A server's reach is fixed while it runs, so a call the gate rules to run
  without the network does not run on a server that has it.
- The streamable HTTP transport is a feature of the same crate: an entry
  that says a url instead of a command, its headers and OAuth tokens kept
  by the server (ADR 0005). It needs the workspace's `reqwest` at the
  crate's version, 0.13.
