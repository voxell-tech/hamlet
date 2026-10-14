#!/usr/bin/env python3
"""Search the web and print the top results as plain text.

DuckDuckGo is asked first. When it answers with a captcha or nothing, Bing is
asked, and then Wikipedia. DuckDuckGo stops answering scripts after a few
searches in a row, so the others matter.
"""
import html
import json
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

UA = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36"


def text(fragment):
    return html.unescape(re.sub(r"<[^>]+>", "", fragment)).strip()


def fetch(url, data=None):
    headers = {"User-Agent": UA, "Accept-Language": "en-US,en;q=0.9"}
    req = urllib.request.Request(url, data=data, headers=headers)
    with urllib.request.urlopen(req, timeout=15) as resp:
        return resp.read().decode("utf-8", "replace")


def duckduckgo(query, limit):
    def target(href):
        """DuckDuckGo wraps links as //duckduckgo.com/l/?uddg=<target>."""
        href = html.unescape(href)
        if href.startswith("//"):
            href = "https:" + href
        found = urllib.parse.parse_qs(urllib.parse.urlparse(href).query).get("uddg")
        return found[0] if found else href

    page = fetch("https://html.duckduckgo.com/html/?" + urllib.parse.urlencode({"q": query}))
    links = re.findall(r'<a[^>]*class="result__a"[^>]*href="([^"]+)"[^>]*>(.*?)</a>', page, re.S)
    snippets = re.findall(r'<a[^>]*class="result__snippet"[^>]*>(.*?)</a>', page, re.S)
    return [
        (text(title), target(href), text(snippets[i]) if i < len(snippets) else "")
        for i, (href, title) in enumerate(links[:limit])
    ]


def bing(query, limit):
    """Bing's RSS answer. Without a market it answers in whatever it guesses."""
    params = {"q": query, "format": "rss", "mkt": "en-US", "setlang": "en", "cc": "US"}
    page = fetch("https://www.bing.com/search?" + urllib.parse.urlencode(params))
    results = []
    for item in re.findall(r"<item>(.*?)</item>", page, re.S):
        title = re.search(r"<title>(.*?)</title>", item, re.S)
        link = re.search(r"<link>(.*?)</link>", item, re.S)
        about = re.search(r"<description>(.*?)</description>", item, re.S)
        if title and link:
            results.append((text(title.group(1)), html.unescape(link.group(1)).strip(), text(html.unescape(about.group(1))) if about else ""))
    return results[:limit]


def wikipedia(query, limit):
    url = "https://en.wikipedia.org/w/api.php?" + urllib.parse.urlencode(
        {"action": "query", "list": "search", "format": "json", "srlimit": limit, "srsearch": query}
    )
    found = json.loads(fetch(url)).get("query", {}).get("search", [])
    return [
        (hit["title"], "https://en.wikipedia.org/wiki/" + urllib.parse.quote(hit["title"].replace(" ", "_")), text(hit.get("snippet", "")))
        for hit in found
    ]


ENGINES = [("DuckDuckGo", duckduckgo), ("Bing", bing), ("Wikipedia only", wikipedia)]


def main():
    if len(sys.argv) < 2:
        sys.exit('usage: search.py "query" [max_results]')
    query = sys.argv[1]
    limit = int(sys.argv[2]) if len(sys.argv) > 2 else 8
    failures = []
    for name, engine in ENGINES:
        try:
            results = engine(query, limit)
        except urllib.error.URLError as e:
            failures.append(f"{name}: could not be reached ({getattr(e, 'reason', e)})")
            continue
        except Exception as e:
            failures.append(f"{name}: {e}")
            continue
        if not results:
            failures.append(f"{name}: no results, or it asked for a captcha")
            time.sleep(1)
            continue
        if name != ENGINES[0][0]:
            print(f"[results from {name}]")
        for n, (title, url, snippet) in enumerate(results, 1):
            print(f"{n}. {title}\n   {url}")
            if snippet:
                print(f"   {snippet}")
        return
    sys.exit(
        "error: no search engine gave results. " + "; ".join(failures) + ". If none could be reached, tool network access may be off."
    )


if __name__ == "__main__":
    main()
