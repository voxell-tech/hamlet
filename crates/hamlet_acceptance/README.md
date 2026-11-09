# hamlet_acceptance

The acceptance suite for Hamlet's server. Each scenario starts a server
process on a scratch realm and checks what it does from outside: through
the client protocol over the authenticated WebSocket, with `hamlet_protocol`'s
generated types, and through the `hamlet` command. It checks behaviour
only, never the server's insides. The default server is `hamlet server`.

Every message a server sends is parsed into `ServerMessage`, written back
and compared with what arrived, so a field outside the client protocol fails the
scenario that received it.

## Running it

The scenarios are compiled only with the `acceptance` feature, so plain
`cargo test` neither builds nor runs them. They need the built `hamlet`
and `hamlet_server`, Node from `harnesses/pi/.nvmrc`, and
`npm ci --prefix harnesses/pi`. `cargo xtask check` runs them. By hand, at the root:

```sh
cargo build -p hamlet_cli -p hamlet_server
cargo test -p hamlet_acceptance --features acceptance
```

They never contact a model server or the internet. Models are faux, or the
loopback endpoint in `hamlet_testing`'s `src/openai.rs`.

| Variable | Selects |
|---|---|
| `HAMLET_ACCEPTANCE_SERVER` | The command that starts a server, split on whitespace. Unset or empty: the `hamlet` command's `server`. |
| `HAMLET_CLI` | The `hamlet` command, for the default server and the CLI scenarios. Unset: `target/debug/hamlet`. |

A server that cannot be started fails its scenario, with the end of what
it printed. Nothing is skipped.

## Starting a server

The suite runs the command with `--port 0` and `--faux <file>`, or, for
the scenarios that use the loopback endpoint, without `--faux` and with
`HAMLET_MODEL` and `HAMLET_BASE_URL` set. Its environment is the suite's
own with the ambient overrides named by `hamlet_testing::OVERRIDES`
removed. It then sets `HAMLET_HOME` to a scratch folder and
`HAMLET_REFLECT_EVERY=0` unless a scenario says otherwise. The server
is ready when `<realm folder>/server.json` names the process the suite
started.
Stdin is closed; stdout and stderr go to a log shown on failure.

## Faux scripts

`Faux` writes the JSON the server's faux model reads:

- `agents`: answers by agent id, as in `Faux::agent(2, [...])`. The Hand
  is 1; the first mayor is 2.
- `kinds`: answers by conversation kind (`worker`, `errand`, `letters`,
  `questions`, `dream`), as in `Faux::kind("dream", [...])`.
- `responses`: answers for everyone else.

A conversation takes its agent's list first, then its kind's, then
`responses`. An exhausted list answers with an error and never falls back.
A faux model costs nothing, so the spend scenarios use the loopback
endpoint, which bills a fixed price per answer.

## Scenarios

| File | Covers |
|---|---|
| `connection` | Token, version, hello order, malformed and oversized messages, shared changes, shutdown, one server per realm. |
| `city` | The empty realm, founding hamlets, the hire tree, removing, restoring and erasing scholars and hamlets, hamlet administration. |
| `chats` | Answers, sessions, `/new`, `/resume`, `/retry`, `/stop`, `Rewind`, history, reflection. |
| `oversight` | Network approvals and grants, manual and auto modes, `ask_chief`, objections. |
| `society` | The Hand's and a lead's tools, tell and ask, workers, reserved acts, roads. |
| `documents` | Charter, character, memory, limits, settings, catalogue, and the prompt that follows them. |
| `spend` | The realm budget, a task's cap, a hire's allowance. |
| `sandbox` | Confinement of reads, writes and the network. |
| `restart` | Waits and the realm across a crash and a stop. |
| `models` | Model and connection checks, key sign-in, listings and prices, refusals. |
| `oauth` | Sign-in prompts and token rotation. |
| `gifts` | Files handed over in parts to an inbox, skills given to a scholar from an agent or another scholar. |
| `versions` | `/tag`, `/versions`, `/pin`, `/revert`, `/fork` and `/purge`. |
| `merges` | `/merge` and the split by `/restore`. |
| `library` | `submit_skill` judged by the librarian, `ask_librarian` and `/unshelve`. |
| `review` | A lead's review of a chief's line to a hire, and a chief's `/worker`. |
| `dreams` | `/dream` of a hire and of a scholar. |
| `cli` | `hamlet chat`, `hamlet edit`, switching agents, approvals across a crash, and the bare, dev and insecure commands. |

## Recording protocol frames

`tests/frames/` records 19 real server sessions through the client protocol:
the Hand's chats, a hire's approvals, reads and edits, sign-in, spend, file
and skill gifts, letters and questions, leaving and merging, hamlets and
roads, the library, and the questions and objections that wait for a chief.
It uses scratch realms, faux responses and a loopback endpoint, never a live
provider. Both wire directions are saved as JSONL; connection tokens and the
fake sign-in key are redacted. IDs, timestamps and stream boundaries come
from the running server.

From the repository root, using the Node version in `harnesses/pi/.nvmrc`:

```sh
npm ci --prefix harnesses/pi
cargo build -p hamlet_cli -p hamlet_server
cargo test -p hamlet_acceptance --features acceptance --test frames -- --ignored
cargo test -p hamlet_protocol -p hamlet_client
```

For a custom Cargo target directory, set `HAMLET_CLI` to the absolute path of
that build's `hamlet`. Recording overwrites
`crates/hamlet_protocol/frames/*.jsonl`; ordinary tests never do. To inspect
new captures without replacing the checked-in fixtures, set
`HAMLET_FRAME_DIR` to an absolute scratch directory for the recorder and
the schema package's `npm test`. Rust client replay reads the checked-in
fixtures. Review replacements before keeping them; changing server code
does not by itself require fixture churn.
