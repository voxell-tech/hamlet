---
name: call-api
description: Call a JSON HTTP API (GET, POST, PUT, DELETE) and get a readable, optionally filtered response.
usage: python3 skills/call-api/scripts/api.py GET|POST|PUT|DELETE "https://api.example.com/items" [json-body]
---

Needs tool network access and python3.

```sh
python3 skills/call-api/scripts/api.py GET "https://api.example.com/items?limit=3"
python3 skills/call-api/scripts/api.py POST "https://api.example.com/items" '{"name": "x"}'
python3 skills/call-api/scripts/api.py GET "https://api.example.com/user" --header "Authorization: Bearer $TOKEN"
python3 skills/call-api/scripts/api.py GET "https://api.example.com/items" --pick "items.0.name"
```

- `--header "Name: value"` can be repeated.
- `--pick a.b.0.c` prints only that part of the response (dots walk into objects and list indexes).
- Prints the HTTP status first, then the body (pretty-printed when it is JSON, cut at 6000 characters).

Never put a secret in a skill file or in NOTES.md. If an API needs a key, ask the
user how they want to provide it.
