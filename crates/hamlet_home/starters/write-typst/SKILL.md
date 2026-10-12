---
name: write-typst
description: Write a document in Typst and compile it to a PDF. Read this before writing any .typ file, since Typst is not Markdown.
usage: typst compile report.typ
---

Needs the `typst` program. Write the `.typ` file in the workspace, then compile:

```sh
typst compile report.typ        # writes report.pdf beside it
```

Fix every `error:` it prints and compile again. Tell the user where the PDF is.
Do not try to open it.

## The markup that works

```typst
#set page(paper: "a4", margin: 2cm)
#set text(size: 11pt)

= A title
== A section

Plain text. *Bold* uses one star each side, _italic_ one underscore.

- a bullet
+ a numbered item

#table(
  columns: 3,
  [*Day*], [*Sky*], [*High*],
  [Mon], [Clear], [17 C],
  [Tue], [Rain], [12 C],
)

#image("picture.jpg", width: 80%)
```

## What goes wrong

- `**bold**` and `== Heading ==` are Markdown. Typst wants `*bold*` and `== Heading`.
- `$...$` is maths only. Never put a title or a date in it.
- A table cell is `[text]`. There is no `row[...]`, `table.cell(bold(...))` or
  `cell-text`. Bold in a cell is `[*text*]`.
- `#set page(...)` takes `paper`, `width`, `height`, `margin`. It has no `title`.
- A literal `#`, `*`, `_`, `$` or `@` in text needs a backslash: `\#`, `\$`.
- `#image` reads a file in the workspace, never a URL. Download it first with
  `curl -sL "<url>" -o picture.jpg`, and check it is an image, not an error page.
- The date is `#datetime.today().display()`.
