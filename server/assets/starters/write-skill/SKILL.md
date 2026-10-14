---
name: write-skill
description: How to write, test and improve a skill so it works the next time it is needed.
---

A skill is a folder `skills/<name>/` with a `SKILL.md` and, usually, a script in
`scripts/`. Only the `description` line is shown to you each turn, so it decides
whether the skill gets used.

## Steps

1. **Check first.** If an existing skill almost does the job, improve it instead
   of adding another.
2. **Name** it in lowercase with hyphens, after what it does: `convert-units`.
3. **Description**: one line saying what it does and when to use it, in words
   the user would use. Not "helper for X".
4. **Script**: put the working commands in `scripts/`. Take inputs as arguments.
   Prefer python3 with only the standard library, or plain shell.
5. **Front matter**: add `usage: <the command that runs it>`, from the workspace
   root, with example arguments. It is shown beside the description every turn.
6. **Body of SKILL.md**: the exact command to run with example arguments, what it
   prints, and what it needs (network access, an API key, a program).
7. **Test it** by running the exact command from SKILL.md, from the workspace
   root. Do not save a skill you have not run successfully.
8. **Paths**: always workspace-relative (`skills/<name>/scripts/<file>`).

## Rules that keep skills trustworthy

- **Report real errors.** Print what actually went wrong and exit non-zero. Never
  catch every exception and print a guess such as "network unavailable".
- **No made-up output.** If a script cannot get the data, it says so. It never
  falls back to invented or remembered values.
- **No secrets** in skill files.
- **Keep output short**: the few lines that answer the question, cut long output.
- When a skill fails in use, fix the skill before moving on, then note what
  changed in its SKILL.md.
