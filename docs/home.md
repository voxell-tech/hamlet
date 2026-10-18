# The home

Everything the server keeps is in one folder, `~/.hamlet`. `HAMLET_HOME`
moves it elsewhere; development uses a scratch home. The server opens a
fresh home and does not import a home of the retained Rust runtime
([`core.md`](core.md#the-retained-runtimes-home)). The layout is
`server/src/home/layout.ts`.

| Path under the home | Holds |
|---|---|
| `home.sqlite` | pi-durable's storage: conversations, characters, the hamlet's record, oversight, spend, limits, settings and the catalogue |
| `credentials.json` | The installation ID, connection token and model credentials |
| `models.json` | pi-ai's stored model lists |
| `server.json` | The running server's URL, process ID and start time, with no token |
| `server.log` | A background server's output |
| `houses/<hamlet>/<number>/` | The agent's writable files: `NOTES.md`, `skills/` and `inbox/` |
| `library/` | Shared skills, starting with the starters, readable by agents |
| `librarian/` | The librarian's own notes |
| `tools/` | Tools added by hand, readable by agents |
| `.gifts/`, `.erasing/` | Private staging for gifts, and houses being erased |

The first hamlet is `home`. Its founder is agent number 0, named `founder`,
with house `houses/home/0/`; `mayor` addresses the office it holds.
Character and conversations are in storage, not writable files in the
house.

One process holds the home's claim: a local socket named from the home's
path, as the Rust app names it, for both the path as given and its
canonical form (`server/src/home/lock.ts`). A second server on the same
home, by any spelling of its path, is refused (S2).

The Rust server keeps its record in `realm.sqlite`, paired with `realm.id`.
It checks that both name the same home before serving it. A missing or
mismatched part is refused; restore the complete home from backup rather
than deleting an identity to open it. Pre-identity Rust databases acquire
the pair on their first successful open. A TypeScript `home.sqlite` is
refused even when a `realm.sqlite` is beside it.

## Credentials

`credentials.json` has mode 0600 and is replaced whole through a temporary
file and a rename. It holds `installationId`, `connection: {token}` and
`models: {provider: credential}`, and the remote workspace's token when one
is chosen. pi-ai receives no ambient credentials; `HAMLET_API_KEY` is held
in memory only. ChatGPT uses its own `chatgpt` provider and credential,
beside an `openai` key, with Hamlet's installation ID and name. ChatGPT and
OpenRouter are the offered sign-ins; other providers use keys.

Tool results, protocol messages and the oversight record are scrubbed of
known credentials.

## The Rust server's record and documents

`hamlet-server --home <folder> --port 0` serves protocol v0 on a free
loopback port and publishes `server.json`. It uses the same claim,
credentials shape and stable connection token. `HAMLET_HOME` selects the
home when `--home` is absent. SIGTERM or the first SIGINT closes clients
with code 1001 and removes the publication; a second SIGINT exits 130.

Its `realm.sqlite` holds the record and chief documents. Characters
belong to recorded agents; `chief_document` holds the charter, limits,
settings and work-folder choices. A limits edit commits its document and
the record's constraints together. Notes stay in house files, opened
relative to the claimed home without following links. `HAMLET_MODEL`,
`HAMLET_BASE_URL`, `HAMLET_REFLECT_EVERY` and `HAMLET_NET` override this
process's settings without changing their stored values.

`crates/hamlet-server/src/service.rs` composes requests and record-backed
state; `runtime.rs` owns authentication, ordered connections and
shutdown. The Node harness script's path is retained in `runtime::Options`
as `faux`. The CLI's default server selection remains in
`hamlet_cli/src/process.rs`.

## Starter skills

`server/assets/starters/` holds the starter skills. The library starts
with them, and a chief gives one to an agent through `give`.
