# The chat

An agent's watched conversation, its rows, and the links and pictures
they hold. The drawing is `hamlet_app/src/screens/chat/` and `markdown/`,
on the Markdown of `hamlet_ui/src/markdown/`. The client transcript is
`hamlet_client/src/chats.rs`. [The client protocol](protocol.md) has the
wire messages, watch rules and page boundaries; this page covers the
window.

## The conversation and its watch

A chat entity has `Chat(ChatRef)`, `Transcript`, `Status`, `Usage` and a
`TalksTo(agent)` relationship. Adding the chat sends `open`, removing it
sends `close`, and a reconnect opens every watched chat again. A desk can
hold several Hubs, each with a chat and a field of its own.

The server answers with a `transcript` page, then live `turn`
operations. The client only applies them. Each item has a stable string
ID, a time and a kind: chief input, assistant text, reasoning, tool,
notice, error, answered, summary or merged.

## One conversation

The Hand and each agent of a hamlet have one conversation, shown the same
by every client. The chief's lines, other agents' messages and its
workers' answers all go into it, one turn at a time. A worker has its
own.

A message from another agent carries `author`, `from` and `message`
(notification, request, response or error response) in place of
`chief`. The model reads it under a bracketed heading, as
`[A request from agent#3 (Mira); your final answer goes back to it as the response]`.
An error response starts no turn.

**Queued lines.** An input waiting its turn is one of the last items,
with `queued`, in run order: the chief's lines and retries first, then
agents' messages. When taken up it is replaced in place (`add` with
`replaces`). The chief takes one back with `withdraw`.

**Stop holds the queue.** Stop ends the running turn and holds the queue
until the chief sends a line, which runs first, or resumes it (`resume`,
`/resume` in the terminal). `status` carries `held`. Workers go on, and
their answers queue under the hold (`conversations/chats.rs`).

**Compaction and merges.** pi summarizes the earlier conversation when
the context fills (C28), or on `/compact`; the summary is a `summary`
item. A merge gives the kept agent's conversation a `merged` item whose
`from` is the absorbed agent. That conversation stays as it was, so a
split gives it back.

A change to the agent (model or effort, stop or resume, a dream, a
rename, the fast tier) is an `event` item for people alone
(`chats::event`).

## Brought in from another program

An agent made for a conversation brought in from another program (C35)
has it as its conversation. The client sends each conversation as a file
and the project as `import-project`; the server
(`conversations/imports.rs`) makes the agents and holds each
conversation's entries until a model is set up. The chief's messages in
it are not rewindable.

## Order and drawing

Rows follow the transcript's order, even if a clock ran back.

**A turn.** Everything the agent does between one input and the next is
one turn (`lines.rs`, `Turn`): its thinking, tool calls and the words
between them are steps, and its last words are the reply. The view stays
at the end or at the first row shown (`anchor.rs`), and moves only when
the chief scrolls, sends a line or asks for the end.

When a transcript has an earlier page, scrolling to the top, or a chat
that fits without scrolling, sends `open` with `before` set to the first
item's ID. Only one page request is outstanding, and the returned items
go above the rows already there without moving them on screen.

Sending a line triggers `ChiefMessage`, which the client sends as `say`
to the address of the Hub's chat. Typed slash lines run only as the
commands the field lists; any other is refused locally.

**What the field lists.** While the word at the caret starts with a sign
of `hamlet_protocol::signs`, a list over the field offers what it may
name:

| Sign | Lists | Picked |
|---|---|---|
| `@` | the other living agents the chat's agent has a channel to, the Hand first | its chip, sent as `agent#3 (Mira)` |
| `/` | `/compact`, `/retry`, `/stop`, `/reflect` and `/model`, at the start of the line | runs as its control does |
| `$` | the library's skills and the agent's own | a chip, sent as `$name` |
| `#` | the files and folders of the hamlet's folder | attached where the word stood (C33) |

`$5` is money and `#3` a wait, so neither opens the list. Each kind is a
`Source` in `crates/hamlet_app/src/screens/chat/suggestions/`, added with
`add_source`.

Faults with a `setup` cause (no model, refused credentials) share one
toast per cause and model (`toasts.rs`); a fault's words come from
`conversations/failures.rs`.

## Links

- `[words](address)` is a link when the address is `http`, `https` or
  `mailto`, or a path whose file is there. Any other scheme is plain
  words.
- A bare `https://..` in running text is always a link.
- A path in running text or inline code is a link when its file is there.
- Nothing in a fenced code block is a link.

**Which files.** Files resolve only on the window's machine. An absolute
path must exist there. A relative path is looked for under the received
work folder of the hamlet of the agent the Hub's chat is with, and
becomes a link only if its canonical path stays under that folder; `..`
and symlinks are followed before that check. The received realm folder is
excluded after resolving links, for absolute and relative paths alike. No
file under an agent's worktree becomes a link or a local picture. Nothing
downloads a server file.

The disk is asked once for each distinct path when a paragraph is built,
never while reading, and the answer is kept until a turn ends or the
work folders or the received realm folder change.

**A click.** A web address goes to the system's browser. A file is
checked once more and then shown by the system's own program, given the
path as one argument with no shell between. A file the system would run
and not show (an executable, a script, an app) is revealed in its
folder, unopened.

## Agents named in the chat

An agent's or a profile's address in a line is drawn as a chip: the glyph
of what it is, its current name, and the address in its tooltip.

- `agent#<id>`, `worker#<id>`, `profile#<id>` and `lead@<hamlet>` are
  chips in running text, inline code and link addresses, as
  `[Mira](agent#3)`.
- `hand` is one only in inline code or as a link's address.
- A copied address's name, `agent#3 (Mira)`, is part of the chip.
- An address that names no one stays words, as does anything in a fenced
  code block.

While an answer grows, its addresses are words, and become chips once it
is whole (`unsettle` in `hamlet_ui/src/markdown/draw.rs`), because a chip
in a reflowing line would jump.

In the field, an address becomes a chip once written whole
(`screens/chat/field.rs`). A bracketed name must be the current one, and
`hand` needs its name. The chip is sent as its words.

## Edits

An `edit` or `write` call carries its change: the path, the count of
lines added and removed, and the lines themselves.
`crates/hamlet_server/src/tools/edit_diff.rs` keeps at most 200 lines,
each cut at 240 characters, and sends them in the tool's `change` field,
live and when the transcript is read back. A change made by a shell
command is not shown.

## Replying and quoting

A reply stays with the chat it was begun in. It is part of the message's
own words: a first line that names the message it answers, its words as a
JSON string, and the reply after a blank line:

```
(Replying to Lead: "Paper or ebook?\nBoth ship soon.")

Paper.
```

So the log, a rewind and a replay need nothing new. At most
`reply_chars` characters of the message are carried (`[chat]` in
`assets/ui.toml`). A rewind to a reply puts what it answered back over
the field.

A line that opens with `> ` is a quote; the model reads it as a Markdown
quote.

The chief's lines are Markdown, drawn as an agent's words are, with each
line break kept (`parse_typed`). The field draws them so while they are
typed: `hamlet_ui/src/markdown/marks.rs` reads the field's text into
fynix marks, which set each part's look and hide the signs, such as `**`
and a quote's `> `, once the part they open is closed. Signs show again
while the caret is in their part, or at a line's `#` or `>`, so they
are edited one character at a time like any other text. A line with an
attachment placed in its words is drawn with its links and quotes only.

## Selecting words

Text of the panels is selected where it is drawn
(`hamlet_app/src/select.rs`). Bevy does not select text, so it uses the
parley layout each text node keeps (`ComputedTextBlock::buffer`), broken
again at the width the node is drawn at, since Bevy breaks it at other
widths while it measures.

## Rewind

A conversation can be taken back to before one of the chief's messages.
That message and everything after it leave the active conversation, and
its words go back in the field.

The server forks the conversation from the entry before that message and
makes the fork the agent's conversation, with no session grants yet. The
client sends `rewind` with the item's string ID, and a `rewound` reply
gives the words back. The server rejects a rewind while work is running
or queued and closes waits raised by the part taken back.

Not undone: files changed, commands run, memory, skills, files attached
and everything spent. Only chief messages can be rewound, and only while
no turn runs.

## Pictures

A picture an agent names in a reply, `![words](place)`, is drawn where
the reply put it. The window sends no bytes of these back to the server
or a model.

- A local picture file a link may lead to (`png`, `jpg`, `jpeg`, `webp`,
  `gif`, `bmp`) is drawn; a GIF as its first frame.
- A file too large or of another kind, no local file, or an unresolved
  relative path gives a one-line chip saying why it is not shown.
- An `http` or `https` address gives a chip, and the picture only after
  the chief asks.

**A picture of the web is never fetched by itself.** A model wrote the
address, and a fetch tells that server whatever the address carries. An
agent fed bad instructions could put a secret in the address and have the
chief's machine send it just by showing the chat. So only the chief's
press of Show Picture fetches. The fetch is bounded in time, size and
redirects, takes only an answer that is an image, and keeps the picture
in memory only.

No picture file is read in a frame. A background task reads it, and a
picture too large to decode safely is refused. One image is kept per
file as last written, shared by every row that names it.

## The field and attachments

The field is fynix's `rich_field`. Each draft, words and chips, is kept
per chat for the window's life (`Unsent` in `screens/chat/field.rs`). An
attachment is a chip in the field, an `Atom` that Backspace removes along
with the attachment; `attach.rs` keeps them in `Attached`, and an upload
goes on while another chat is shown.

On send, the field's plain text (no chips) is the line's words, trimmed,
and the byte each chip stood at in it is its attachment's `at`. A reply
puts its line before the words, so every `at` moves by that line's
length. A line that cannot be sent yet, because an attachment is still
going up, waits for a word or was refused, leaves the field as it is.

A chief attaches PNG, JPEG, WebP and GIF images (C33) and any file or
folder. Each is uploaded at once and sent with the line, which may hold
only attachments; a line waits for every upload to be kept. An image
over 20 MiB is refused unread. A rewind puts them back in the field.

A folder goes with everything under it, links left out, as one chip,
uploaded one file at a time (`put-file`). The model is not given a file;
the line names where each is saved in the agent's `inbox/`, so any model
takes files. What is attached other than images is held at 32 MiB or 1000
files until the chief presses "Send anyway".

**Where an attachment sits.** A line's images and files may each carry a
place in its words, a byte offset on a character boundary (`at` in
[the protocol](protocol.md)). The server puts each one's caption in the
words at its place, for the model, and the transcript keeps the words
alone with each place beside them. The terminal chat places the ones its
`#path` tokens name.

**Models without images.** For a model that takes no images a paste or
drop is refused, and the server rejects a line that still holds one
(S30). The agent's catalogue entry tells which; a model the catalogue
does not list is taken to see. A line takes as many images as the model's
limit. A chief says that a model of a custom endpoint takes images, and
within what limits, in the model catalogue in Settings or with
`hamlet models images`.

A sent line shows its images as tiles of the small copy the server makes
(S29), fetched by id.

## Questions to the chief

An agent puts questions to the chief with `request_chief`, and its turn
waits for the answer. It asks from any turn of its conversation; a worker
or an errand asks only in the turn of a chief's own line. One call holds
every question the agent has.

A call has a `message` and 1 to 10 `questions`, each with a `question`,
an optional `description`, at most 6 `options` (`title` and optional
`description`) and `text_input`, true unless the agent says otherwise; a
question with no options always takes words.
`crates/hamlet_server/src/conversations/questions.rs` checks a call, and
a call it refuses tells the agent why.

One card, `hamlet_app/src/asking.rs`, is shown on the realm's page and
over the field of the chat with the agent that asked, and both read the
same answers. Each question takes one option or words, not both. Dismiss
tells the agent to go on with its best judgement.

The agent is given the answer as words (`The chief answers:`, then each
question quoted over the option picked or the words typed). A line typed
to the agent in the chat or the terminal still answers the whole card at
once. In the chat the answer is `Entry::Answered`, made from the
`request_chief` result; it is a tool's answer, not a message of the log,
so it cannot be rewound to. The `asking.questions` limit counts the calls
of one turn, not the questions in them.

## What it costs

The chat is fynix's `virtual_list`, put in order again only when a
conversation's shape changes (an entry added, a tool ended, an answer
beginning or ending, a selection or a fold); a word added to a reply
changes no shape. `fold_lines` compares rows by key and tells the list
which span changed, so rows added above the view do not move what is
being read. A growing reply is read again from its last settled block
only.

Bevy marks every text input changed each frame, which would lay out
everything above it again. `hamlet_ui/src/fields.rs` runs that size
system only when a font, a scale, a new field or a field's lines changed.
