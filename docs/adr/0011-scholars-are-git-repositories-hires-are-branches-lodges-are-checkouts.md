---
status: accepted
---

# Scholars are git repositories, hires are branches, lodges are checkouts

What a scholar knows is a git repository, `scholars/<id>.git`, bare and
written only by the server through gix, never by a git program and never
reading the machine's git configuration. It holds `CHARACTER.md`, `memory/`
(files at its root are in every prompt, a folder is described by its
`MEMORY.md`) and `skills/<name>/SKILL.md`, with their whole history. `main`
is what the scholar knows. A version is a tag. The library is a repository
too (`library.git`), and the Hand has a scholar of its own.

A hire is a scholar working in a hamlet, and it gets a branch of its
scholar's repository, `hires/<agent id>`, starting at `main` or at its pin.
Its lodge, `hamlets/<hamlet>/<number>/` (the Hand's is `hand/`), is a
checkout of that branch, without `CHARACTER.md`, plus an `inbox/` that is
never committed. Its tools write there. After a turn that changed the lodge
the server commits it to the branch. What a hire learns stays on its branch
until a dream brings it home to `main`; when `main` moves, an idle unpinned
hire has it merged into its branch, never while a turn of it runs.

Who exists, who answers to whom, roads, conversations, waits and spend are
not in git. They are in the database (ADR 0012). Git keeps knowledge, the
database keeps structure.

## Considered options

- A folder of files per agent, as before: notes, skills and an inbox in a
  house, beside a character in the database. Rejected: it has no history, so
  nothing could be tagged, reverted, forked or purged; a merge of two agents
  needed a model to rewrite their notes; and a scholar hired twice would have
  had two unrelated houses.
- The scholar's knowledge as rows or documents in the database. Rejected:
  tools read and write files, so every file would be kept in step both ways,
  and a diff between two versions would have to be built.
- A git program on the server's machine. Rejected: it depends on the machine's
  git, its configuration, its hooks and its credentials, which is the chief's
  business in the chief's folder (ADR 0007), not the server's own records.
- Hires writing straight into `main`. Rejected: one hire's turn would change
  what the scholar's other hires know in the middle of theirs, and nothing
  would stand between a hire's mistake and the scholar.
- One repository per hamlet, a branch per hire. Rejected: a scholar's
  knowledge crosses hamlets, and erasing a hamlet would reach into it.
- A clone of the scholar's repository per hire. Rejected: a branch gives the
  same separation without copying the history, and a merge or a dream sees
  both sides in one repository.

## Consequences

- Every change to what a scholar knows is a commit with its author: a chief,
  a hire, the Hand, a dream or the realm. A chief can tag, pin, revert, fork
  and purge, and read the difference between a hire's branch and `main`.
- A lodge has no character file, so C13 holds by construction: a hire cannot
  write its own character, and a file by that name is never committed.
- Merging two hires, splitting them and taking in `main` are git merges and
  reverts; the clashing path is kept on the side that was there and named.
  Purging a path rewrites every commit of every branch and replaces the
  repository.
- The repositories stay on the server. A repository changes only in a job
  that is kept with the change that needs it and run once more after a crash
  (ADR 0012). Jobs run one at a time on a thread of their own, so two changes
  to a scholar never interleave.
- A realm is its database and its repositories together, and a backup must
  hold both.
- Erasing a scholar, a hire or a hamlet deletes repositories, branches and
  lodges, and cannot be undone.
