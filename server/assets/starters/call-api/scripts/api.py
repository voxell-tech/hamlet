#!/usr/bin/env python3
"""Call an HTTP API and print the status and a readable body."""
import json
import sys
import urllib.error
import urllib.request

LIMIT = 6000


def main():
    args = sys.argv[1:]
    headers, pick, rest = {"User-Agent": "hamlet/0.1", "Accept": "application/json"}, None, []
    while args:
        a = args.pop(0)
        if a == "--header" and args:
            name, _, value = args.pop(0).partition(":")
            headers[name.strip()] = value.strip()
        elif a == "--pick" and args:
            pick = args.pop(0)
        else:
            rest.append(a)
    if len(rest) < 2:
        sys.exit('usage: api.py METHOD URL [JSON_BODY] [--header "Name: value"] [--pick path]')
    method, url = rest[0].upper(), rest[1]
    body = rest[2].encode() if len(rest) > 2 else None
    if body is not None:
        headers.setdefault("Content-Type", "application/json")

    req = urllib.request.Request(url, data=body, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            status, raw = resp.status, resp.read(4_000_000)
    except urllib.error.HTTPError as e:
        status, raw = e.code, e.read(4_000_000)
    except urllib.error.URLError as e:
        sys.exit(f"error: could not reach {url} ({e.reason}). Tool network access may be off.")
    except Exception as e:
        sys.exit(f"error: {e}")

    print(f"HTTP {status}")
    text = raw.decode("utf-8", "replace")
    try:
        data = json.loads(text)
    except ValueError:
        print(text[:LIMIT])
        return
    if pick:
        for key in pick.split("."):
            try:
                data = data[int(key)] if isinstance(data, list) else data[key]
            except (KeyError, IndexError, ValueError, TypeError):
                sys.exit(f"error: nothing at {pick!r} (stopped at {key!r})")
    out = json.dumps(data, indent=2, ensure_ascii=False) if isinstance(data, (dict, list)) else str(data)
    print(out[:LIMIT])
    if len(out) > LIMIT:
        print(f"\n[cut at {LIMIT} of {len(out)} characters; use --pick to narrow it]")


if __name__ == "__main__":
    main()
