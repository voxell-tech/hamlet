---
status: accepted
---

# Agents use the chief's own git

On the chief's machine, agents run the chief's own git in the chief's own
folder, with whatever credentials the machine has. Their commands can read
those credentials, including `~/.ssh` and `~/.gitconfig`. Git reaches a host
only with the network, like any other command (C19, H11). Agents may write
hooks and config, including in nested repositories; the chief accepts that
these may run under the chief's own git later.

## Considered options

- A clone Hamlet keeps, whose `origin` is a git proxy in the server that
  holds the real token and adds it to each push. Rejected on the chief's
  machine: more machinery than one chief's local tool needs, and the
  chief's git already has the credentials.
- Pass the SSH agent into the sandbox and hide `~/.ssh`, so commands can use
  the key but not copy it. Not taken: the key stays readable.
- A lock on `.git/hooks` and `.git/config`. Not taken: the lock prevents
  `git init`, cloning into the folder, `git config` and `git remote add`.
  Under bubblewrap it needs a scan before every command and still misses a
  repository made during one.
- Git on a remote machine. Left for deployment: either a proxy or a
  short-lived GitHub App token inside the remote machine.
  [`docs/proposals/research/git-credentials-in-agent-sandboxes.md`](../proposals/research/git-credentials-in-agent-sandboxes.md)
  is how other products do this.

## Consequences

- A command the chief lets reach the network can read the chief's keys and
  tokens and send them anywhere. A key without a passphrase then works for
  whoever has it, on every repository the chief can reach.
- Agents' git can do anything the chief's credentials allow: any repository,
  force pushes, deleting branches and tags.
- Hooks and config written by agents may run under the chief's git.
- Nothing in Hamlet holds a git token.
