---
status: accepted
---

# Agents push through a git proxy in the server

A hamlet's clone points its `origin` at a proxy in the server, and agents run
ordinary `git` against it. The sandbox holds only a random credential for
that hamlet, which the proxy accepts and nothing else does. The proxy serves
only git's own endpoints for the hamlet's one repository, refuses deleting
branches and pushing tags, and adds the real token before passing the
request on. Agents may push any other branch; protecting `main` is left to
rules the chief sets on the git host. The chief's own Push goes through the
same proxy.

The real token is one the chief pastes in Settings, ideally a fine-grained
token limited to the repository. The server keeps it in the home's
credential file, and it never enters a sandbox.

See `docs/proposals/research/git-credentials-in-agent-sandboxes.md` for how
other products do this. Claude Code on the web uses the same pattern.

## Considered options

- A `push` tool that the server runs outside the sandbox, as the GitHub
  Copilot cloud agent does. Rejected: every git action agents need would
  become a tool of ours.
- An HTTPS proxy that adds the token to any request, with a certificate
  authority installed in the sandbox. Rejected: it decrypts all traffic and
  cannot see which refs a push updates.
- The token inside the sandbox. Rejected: the agent could read it and send
  it anywhere.
- A GitHub App minting a short-lived token per push. Left for deployment:
  an open-source app cannot ship the App's private key.

## Consequences

- With the network off, the sandbox still needs a way to reach the proxy:
  a bridge under bubblewrap, one allowed local port under Seatbelt.
- Force pushes are not told apart by the proxy; a ruleset on the git host
  covers them.
