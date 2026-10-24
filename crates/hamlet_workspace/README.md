# hamlet_workspace

One binary, `hamlet-workspace`. `worker CONFIG` serves a remote
workspace on a Linux machine: it authenticates each Hamlet home by its
bearer token and runs each file or process operation confined by
bubblewrap, with only the calling agent's house and, when granted, the
shared project writable. Hamlet's server talks to it directly.

The binary is Linux-only. Built for another system, it only reports
that it is unsupported, so the Cargo workspace still builds there.

This page covers packaging, configuration and operation. What agents
and the chief see is in [`docs/sandbox.md`](../../docs/sandbox.md), and
the protocol is the `hamlet_workspace_wire` crate
([`../hamlet_workspace_wire/src/lib.rs`](../hamlet_workspace_wire/src/lib.rs)),
which Hamlet's server uses too, with example messages and replies in
[`fixtures/protocol-v1.json`](../hamlet_workspace_wire/fixtures/protocol-v1.json).

## Build the image

From the repository root:

```sh
cargo xtask workspace-image
```

This tags the image `hamlet-workspace:<short commit>` and labels it
`org.opencontainers.image.revision` with the full commit, so a deployed
image tells which commit it holds. Arguments after the task go to `docker
build`, such as `--platform linux/amd64`; a `-t` there replaces the
default tag.

The build stage copies the whole Cargo workspace, because Cargo needs
every member to load it, then compiles only this crate with
`cargo build --locked --release -p hamlet_workspace`. It fails if
`Cargo.lock` would change.

The runtime image is Debian 12 slim with:

- `/usr/local/bin/hamlet-workspace` as the entrypoint, with no default
  arguments;
- `bubblewrap` pinned to `0.8.0-2+deb12u1` at `/usr/bin/bwrap`, the
  path the worker calls;
- `bash`, coreutils, findutils, grep, sed, gawk, diffutils, patch, file,
  tar, gzip, xz, procps, less, git, curl, Python 3 and ripgrep for agent commands
  and the shipped starter skills;
- `socat` for the confined operation's loopback-to-Unix proxy relay;
- `ca-certificates`, the public trust bundle network-granted commands
  see.

Both base images are pinned by index digest in the `ARG` lines. Commands
run with only `/usr`, `/bin`, `/lib` and `/lib64` mounted read-only, so
a tool is available to agents only if it is installed under those paths.
Only `/etc/ssl/certs/ca-certificates.crt`, the public CA trust bundle,
is additionally mounted read-only for network-granted commands. Other
`/etc` contents remain hidden.

The image holds no configuration and no tokens.

## Configuration

The worker reads one JSON file named by its second argument. It rejects
unknown fields, so it cannot hold comments. See
[`examples/worker.example.json`](examples/worker.example.json).

| Field | Meaning |
|---|---|
| `listen` | Plain HTTP address for `POST /v1`. |
| `root` | Durable data root, which must already exist; it is set to mode 0700. |
| `homes` | Each home's stable name (letters, digits, `-`, `_`, at most 128) and the bearer token its Hamlet sends: at least 16 characters, different for every home. |

Homes. One worker serves any number of Hamlet homes. The token alone
tells the worker which home is calling: two homes never share a file or
an operation record, even when their hamlets have the same names. To
add a home, give it a name and a new random token in `homes` and restart
the worker, then give its chief the worker's address and that token
under Workspace in Hamlet's Settings. A home's name must never change:
its files are found by it. A home taken out of `homes` is refused from
the next start; its files stay until an operator deletes them.

Secrets. The homes' tokens sit in the configuration file, so mount it
from a secret store, read-only. The worker holds no other credentials:
no model, git or cloud credentials. Commands see none of the machine's
files beyond their house, the project, the library and the system
folders above.

Files. Houses live at `{root}/{home}/{workspace}/houses/{agent}`, the
shared project at `{root}/{home}/{workspace}/project`, the operation
journal at `{root}/{home}/operations` and library snapshots at
`{root}/{home}/.library/{sha256}`. Put `root` on persistent storage:
the houses live only there, and files are as durable as that storage.
The worker holds `{root}/.control/lock` while running, so a second
worker on the same root exits. A missing `root` stops the worker rather
than starting it empty, but a root emptied by accident cannot be told
from a new one.

Journal. Each operation id is admitted once. The worker writes the
id's marker durably before running it and the reply after it. A
repeated id is never run again and answers `uncertain`, as does an id
whose operation was cut short by a worker restart. A `cancel` received
before its `execute` marks the id cancelled before acknowledging it, so
it is never admitted, even after a restart; its `execute` answers
`aborted`. A `cancel` of a running operation stops its whole process
tree before the operation's reply is written. A supervisor still
running 10 seconds after the cancel is killed instead, and its reply,
an uncertain `confinement`, does not wait for the tree to end.

TLS. The worker serves plain HTTP and does not terminate TLS. Hamlet
accepts only an `https://` workspace URL or a loopback `http://` one.
Either keep `listen` on the worker machine's loopback and reach it
through an SSH tunnel from the server's machine:

```sh
ssh -N -L 7420:127.0.0.1:7420 USER@WORKSPACE-HOST
```

then choose `http://127.0.0.1:7420` and the home's token under
Workspace in Hamlet's Settings; or put a TLS proxy in front of it. Do
not expose the plain port on a network.

## Running in Docker

```sh
docker run -d --name hamlet-workspace \
  --security-opt seccomp=unconfined \
  --security-opt systempaths=unconfined \
  -p 127.0.0.1:7420:7420 \
  -v "$PWD/worker.json:/etc/hamlet-workspace/worker.json:ro" \
  -v hamlet-workspace:/workspace \
  hamlet-workspace:TAG worker /etc/hamlet-workspace/worker.json
```

Inside the container, `listen` must be `0.0.0.0:7420` for the
published port to reach it; `-p 127.0.0.1:…` keeps that port on the
host's loopback. `root` is `/workspace`, the volume.

Every operation runs in a new user and network namespace with all
capabilities dropped. Docker's default seccomp profile refuses those
namespaces, and its masked `/proc` paths stop bubblewrap mounting a new
`/proc`, hence the two options; neither is enough alone. The worker
maps its own root into each namespace, which needs `SETFCAP`, part of
Docker's default capabilities, so do not drop it. Without any of these
every operation fails with `confinement`: nothing runs unconfined.

This setup was checked on Docker 29.4 on arm64 Linux: confined commands
saw only their own house, the shared project and the system folders, a
denied command could not resolve a name, and a granted one reached a
public HTTPS site through the proxy but not the metadata address. Hosts
with AppArmor may need `--security-opt apparmor=unconfined` as well;
that is not checked.

Outside Docker the worker needs bubblewrap at `/usr/bin/bwrap`, socat
at `/usr/bin/socat`, user namespaces it may create, and the tools
agents use under `/usr`, `/bin`, `/lib` and `/lib64`. That is not
checked either.

## Deleting a workspace

Hamlet never deletes a workspace. The operator does, with the worker
stopped, by removing `{root}/{home}/{workspace}`. Nothing remembers it:
a later request for the same workspace starts it empty.

## Shared skills

Hamlet owns the library on the server, outside the remote houses. Its
confined shelf reader takes a snapshot of the shelf for each tool call
and names it on an execute request by `library.sha256`, the digest of
its files. The worker keeps each snapshot it was given under
`{root}/{home}/.library/{sha256}` and binds it read-only at `/library`.
When it does not hold the named snapshot it answers `library_missing`,
and Hamlet sends the request again with `library.files`: relative
`path`, base64 `data` and permission `mode` for each regular file.
Links, special files, more than 1024 entries and more than 4 MiB are
refused, and files whose digest is not the named one are not kept. Only
library files are transferred, never the server's credentials, private
librarian notes or house folders. A snapshot unused for 15 minutes is
swept. Publication and removal change the digest, so they affect the
next tool call; a running operation keeps its snapshot. Failed snapshot
reads prevent the operation from being sent. Hamlet still owns librarian
judgment, starter installation and skill publication/removal, including
confined staging from remote houses.

## Network grants

Each operation has a private network namespace. A denied operation has
no proxy socket or external route. A granted operation gets a Unix
socket connected to the worker's HTTP proxy and a loopback relay on
port 3128, advertised as `HTTP_PROXY` and `HTTPS_PROXY`. The proxy
accepts public IPv4 and IPv6 destinations on ports 80 and 443 only,
rejects private, loopback, link-local (metadata), shared and transition
addresses, and connects to the exact checked DNS result. Direct
external sockets and UDP remain unavailable. Clients must honor the
proxy environment.

The check is by address alone. A machine on a network that routes
public-looking addresses privately needs those ranges blocked outside
the worker before network grants are used; the worker has no list of
extra ranges.

The relay is part of the operation's process tree and cannot outlive
its completion barrier. Proxy connections have a 120-second lifetime,
a 16 KiB header limit and a 64-connection worker-wide bound. Killing the
relay closes the corresponding upstream connections. CONNECT permits
a byte tunnel on the allowed ports; it does not inspect TLS or promise
that the tunneled application is HTTP.

## Tests

On Linux, from the root:

```sh
cargo test -p hamlet_workspace
```

These cover the worker's tokens, its journal across restarts, its
answers to the shared fixture's refusals, its no-follow file operations
and its address checks. Hamlet's server tests against this protocol
with a loopback double of the worker, in
`crates/hamlet_server/tests/crashes/remote.rs`:

```sh
npm ci --prefix harnesses/pi
cargo test -p hamlet_server --features harness --test crashes remote::
```

They cover remote house notes, prompts, file and skill gifts, agent
creation, merge/split, library publication and removal, copies between
workspaces, manual house-write approval, network grants, library
snapshots, cancellation by request ID, uncertain results and refusal of
host paths without local fallback. No automated test runs the server
against the real worker.
