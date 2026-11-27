---
status: accepted
---

# Crates that hold ported code are Apache-2.0 only

The actor core and the agent's wrapper are ported from projects under the
Apache License 2.0. A crate or package that holds code copied or adapted
from them is licensed `Apache-2.0` alone, not under the workspace's
`MIT OR Apache-2.0`. Each one has a `LICENSE` with the Apache-2.0 text and a
`NOTICE` that names the origin project, its address, the commit it was taken
from, and the origin's copyright line word for word. The `NOTICE` also says
that the files were changed. Each changed file says so on its first line. The
`NOTICE` is the only kind of file in the repository that names an origin
project.

The crates under this rule are `hamlet_actor_core`, `hamlet_actor`,
`hamlet_actor_client`, `hamlet_actor_wire`, `hamlet_actor_error`,
`hamlet_actor_error_macros` and `hamlet_actor_node`, and the packages are
`hosts/node/src/actor` and `hosts/node/src/pi`. Code written new for Hamlet
stays under the workspace licence, even when it builds on them:
`hamlet_host`, `hamlet_actors`, `hamlet_messages` and `hamlet_killkit` are
such code. Code is moved into an Apache-only crate when it is adapted from
the origin, as the core's `host/running.rs` was. When it is unclear whether
code is adapted, it goes under the stricter licence.

## Considered options

- Relicense the ported code under the workspace's dual licence. Rejected:
  Apache-2.0 does not allow the origin's code to be offered under MIT.
- Copy the origin unchanged under `vendor/` with patch files. Rejected: it
  left the origin's name in nearly every file, in every actor's table names
  and in the manifests, and Hamlet's checks could not cover it.
- One `NOTICE` at the repository's root. Rejected: a crate is what a reader
  takes alone, so each crate carries its own.
