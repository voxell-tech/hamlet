---
status: partly superseded by ADR 0009
---

# The home is one pi-durable storage, beside the files tools use

A home keeps its conversations and Hamlet's records in one pi-durable
storage: a single SQLite file. The records (the hamlet's record, the realm,
oversight, spend, the library's record) are pi-durable documents, and so
are the chief's documents (limits, charter, characters, allowed models,
settings), which the window edits through the server.

[ADR 0009](0009-the-server-is-rust-and-harnesses-run-the-conversations.md)
supersedes the one storage: the server keeps Hamlet's records in a store of
its own, and each harness keeps its conversations in its own. A change and
the marker of its result still commit together, now in the server's store,
and the marker is kept for good. The files tools use still stay files.

A tool that changes a record commits the change in one transaction with a
marker of its result, kept for that call only. pi-durable commits a tool's
result in a transaction of its own, which a tool cannot add to, so a call
can be cut off between the two. Such a call runs again, finds the marker
and gives the result it holds, and the marker goes when the call ends. A
change is made once however often its call runs. A stop that lands between
the two leaves the change, and a result that says the call was stopped.

What tools read, write and run stays as files: each house's `NOTES.md` and
`skills/`, the library's skill folders, tools added by hand, and the folder
each hamlet works on.

## Considered options

- One storage per hamlet. Rejected: moving an agent, a road between
  hamlets and a vote that founds one each span hamlets, and in one storage
  each is one commit.
- JSONL storage. Rejected: pi-durable keeps a JSONL history in memory and
  advises SQLite for long-running agents.
- Hamlet's records kept as our own JSONL files beside pi-durable's storage.
  Rejected: a record and the marker of the call that made it could not
  commit together, so every record would need repair code after a crash.
- A record committed in the same transaction as the tool result that made
  it. Not possible: pi-durable 1.2.0 takes nothing of the tool's into the
  transaction of its result.
- Notes and skills in the storage too, copied out for tools. Rejected:
  every file a tool touches would have to be kept in step both ways.

## Consequences

- A home can no longer be read with a text editor; inspecting it needs the
  server or SQLite.
- Only one process may open a home at a time.
- Every tool that changes a record is safe to run twice, and is built with
  the one helper that commits the change with its marker.
