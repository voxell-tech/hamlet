#!/usr/bin/env python3
"""Fetch a URL and print its readable text."""
import html
import re
import sys
import urllib.error
import urllib.request
from html.parser import HTMLParser

UA = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36"
SKIP = {"script", "style", "noscript", "svg", "head", "nav", "footer", "form", "iframe"}
BLOCK = {"p", "div", "br", "li", "tr", "h1", "h2", "h3", "h4", "h5", "h6", "section", "article", "pre", "blockquote"}


class Text(HTMLParser):
    def __init__(self):
        super().__init__()
        self.parts, self.skipping, self.title, self.in_title = [], 0, "", False

    def handle_starttag(self, tag, attrs):
        if tag in SKIP:
            self.skipping += 1
        if tag == "title":
            self.in_title = True
        if tag in BLOCK:
            self.parts.append("\n")

    def handle_endtag(self, tag):
        if tag in SKIP and self.skipping:
            self.skipping -= 1
        if tag == "title":
            self.in_title = False
        if tag in BLOCK:
            self.parts.append("\n")

    def handle_data(self, data):
        if self.in_title:
            self.title += data
        elif not self.skipping:
            self.parts.append(data)


def main():
    if len(sys.argv) < 2:
        sys.exit("usage: read.py URL [max_chars]")
    url = sys.argv[1]
    limit = int(sys.argv[2]) if len(sys.argv) > 2 else 8000
    try:
        req = urllib.request.Request(url, headers={"User-Agent": UA})
        with urllib.request.urlopen(req, timeout=20) as resp:
            kind = resp.headers.get_content_type()
            charset = resp.headers.get_content_charset() or "utf-8"
            body = resp.read(4_000_000).decode(charset, "replace")
    except urllib.error.HTTPError as e:
        sys.exit(f"error: the server answered HTTP {e.code} for {url}")
    except urllib.error.URLError as e:
        sys.exit(f"error: could not reach {url} ({e.reason}). Tool network access may be off.")
    except Exception as e:
        sys.exit(f"error: {e}")

    if "html" in kind:
        parser = Text()
        parser.feed(body)
        text = html.unescape("".join(parser.parts))
        text = re.sub(r"[ \t\r\f\v]+", " ", text)
        text = re.sub(r"\n\s*\n+", "\n\n", text).strip()
        title = parser.title.strip()
    else:
        text, title = body.strip(), ""
    if title:
        print(f"# {title}\n")
    print(text[:limit])
    if len(text) > limit:
        print(f"\n[cut at {limit} of {len(text)} characters]")


if __name__ == "__main__":
    main()
