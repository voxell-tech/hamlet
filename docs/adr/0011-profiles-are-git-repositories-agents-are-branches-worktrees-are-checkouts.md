---
status: accepted
---

# Profiles are git repositories, agents are branches, worktrees are checkouts

What a profile knows is a git repository, `profiles/<id>.git`, bare and
written only by the server through gix, never by a git program and never
reading the machine's git configuration. It holds `CHARACTER.md`, `memory/`
(files at its root are in every prompt, a folder is described by its
`MEMORY.md`) and `skills/<name>/SKILL.md`, with their whole history. `main`
is what the profile knows. A version is a tag. The library is a repository
too (`library.git`), and the Hand has a profile of its own.

An agent of a hamlet is a profile working in it, and it gets a branch,
`agents/<agent id>`, starting at `main` or at its pin. Its worktree,
`hamlets/<hamlet id>/<agent id>/` (the Hand's is `hand/`), is a checkout of
that branch without `CHARACTER.md`, plus an `inbox/` that is never
committed. Its tools write there, and after a turn that changed it the
server commits it to the branch. What an agent learns stays on its branch
until a dream brings it home to `main`; when `main` moves, an idle unpinned
agent has it merged into its branch, never while a turn of it runs.

Structure (who exists, who answers to whom, channels, conversations, waits,
spend) is in the database (ADR 0012). Git keeps knowledge.

## Considered options

- A folder of files per agent, beside a character in the database.
  Rejected: no history, so nothing could be tagged, reverted, forked or
  purged; merging two agents needed a model to rewrite their notes; and a
  profile working as two agents had two unrelated houses.
- The profile's knowledge as rows in the database. Rejected: tools read and
  write files, so every file would be kept in step both ways, and diffs
  would have to be built.
- A git program on the server's machine. Rejected: it depends on the
  machine's git, configuration, hooks and credentials, which are the
  chief's business in the chief's folder (ADR 0007).
- Agents writing straight into `main`. Rejected: one agent's turn would
  change what the profile's other agents know in the middle of theirs.
- One repository per hamlet, a branch per agent. Rejected: a profile's
  knowledge crosses hamlets, and erasing a hamlet would reach into it.
- A clone per agent. Rejected: a branch separates as well without copying
  the history, and a merge or a dream sees both sides in one repository.

## Consequences

- Every change to what a profile knows is a commit with its author: a
  chief, an agent, the Hand, a dream or the realm. A chief can tag, pin,
  revert, fork and purge, and read the difference between a branch and
  `main`.
- A worktree has no character file, so C13 holds by construction: an agent
  cannot write its own character, and a file by that name is never
  committed.
- Merging two agents, splitting them and taking in `main` are git merges
  and reverts; the clashing path is kept on the side that was there and
  named. Purging a path rewrites every commit of every branch and replaces
  the repository.
- A repository changes only in a job kept with the change that needs it and
  run once more after a crash (ADR 0012). Jobs run one at a time on a
  thread of their own, so two changes to a profile never interleave.
- A realm is its database and its repositories together, and a backup must
  hold both.
- Erasing a profile, an agent or a hamlet deletes repositories, branches
  and worktrees, and cannot be undone.
