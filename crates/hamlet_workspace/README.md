# hamlet_workspace

One binary, `hamlet-workspace`, with two long-running roles:

- `manager CONFIG` runs outside any actor. It authenticates Hamlet,
  binds each workspace to a Substrate actor through the control API,
  signs every request and forwards it through the atenet router.
- `worker CONFIG` runs inside the actor. It verifies the manager's
  signature and runs each file or process operation confined by
  bubblewrap, with only the calling agent's house and, when granted,
  the shared project writable.

`keygen NEW_KEY_FILE` writes a new Ed25519 private key (mode 0600,
refuses to overwrite) and prints its base64 public key.

`delete CONFIG HOME WORKSPACE EXPECTED_UID` is the administrative deletion
described under [Deleting a workspace](#deleting-a-workspace).

Every role is Linux-only. Built for another system, the binary only
reports that it is unsupported, so the Cargo workspace still builds
there.

This page covers packaging, configuration and operation. What agents
and the chief see is in [`docs/sandbox.md`](../../docs/sandbox.md), and
the protocol is the `hamlet_workspace_wire` crate
([`../hamlet_workspace_wire/src/lib.rs`](../hamlet_workspace_wire/src/lib.rs)),
which Hamlet's server uses too, with example messages and replies in
[`fixtures/protocol-v1.json`](../hamlet_workspace_wire/fixtures/protocol-v1.json).
Deployment requires an operator-provisioned Substrate installation.

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

The image builds locally. Its runtime smoke check exercises key creation
and permissions, the packaged tools, worker startup and rejection of an
unsigned request. That check does not establish confinement inside the
deployed actor; the real-worker acceptance below covers local confinement.

The build stage copies the whole Cargo workspace, because Cargo needs
every member to load it, then compiles only this crate with
`cargo build --locked --release -p hamlet_workspace`. It fails if
`Cargo.lock` would change.

The runtime image is Debian 12 slim with:

- `/usr/local/bin/hamlet-workspace` as the entrypoint, with no default
  role or arguments;
- `bubblewrap` pinned to `0.8.0-2+deb12u1` at `/usr/bin/bwrap`, the
  path the worker calls and the build the local runsc probes used;
- `bash`, coreutils, findutils, grep, sed, gawk, diffutils, patch, file,
  tar, gzip, xz, procps, less, git, curl, Python 3 and ripgrep for agent commands
  and the shipped starter skills;
- `socat` for the confined operation's loopback-to-Unix proxy relay;
- `ca-certificates` for the manager's HTTPS router and gRPC control
  clients.

Both base images are pinned by index digest in the `ARG` lines. Commands
run with only `/usr`, `/bin`, `/lib` and `/lib64` mounted read-only, so
a tool is available to agents only if it is installed under those paths.
Only `/etc/ssl/certs/ca-certificates.crt`, the public CA trust bundle,
is additionally mounted read-only for network-granted commands. Other
`/etc` contents remain hidden.

The image holds no configuration, keys or tokens. Each role reads one
JSON file named by its second argument, normally mounted by the
deployment:

```sh
docker run --rm \
  -v "$PWD/manager.json:/etc/hamlet-workspace/manager.json:ro" \
  -v "$PWD/signing-key:/run/secrets/hamlet-workspace/signing-key:ro" \
  -v hamlet-workspace-state:/var/lib/hamlet-workspace \
  hamlet-workspace:local manager /etc/hamlet-workspace/manager.json
```

Both files reject unknown fields, so they cannot hold comments. Examples
with placeholders are in [`examples/`](examples). They show one setup,
Substrate on GKE, and are not an official deployment.

## Keys

Generate the signing key where the manager's secrets are kept, not in
an actor:

```sh
hamlet-workspace keygen ./signing-key   # prints the public key
```

The private key goes only to the manager. The printed public key goes
into the worker configuration. It is not a secret.

## Manager

The manager holds every credential and all durable state. One manager
serves any number of Hamlet homes. Each home has its own token, and the
token alone tells the manager which home is calling: two homes never
share an actor, a file or an operation record, even when their hamlets
have the same names. See
[`examples/manager.example.json`](examples/manager.example.json).

| Field | Meaning |
|---|---|
| `listen` | Plain HTTP address for `POST /v1`. |
| `state` | Durable state directory, created with mode 0700. |
| `homes` | Each home's stable name (letters, digits, `-`, `_`, at most 128) and the bearer token its Hamlet sends: at least 16 characters, different for every home. |
| `signing_key` | Path to the PKCS#8 Ed25519 key from `keygen`. |
| `control` | Substrate control gRPC endpoint. |
| `control_token` | Optional bearer token for the control API, or `null`. |
| `control_token_file` | Optional rotating bearer-token file, read before every control RPC. Mutually exclusive with `control_token`. |
| `control_tls` | Optional `{ "ca": PEM_FILE, "server_name": DNS_NAME }` for private control-plane TLS. |
| `router` | atenet router base URL; requests go to `{router}/v1`. |
| `router_ca` | Optional PEM CA bundle for the private router. The URL's hostname must match its certificate. |
| `atespace` | Atespace of the actors and template. |
| `template` | Actor template every new workspace actor uses. |
| `idle_seconds` | Optional seconds without operations before a workspace's actor is suspended; default 300, never 0. |

Homes. To add a home, give it a name and a new random token in `homes`
and restart the manager, then give its chief the manager's address and
that token for Workspace in Hamlet's Settings. A home's name must never
change: its actors are found by it. A home taken out of `homes` is
refused from the next start; its actors and files stay until an operator
deletes them.

Secrets. The homes' tokens and `control_token` sit in the configuration file, so
mount the whole file from a secret store, read-only. Mount
`signing_key` as a separate secret file. None of these go into an
actor, an image, or Hamlet beyond the manager's address and token,
which Hamlet keeps with its credentials.

Durable state. `state` holds a lock, actor bindings (name, immutable
UID, last lifecycle intent and any deletion tombstone per workspace)
and, under `homes/<home>/`, a record per operation id: its route,
written before forwarding, and its settled reply once known. A manager
from before `homes` kept its operation records at the top of `state`;
it refuses to start until they are moved into `homes/<the home it
served>/`. Its bindings stay valid under the same home name. Put it on a
dedicated persistent volume. Run exactly one manager: it takes an
exclusive `flock` on `state/lock` and exits if another manager holds it,
but that only works when both see the same local filesystem. Use a
single replica with a stop-before-start rollout and a single-writer
volume. Losing `state` loses the bindings; the manager will not silently
replace an existing actor.

Control. The manager calls Substrate's `Control` gRPC service
(`GetActor`, `CreateActor`, `ResumeActor`, `SuspendActor`, and
`DeleteActor` for the delete command) directly, with no `kubectl` and
no Kubernetes credentials. The first authorized operation for a
workspace creates its actor from `template` and records its UID; a
lost create reply is settled by `GetActor`, never by a second actor.
An operation for a suspended actor resumes it first. After
`idle_seconds` without operations, and only with none in flight or of
unknown outcome on it, the manager suspends the actor. Bindings,
suspension intent and operation records are written to `state` before
the call they describe and settled after it, so after an interruption
an unresolved suspension is checked again on the next request or idle
check instead of being guessed. Operations of unknown outcome are
settled by a signed `status` to the worker and are never sent again. A
missing actor or one whose UID or template changed is refused, never
replaced.

Cancellation. A `cancel` received before `execute` writes a durable
cancellation record before acknowledging it. That id can never be admitted,
including after restart, and its `execute` and `status` return `aborted`.
It does not create an actor. Cancellation of an already admitted operation
still goes to the worker and waits for its completion barrier before the
execution result settles.

Exclusive control. `ResumeActor` and `SuspendActor` address the actor
by name only. The manager checks the UID in each reply and refuses to
continue on a mismatch, but it cannot make the call itself conditional:
if something else deleted and recreated an actor of the same name in
between, that other actor would already have been resumed or
suspended. Atenet routes by `<atespace>/<actor-name>`; the signed body
carries the expected UID, which the worker checks against its live
SystemInfo file before any operation. A replacement cannot execute it,
although routing may wake that replacement. Deletion carries a UID
precondition. The operator
must therefore ensure that nothing but this manager creates, deletes,
resumes or suspends actors named `hamlet-…` in `atespace`.

`control` and `router` must be
`https://`, or `http://` on `127.0.0.1`, `[::1]` or `localhost`, and may
not embed credentials. HTTPS uses system roots unless `control_tls`
supplies the control plane's CA and certificate name. For a GKE private
port-forward, use `https://127.0.0.1:18443`, `server_name` of
`api.ate-system.svc`, and the live `servicedns.podcert.ate.dev/identity`
ClusterTrustBundle. The control token's audience is `api.ate-system.svc`.
In a pod, use `control_token_file` with a projected service-account token;
Kubernetes rotates it and the manager reads the replacement on its next RPC.
A missing or empty file fails closed. Static `control_token` users must refresh
it and restart before expiry. TLS verification is never disabled.

TLS. The manager serves plain HTTP and does not terminate TLS. Bind
`listen` to loopback and put TLS in front of it outside the binary: a
sidecar or ingress in the same pod, or a local port-forward. Hamlet
accepts only an `https://` manager URL or loopback `http://`. Do not
expose the plain port on a network.

The image runs as root by default. The manager needs no capabilities
and can run as any user that owns `state`.

The example [`examples/gke-manager.yaml`](examples/gke-manager.yaml) runs one non-root manager
with a 1 GiB persistent volume, a projected control token and the cluster's
service CA. Supply its immutable image and `hamlet-workspace` Secret separately.
The secret holds `manager.json` and `signing-key`. Configure the manager to
listen on `127.0.0.1:7420`; no Service or public endpoint is needed. Connect with
`kubectl -n ate-system port-forward deployment/hamlet-workspace 7420:7420`, then
choose the loopback URL and the manager's bearer token under Workspace in
Hamlet's Settings. The laptop needs
Kubernetes authentication for the tunnel; Hamlet itself has no cloud credentials.
Stop the old manager before transferring its state and starting this one.

## Deleting a workspace

Hamlet never deletes a workspace. The operator does, with the manager
stopped:

```sh
hamlet-workspace delete /etc/hamlet-workspace/manager.json HOME WORKSPACE EXPECTED_UID
```

`HOME` is the home's name in `homes`, `WORKSPACE` the hamlet's workspace
name and `EXPECTED_UID` the actor UID recorded in its binding, a
`state/actor-*.json` file. The command
takes the same `state/lock` as the manager, so it fails while a manager
runs on that state. It refuses unless the binding holds that UID, writes
a tombstone, asks Substrate to delete the actor with the UID as a
precondition, and marks the tombstone completed. If the deletion fails
or its outcome is unknown the tombstone stays requested and the command
can be repeated. A tombstoned workspace is never recreated: the manager
refuses its requests from then on.

The tombstone lives only in `state`. Restoring `state` from a backup
older than the deletion brings back the old binding; the manager then
finds the actor missing and refuses rather than recreating it. Worker
files are only as durable as the actor's last successful snapshot, so an
unexpected worker loss can roll them back, and the worker's own
operation journal with them, even after a reply recorded them as
written.

## Worker

The worker holds no private credentials: no signing key, no tokens, no
model or cloud credentials. See
[`examples/worker.example.json`](examples/worker.example.json).

| Field | Meaning |
|---|---|
| `listen` | Address the router reaches inside the actor. |
| `root` | Durable data root, created with mode 0700. |
| `public_key` | Base64 Ed25519 public key printed by `keygen`. |
| `actor_uid_file` | Read-only SystemInfo projection of `ACTOR_METADATA_FIELD_UID`. |

The worker accepts a request only with a valid manager signature less
than 180 seconds old and a matching actor UID, including on loopback.
It reads the UID file on every request because a restored snapshot's
memory can still hold the source actor's identity. Houses live at
`{root}/{home}/{workspace}/houses/{agent}`, the shared project at
`{root}/{home}/{workspace}/project` and the operation journal at
`{root}/{home}/operations`. It holds `{root}/lock` while running.

Shared skills. Hamlet owns the library on the server, outside the remote
houses. Its confined shelf reader takes a snapshot of the shelf for each
tool call and names it on an execute request by `library.sha256`, the
digest of its files. The worker keeps each snapshot it was given under
`{root}/{home}/.library/{sha256}` and binds it read-only at `/library`.
When it does not hold the named snapshot it answers `library_missing`, and
Hamlet sends the request again with `library.files`: relative `path`,
base64 `data` and permission `mode` for each regular file. Links, special
files, more than 1024 entries and more than 4 MiB are refused, and files
whose digest is not the named one are not kept. Only library files are
transferred, never the server's credentials, private librarian notes or
house folders. A snapshot unused for 15 minutes is swept. Publication and
removal change the digest, so they affect the next tool call; a running
operation keeps its snapshot. Failed snapshot reads prevent the operation
from being sent. Hamlet still owns librarian judgment, starter installation
and skill publication/removal, including confined staging from remote
houses.

[`examples/template.example.json`](examples/template.example.json) is the
protobuf JSON body for `CreateActorTemplate`. Replace its image digest,
snapshot URI and existing gVisor config placeholders before submission.
It is not deployment authorization. It limits one actor to 1 CPU and
512 MiB, uses a full snapshot, and adds only SETFCAP to the pinned
default capabilities. The configuration image contains only
`worker.json` at its root, with the public verification key; never
copy the manager configuration or private key into it.

Actor template fields from the pinned `ateapi.proto`:

- `containers[].image`: this image, pinned by digest (required by
  Substrate);
- `containers[].args`: `["worker", "/etc/hamlet-workspace/worker.json"]`,
  leaving `command` unset so the image entrypoint is used;
- a `durable_dir` volume mounted at `root`, for example `/workspace`;
- the configuration file delivered by a read-only `image` volume
  mounted at `/etc/hamlet-workspace`, or baked into a derived image.
  Changing either image reference invalidates existing snapshots;
- a read-only `system_info` volume projecting `ACTOR_METADATA_FIELD_UID`
  at `/run/ate/actor-uid`, outside all operation mounts;
- `listen` on the port the router targets. The example uses port 80,
  matching the existing shell template's `PORT=80`; which port the
  router uses for this template is not verified here;
- `wakeup_probe` requests GET `/v1` on port 80 for up to 60 seconds.
  The worker returns HTTP 200 with a protocol-error body to this
  unauthenticated GET, without executing an operation. This makes
  resume wait for the listener rather than racing the first command.

### Capabilities

The pinned Substrate default capability set for an actor container is
exactly `AUDIT_WRITE`, `KILL` and `NET_BIND_SERVICE`, with the process
running as UID 0 (`cmd/atelet/oci.go`, `internal/ocispec/ocispec.go`).

That set is not enough. For every operation the worker creates a user
and network namespace and maps UID 0 inside it to its own UID 0. gVisor
requires `SETFCAP` to map the parent's UID 0. In the local runsc probes
with the default set, bubblewrap failed with
`setting up uid map: Operation not permitted`; with `SETFCAP` added it
succeeded. Those probes needed no `SYS_ADMIN` and no privileged host
helper, and commands then ran with all five capability sets at zero
after `--cap-drop ALL`.

The worker therefore needs this, and only this, added to the default:

```json
"securityContext": {"capabilities": {"add": ["SETFCAP"]}}
```

Granting `SETFCAP` in any actor template needs
the chief's approval, and the isolation it is meant to provide still
needs validation on that deployment. Without it the worker fails
closed on the default set: operations return a `confinement` error and
never run unconfined. The local probes used the exact runsc release
binary in an OCI fixture with `--network=none`, not Substrate's real
mounts, networking, resource limits or checkpointing.

## Pinned Substrate source

The control client is generated at build time from
[`proto/ateapi.proto`](proto/ateapi.proto), a copy of
`pkg/proto/ateapipb/ateapi.proto` at Substrate v0.4.0,
[756c2a53741121e728f4cc3066c8a19e575b4919](https://github.com/agent-substrate/substrate/tree/756c2a53741121e728f4cc3066c8a19e575b4919),
the pin in [the proposal](../../docs/proposals/remote-workspaces.md).
Its SHA-256 is
`4baf360853a9a61085c7a9d86b3b3a720e094834eb866c5c76e4a0900f142f31`.
Update the copy and this pin together. `protoc` comes from
`protoc-bin-vendored`, so the build needs no system protobuf compiler.

## Network grants

Each operation has a private network namespace. A denied operation has
no proxy socket or external route. A granted operation gets a Unix
socket connected to the worker's HTTP proxy and a loopback relay on
port 3128, advertised as `HTTP_PROXY` and `HTTPS_PROXY`. The proxy
accepts public IPv4 and IPv6 destinations on ports 80 and 443 only,
rejects private, loopback, metadata and transition addresses, and
connects to the exact checked DNS result. Direct external sockets and
UDP remain unavailable. Clients must honor the proxy environment.

The proxy also rejects GKE's reserved internal Service range,
`34.118.224.0/20`. Deployments using other public-looking addresses for
private networks need additional exclusions before enabling network grants.

Substrate's egress gateway is a separate restriction. An actor without an
`EgressPolicy` has no outbound access, even for a network-granted operation.
The manager does not create that policy. An operator must approve and install
the allowed destinations on the actor. TLS passthrough preserves the remote
server's certificate; intercepted HTTPS additionally requires trusting the
gateway CA. Do not disable certificate verification to bypass that requirement.

The pinned gateway resolves TLS SNI again, so the worker's checked address is
not necessarily its destination. Broad TLS passthrough in the demo therefore
does not guarantee public-internet-only access. There is no additional gateway
proxy. Do not put sensitive credentials in actors or treat this prototype's
network grants as a production infrastructure-isolation boundary.

The relay is part of the operation's process tree and cannot outlive
its completion barrier. Proxy connections have a 120-second lifetime,
a 16 KiB header limit and a 64-connection worker-wide bound. Killing the
relay closes the corresponding upstream connections. CONNECT permits
a byte tunnel on the allowed ports; it does not inspect TLS or promise
that the tunneled application is HTTP.

## Local acceptance

From the root, with the Node version in `harnesses/pi/.nvmrc`:

```sh
cargo test -p hamlet_workspace
npm ci --prefix harnesses/pi
CARGO_INCREMENTAL=0 cargo test -p hamlet_server --features harness --test server remote::
```

The server checks start the server and pi with faux model responses.
An authenticated loopback manager double records workspace-v1 requests
and operates disposable file trees. These checks cover remote house notes,
prompts, file and skill gifts, agent creation, merge/split, library publication
and removal, backend copies, manual house-write approval, network grants,
library snapshots, cancellation by request ID, uncertain results and refusal
of host paths without local fallback. Copies check binary contents, modes,
empty directories and skipped links. Backend selection stays fixed until
restart even when the Settings target changes.

The manager's separate Rust tests use an offline control double for lifecycle
races, uncertain creation/suspension and UID-checked deletion. Neither suite
proves the server-to-deployed-worker path, Substrate snapshots, cluster network
isolation or recovery on real infrastructure. Those need a separately approved
deployment and an operator's acceptance matrix on a disposable home. No live
infrastructure test is run by the commands above.

## Not established

- Successful writes are not synchronous snapshots; worker loss may roll
  acknowledged files back to the last completed snapshot.
- Network grants require both the worker's per-operation permission and
  the deployment's actor-wide policy. An address-only check cannot identify
  arbitrary privately routed public ranges.
- One deployment's acceptance does not establish behavior on another runtime,
  topology or version. Repeat the live isolation and recovery matrix there.
