---
name: web-search
description: Search the web and get the top results (title, URL, snippet) as plain text.
usage: python3 skills/web-search/scripts/search.py "your query" [max_results]
---

Needs tool network access (the user turns it on in the Connection settings) and python3.

```sh
python3 skills/web-search/scripts/search.py "your query" [max_results]
```

Prints up to `max_results` results (default 8), each as title, URL and snippet.
To read one of the pages, use the `read-webpage` skill with its URL.

It asks DuckDuckGo, then Bing, then Wikipedia, and says which one answered when
it was not the first. Search engines stop answering after many searches in a
row, so put everything you need into a few good queries rather than many small
ones.

If it prints an error, report that error to the user as it is. Do not answer
from memory and present it as a search result.
