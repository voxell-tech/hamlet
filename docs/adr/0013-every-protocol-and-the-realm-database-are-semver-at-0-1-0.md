---
status: accepted
---

# Every protocol and the realm database are semver, at 0.1.0 until the first release

The client protocol, the runtime protocol, the workspace protocol and the
realm's database each carry a version in semver, `major.minor.patch`
(`hamlet_protocol::Version`). It is checked where two sides meet: a client's
`hello`, a runtime's `hello`, a workspace worker's handshake, and the opening
of `realm.sqlite`. Two versions are compatible by semver's caret: the same
major version and, below 1.0, the same minor one. A peer or a database whose
version is not compatible with the server's is refused in words for a chief.

Every one of them stays at 0.1.0 until Hamlet's first release. None moves
before it, whatever changes. After it, a change that breaks a peer moves the
version by semver. The generated schema pi reads exports the runtime
protocol's `version`.

## Considered options

- An integer for each protocol, moved by hand with every change of shape, as
  before. Rejected: it can only say "the same" or "not the same", and before a
  release it moved with nearly every change, telling nobody anything.
- A fingerprint of the schema as the version. Rejected: a fingerprint says
  something changed, not whether a peer can still be understood. The client
  schema's fingerprint stays as a check that a change to the types was
  noticed, but it forces no version to move.
- No version in the database, since a realm is made by one server. Rejected:
  a server must refuse a realm it cannot read, and say so, rather than
  misread it.

## Consequences

- Realms made before the rebuild are not carried over: their databases do not
  have this schema, and there is no upgrade path, only a refusal.
- A change to a message's shape needs no version bump, and a peer built from
  an older checkout may fail in other words than a refusal. That is accepted
  until the release.
- At the first release each version is set on purpose, and from then a
  breaking change to a protocol or the schema needs a new major version, or a
  new minor one while it is below 1.0, and a way to read the old.
