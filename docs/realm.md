# The realm

The server keeps one realm, `~/.hamlet` unless `HAMLET_HOME` or
`hamlet_server --home` selects another folder for it. Use a fresh scratch
folder for development. The realm is kept by actors, each in a file of its
own under `actors/`; the folder's layout is
[A4 of the actor runtime](spec/actor-runtime.md#where-things-are-stored).
The server's chief is named by `HAMLET_USER`, else the account it runs as
(`USER`, `USERNAME` or `LOGNAME`), else "chief" (`chief_name` in
`crates/hamlet_server/src/serve.rs`).

A new realm is made with its first chief and its Hand. It has no hamlet: a
chief creates the first with `/create-hamlet`, or asks the Hand to. The
library is made with the starter skills as its first commit.

The workspace reads `tools/` whenever an agent asks for its tool list
(`workspace/tools.rs` in `hamlet_actors`), so a tool a chief puts there by
hand reaches the agents without a restart.

## Claim and storage

`serve` in `crates/hamlet_server/src/front.rs` resolves the folder to its
canonical path, and `Claim::take` (`claim.rs`) takes a local socket claim on
that path, so a second server cannot open the same realm under another
spelling. The hosts then lock the folder and each kind of actor they run
(A6).

Each actor's file is a plain SQLite database
([ADR 0015](adr/0015-each-actor-keeps-its-data-in-one-plain-sqlite-file.md)).
The realm actor's tables are in `crates/hamlet_actors/src/realm/schema.sql`,
and its `realm` row holds the schema's number. A change an actor makes is
one transaction of its own file; work that spans actors is a row of steps
in the realm (A12). A call made again under the same id gets the first
answer (A9). Back up the realm folder whole, with the server stopped: the
actors' files, `profiles/` and `library.git` belong together.

A client request whose handling fails is answered with the failure, and the
server goes on.

## Profiles and agents' filesystems

A profile's repository holds `CHARACTER.md`, `memory/` and `skills/`;
`main` is what the profile knows, `hires/<agent id>` is a hire's branch,
and a tag is a version (ADR 0011). Its `profile` actor alone writes it,
through `hamlet_git`.

An agent's filesystem, `actors/agent/<n>.fs/`, is filled from its branch
without `CHARACTER.md`, plus `inbox/`, which the profile never takes
(`hosts/node/src/agent/branch.ts`). After a turn, the agent sends the
profile what its filesystem changed and then pulls what the branch holds
now. The profile takes `main` into each branch that is not pinned
(`follow_main` in `crates/hamlet_actors/src/profile/hires.rs`), so a turn
never sees `main` change under it. The character is read from the
branch's tip for the prompt, and edited on a profile's `main` through the
client protocol.

## Credentials and documents

`credentials.json` is mode 0600 and replaced whole through a temporary file
and rename. It holds `installationId`, `connection: {token}`, the provider
credentials and the remote workspace chosen in the settings. The server's
`Credentials` (`crates/hamlet_server/src/credentials.rs`) is the only reader
of `HAMLET_API_KEY`. With it set, the settings the server shows count the
settings' model's endpoint as configured. The key is not written to the
file, and the Node host is not given it (`ENVIRONMENT` in
`crates/hamlet_host/src/child.rs`).

ChatGPT and OpenRouter support subscription sign-in. ChatGPT credentials
are separate from OpenAI API keys.

The realm actor holds the chief's documents: the charter and the model
catalogue's overrides as rows of the `document` table
(`realm/documents.rs`), the settings as the `settings` row
(`realm/settings.rs`), the realm's limits in its `realm` row, and a
hamlet's limits in its `hamlet` row, so a limits edit is one change. A
character is a file of its profile's `main`, and memory is files of an
agent's filesystem or of `main`.

`HAMLET_MODEL`, `HAMLET_BASE_URL`, `HAMLET_REFLECT_EVERY` and `HAMLET_NET`
override the kept settings for this process and are never kept
(`Overrides` in `realm/settings.rs`). `HAMLET_MODEL` is the settings'
model, so an agent's or hamlet's own choice still stands over it.
`HAMLET_BASE_URL` is the address of that model's endpoint, which it adds
when the settings have none of that name. A realm starts with no endpoint
and no model. Setup is done once the chief picks a model; the window's
This computer adds Ollama's endpoint, `local` at
`http://localhost:11434/v1`, when it is picked.

The settings also hold when dreams run: whether they are on, the local hours
they start in, and the most one may cost.

## Starting and stopping

`hamlet_server --home <folder> --port 0` publishes a free loopback address
in `server.json` once the realm answers. `serve` in `front.rs` owns the
ordered start and stop, `clients/connection.rs` the sockets, and
`front/route.rs` what each client message becomes. `--faux` selects an
offline response script for the agents' models.

SIGTERM or Ctrl-C closes clients with code 1001, settles the Hand, stops
the Node host and the actors, and removes `server.json`. Closing a client
or window does not stop the server.

## Starter skills

`crates/hamlet_server/assets/starters/` supplies the starter skills, which
the library actor embeds (`crates/hamlet_actors/src/library.rs`). Beside
them, `crates/hamlet_server/assets/charter.md` is the default charter and
`crates/hamlet_server/assets/hand.md` the Hand's first character. The
library's first commit holds the starters once; a skill taken out or
changed later stays so.
