---
name: system-info
description: Report the machine's OS, CPU, memory, disk space, uptime and the busiest processes.
usage: python3 skills/system-info/scripts/info.py
---

Needs python3. Works without network access.

```sh
python3 skills/system-info/scripts/info.py
```

Some values can be missing inside the sandbox (for example the process list);
the script says which ones it could not read instead of guessing.
