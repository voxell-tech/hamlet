---
status: accepted
---

# The home is the server's store, the harnesses' stores and files

A home keeps Hamlet's records in one store of the server's: the hamlets'
records, the realm, what waits and how it was answered, spend, the
library's record, the chief's documents (limits, charter, characters,
allowed models, settings) and which harness each conversation runs in.
Each harness runtime keeps its conversations in a store of its own. Each
store is opened by its owner alone, and neither writes the other's.

A tool call that changes a record asks the server, which commits the
change, its charge and a marker of its result in one transaction. A call
that runs again, after a crash on either side or a lost reply, finds the
marker and gets the result it holds, so a change is made once however
often its call runs. The same marker asked for with different arguments
is refused. Markers are kept for good.

What tools read, write and run stays as files: each house's `NOTES.md` and
`skills/`, the library's skill folders, tools added by hand, and the folder
each hamlet works on.

## Considered options

- One store per hamlet. Rejected: moving an agent, a road between hamlets
  and a vote that founds one each span hamlets, and in one store each is
  one commit.
- One pi-durable storage for the records and the conversations together.
  Rejected: the records would be shaped by one harness's storage, and a
  second harness could not be added.
- The records kept as JSONL files. Rejected: a record and the marker of
  the call that made it could not commit together, so every record would
  need repair code after a crash.
- A record committed in the same transaction as the harness's tool result.
  Not possible: they are in different stores, and pi-durable 1.2.0 takes
  nothing of a tool's into the transaction of its result.
- Notes and skills in a store too, copied out for tools. Rejected: every
  file a tool touches would have to be kept in step both ways.

## Consequences

- A home cannot be read with a text editor; inspecting it needs the
  server or SQLite.
- A home is claimed by one server at a time, and the harnesses it runs open
  their stores only while it holds the claim.
- A store that is missing, or belongs to another home or runtime, is an
  error, never replaced by an empty one.
- Every tool that changes a record is safe to run twice, and goes through
  the one path that commits the change with its marker.
