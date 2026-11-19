# hamlet_workspace

One binary, `hamlet_workspace`. `worker CONFIG` serves a remote
workspace on a Linux machine: it authenticates each Hamlet realm by its
bearer token and runs each file or process operation confined by
bubblewrap, with only the calling agent's worktree and, when granted, the
shared project writable. Hamlet's server talks to it directly.

The binary is Linux-only; elsewhere it only reports that it is
unsupported, so the Cargo workspace still builds.

What agents and the chief see is in
[`docs/sandbox.md`](../../docs/sandbox.md). The protocol is the
`hamlet_workspace_wire` crate, with examples in its
[`fixtures/protocol.json`](../hamlet_workspace_wire/fixtures/protocol.json).

## Build the image

```sh
cargo xtask workspace-image
```

This tags the image `hamlet_workspace:<short commit>`. Arguments after
the task go to `docker build`, such as `--platform linux/amd64`. The
build fails if `Cargo.lock` would change.

The runtime image is Debian 12 slim, with `bubblewrap` at `/usr/bin/bwrap`
and `socat` at `/usr/bin/socat`. Commands see only `/usr`, `/bin`, `/lib`
and `/lib64` read-only, so a tool is available to agents only if it is
installed under those paths. The image holds no configuration and no
tokens.

## Configuration

The worker reads one JSON file named by its second argument. It rejects
unknown fields, so it cannot hold comments. See
[`examples/worker.example.json`](examples/worker.example.json).

| Field | Meaning |
|---|---|
| `listen` | Plain HTTP address for `POST /v1`. |
| `root` | Durable data root, which must already exist; it is set to mode 0700. |
| `realms` | Each realm's stable name (letters, digits, `-`, `_`, at most 128) and its bearer token: at least 16 characters, different for every realm. |

Realms. One worker serves any number of realms; the token alone tells it
which is calling, and two realms never share a file. To add a realm, add a
name and a new random token to `realms`, restart the worker, and give the
chief the address and token under Workspace in Hamlet's Settings. A
realm's name must never change, since its files are found by it. A realm
removed from `realms` is refused from the next start; its files stay.

Secrets. The tokens sit in the configuration file, so mount it read-only
from a secret store.

Files. A workspace is a hamlet's, named by its id, or `realm` for agents
of no hamlet. Worktrees are under
`{root}/{realm}/{workspace}/worktrees/{agent}`, bound at `/worktree`; the
shared project is `{root}/{realm}/{workspace}/project`, bound at
`/project`. Put `root` on persistent storage: the project lives only
there.

A remote worktree is a copy. The server stays its source: it pushes the
worktree when its own copy changes and pulls it back after each tool call
that may have written it. A second worker on the same root exits, and a
root emptied by accident cannot be told from a new one.

Journal. Each operation id is admitted once. A repeated id is never run
again and answers `uncertain`, as does one a restart cut short. A
`cancel` received before its `execute` means the `execute` is never
admitted, even after a restart, and answers `aborted`. A `cancel` of a
running operation stops its whole process tree before the reply is
written.

TLS. The worker serves plain HTTP; Hamlet accepts only an `https://`
URL or a loopback `http://` one. Keep `listen` on loopback and tunnel:

```sh
ssh -N -L 7420:127.0.0.1:7420 USER@WORKSPACE-HOST
```

or put a TLS proxy in front. Never expose the plain port on a network.

## Running in Docker

```sh
docker run -d --name hamlet_workspace \
  --security-opt seccomp=unconfined \
  --security-opt systempaths=unconfined \
  -p 127.0.0.1:7420:7420 \
  -v "$PWD/worker.json:/etc/hamlet_workspace/worker.json:ro" \
  -v hamlet_workspace:/workspace \
  hamlet_workspace:TAG worker /etc/hamlet_workspace/worker.json
```

Inside the container, `listen` must be `0.0.0.0:7420` and `root`
`/workspace`, the volume.

Docker's default seccomp profile refuses the user and network
namespaces each operation runs in, and its masked `/proc` paths stop
bubblewrap mounting a new `/proc`, hence the two options; neither is
enough alone. Do not drop `SETFCAP`. Without these every operation fails
with `confinement`: nothing runs unconfined.

Hosts with AppArmor may need `--security-opt apparmor=unconfined`; not
checked. Outside Docker the worker needs bubblewrap and socat at the
paths above.

## Deleting a workspace

Hamlet never deletes a workspace. The operator does, with the worker
stopped, by removing `{root}/{realm}/{workspace}`.

## Shared skills

Hamlet names a snapshot of its library on each execute request by
`library.sha256`; the worker binds it read-only at `/library`. If it lacks
the snapshot it answers `library_missing` and Hamlet resends it with
`library.files`. Links, special files, more than 1024 entries and more
than 4 MiB are refused. An unused snapshot is swept after 15 minutes.

## Network grants

A denied operation has no route out. A granted one reaches the worker's
HTTP proxy through a Unix socket and a loopback relay on port 3128
(`HTTP_PROXY`, `HTTPS_PROXY`). The proxy allows public destinations on
ports 80 and 443 only, rejecting private, loopback, link-local
(metadata), shared and transition addresses. The check is by address
alone: on a network that routes public-looking addresses privately, block
those ranges outside the worker before using network grants.

## Tests

`cargo test -p hamlet_workspace` on Linux. The server's tests use a
loopback double of the worker, in
`crates/hamlet_server/tests/crashes/remote.rs`; no automated test runs the
server against the real worker.
