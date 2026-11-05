---
name: manage-memory
description: How to keep memory/ short, current and useful as long-term memory.
---

The files at the root of `memory/` are shown to you before every call, so
every line costs attention. Keep them under about 60 lines together. A
folder under `memory/` shows only the description in its `MEMORY.md`; read
the folder's files when that description says they help.

## What belongs

- Facts about each chief: name, location, units, language, standing preferences.
- Decisions and lessons that change what you do next time.
- Pointers: which skill handles what, where a project lives.

## What does not

- Anything already in a skill's SKILL.md.
- A log of what happened in a session.
- Guesses. If a chief did not say it and you did not verify it, leave it out.
- Secrets, tokens or passwords.
- A temporary state written as a fact ("network is unavailable"). Settings
  change; describe how to check instead.

## Layout

Every file opens with a front matter `description:` line.

```markdown
---
description: The chiefs I work for and what they prefer.
---

- Alice: Toronto, metric units, prefers short answers.
```

Put a topic that grows into its own folder, such as
`memory/projects/MEMORY.md` with a description and one file per project.

## Upkeep

Edit lines in place with the `edit` tool; do not append duplicates. When a note
turns out wrong, correct or delete it. With several chiefs, record whose a
preference is. What you keep in your lodge goes home to your scholar, so keep
what any hire of it would want to know.
