---
name: manage-notes
description: How to keep NOTES.md short, current and useful as long-term memory.
---

NOTES.md is shown to you at the start of every turn, so every line costs
attention. Keep it under about 60 lines.

## What belongs

- Facts about each user: name, location, units, language, standing preferences.
- Decisions and lessons that change what you do next time.
- Pointers: which skill handles what, where a project lives.

## What does not

- Anything already in a skill's SKILL.md.
- A log of what happened in a session.
- Guesses. If the user did not say it and you did not verify it, leave it out.
- Secrets, tokens or passwords.
- A temporary state written as a fact ("network is unavailable"). Settings
  change; describe how to check instead.

## Layout

```markdown
# Notes

## Users
- Alice: Toronto, metric units, prefers short answers.

## Lessons
- Check `skills/` before writing a new script.

## Projects
- hamlet: /path/to/project, Rust.
```

## Upkeep

Edit lines in place with the `edit` tool; do not append duplicates. When a note
turns out wrong, correct or delete it. With several users, record who a
preference belongs to.
