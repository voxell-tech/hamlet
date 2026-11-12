# The chat

An agent's watched conversation, its rows and the links and pictures they
hold. The drawing is `hamlet_app/src/screens/chat/` and `markdown/`, on
the Markdown of `hamlet_ui/src/markdown/`. The client
transcript is `hamlet_client/src/chats.rs`; the server keeps the
conversations through pi-durable. [The client protocol](protocol.md) describes the
wire messages, watch rules and page boundaries; this page covers the window.

## The conversation and its watch

A chat entity has `Chat(ChatRef)`, `Transcript`, `Status`, `Usage` and a
`TalksTo(agent)` relationship. Its reference names the agent by its id
(the Hand, a hire or a worker). Adding the chat sends `open`; removing it
sends `close`. Reconnecting opens every watched chat again.

The server answers with a `transcript` page. Live `turn` operations add or
replace an item, append text, close a tool, change status or usage, or reset
the transcript. The client applies those operations; it runs no agent and
reads no conversation files. Each item has a stable string ID, a time and
a kind: chief input, assistant text, reasoning, tool, notice, error or
answered.

The chief's chat with an agent is shared by every client. `/new` moves to a
new numbered conversation while the old one finishes its queue. History
comes from `read history`, listed by the day each was last used; Read
opens a numbered past conversation read-only, drawn as the chat is, and
Resume sends `/resume <n>`. Every conversation is kept.

A conversation brought in from another program (C35) is one more
numbered session. `hamlet_import` reads it on the chief's machine, the
client sends it as a file and `import`, and the server holds its entries
until the runtime makes the conversation with them. It opens with a
notice of where it came from. The chief's messages in it are the chief's
rows, not rewindable; the other program's tool calls are tool rows of
their tool and summary, kept for people alone.

The session menu of the Hub's chat lists the agent's sessions, its
conversations with other agents and the workers it started. Choosing a
sender's letters or questions opens a reference with `from` and `kind`;
without a kind the server selects letters. These watches have paging and
live updates, but no input field, stop control or rewind. The first entry
returns to the chief's own conversation. The server also refuses edits of
peer conversations.

A peer input carries `author`, its sender's address, rather than `chief`.
The client keeps it as an agent entry, including a letter's reply arriving
in the chief's chat. It is never drawn as the chief's own words.

## Order and drawing

Rows follow the transcript's order, even if a clock ran back. Rows by one
speaker to one listener that follow each other are a run: the first is
named and has the speaker's picture beside its name. The chief's own
messages are at the right on brass. The agent's are at the left, with a
disc in the paint of its robot. The steps of a turn are folded into one
quiet line that opens, under the reply before them. Steps that open a turn
stand under its first reply, joined by those after it.

When a transcript has an earlier page, scrolling to the top, or a chat
that fits without scrolling, sends `open` with `before` set to the first
item's ID. Only one page request is outstanding, and the returned items go
above the rows already there, which stay where they are on screen.

Sending a line triggers `ChiefMessage`, which the client sends as `say` to
the selected address. Typed slash lines are refused locally with directions
to use the window's controls; those controls send commands to the server.
Unaddressed server faults and error replies to the chat's own requests show
under the field and play the Error sound. Fault waits stay on the realm's page
in the Hub.

## Links

A web address or a file named in the chat can be clicked.

- `[words](address)` is a link when the address is `http`, `https` or
  `mailto`, or a path whose file is there. Any other scheme is plain words.
- A bare `https://..` in running text is always a link.
- A path in running text or inline code is a link when its file is there.
- Nothing in a fenced code block is a link.

**Which files.** Files resolve only on the window's machine. An absolute
path must exist there. A relative path is looked for under the selected
hamlet's received work folder, and becomes a link only if its canonical
path stays under that folder. `..` and symlinks are followed before that
check. The received realm folder is excluded after resolving links, for
absolute and relative paths alike. No file under an agent's lodge becomes a
link or a local picture. The work folder is a path on the server's machine;
with `hamlet dev` it is local too. Nothing downloads a server file.
Lodge-relative paths such as an agent's `memory/` or `skills/` stay plain
text unless they name a file in the local work folder.

**When the disk is asked.** Once for each distinct path when a paragraph
is built, never while reading. The answer is kept until a turn ends or the
selected chat, work folder or received realm folder changes.

**A click.** A web address goes to the system's browser. A file is checked
once more and then shown by the system's own program, given the path as
one argument with no shell between. A file the system would run and not
show (an executable, a script, an app) is revealed in its folder,
unopened.

## Edits

An `edit` or `write` call can show its change in place of the tool row:
the path, the count of lines added and removed, and the lines themselves
in red and green. `crates/hamlet_server/src/work/cut.rs` computes the preview and keeps
at most 200 lines, each cut at 240 characters. It arrives in the tool's
`change` field, both live and when the transcript is read back.

A change is open when it arrives and shows its first `change_lines` lines
(`[chat]` in `assets/ui.toml`), with a button for the rest. An edit that
was not applied says so with the reason. An edit that waits for the chief
in manual mode shows the same view in its card in the Hub. A change made by a shell
command is not shown.

## Replying

A message can answer another, to say which one it is about.

**In the window.** Every message that has words carries a small mark,
shown while the pointer is on its row. A click puts "Replying to" and
the start of the message over the field, with a cross to drop it. The
next message sent is the reply. A reply begun in one chat is dropped
when another chat is opened.

**What is sent.** The reply is part of the message's own words: a first
line that names the message it answers, its words as a JSON string, and
the words of the reply after a blank line:

```
(Replying to Mayor: "Paper or ebook?\nBoth ship soon.")

Paper.
```

So the model reads what is answered, and the log, a rewind and a replay
need nothing new. At most `reply_chars` characters of the message are
carried (`[chat]` in `assets/ui.toml`). The chief's own bubble draws the
first line as one dim line, the speaker and the start of the words, and
only the words after it as the message. A rewind to a reply puts what
it answered back over the field.

## Quoting

A line the chief types that opens with `> ` is a quote, as in the chat
apps: the bubble draws it dim beside a bar. It is no more than words,
with nothing behind it, and the model reads it as a Markdown quote.
Shift+Enter starts a new line at the caret, in place of what is selected,
and the field grows with its lines, so a quote can be followed by the chief's own words.

## Selecting words

Any text of the panels is selected where it is drawn, as on a web page:
a press and a drag, a double-click for a word, a triple-click for a line.
`hamlet_app/src/select.rs` holds it. An `Area` is selected as one across
all its text: the words of a message, and every Markdown document, so a
drag runs across the paragraphs, lists and code blocks of a reply or of
a memory file. Any other text is selected on its own. What a button, a
field, a menu row or anything with a pointer of its own holds is left to
it, and a field selects its own words as Bevy's text box does.

Bevy draws text but does not select it, so the selection is made with
what Bevy keeps of each text node: the parley layout of its words
(`ComputedTextBlock::buffer`). Parley's `Selection` finds the words under
the pointer and the boxes they fill. Bevy also breaks that layout at
other widths while it measures, so it is broken again at the width the
node is drawn at before it is read.

What is selected is highlighted, in `chosen` of the palette, on one layer over
the panels, under a dragged tab, the menus and the tooltips, and cut to
what clips the text. Selectable text shows the text cursor. Cmd+C copies
it, Cmd+A selects the whole area and Esc lets it go. The parts of nodes
on one line, such as a bullet and its item, are joined with a space, and
those on different lines with a line. The room a bubble keeps for its
hour is never selected. In the chat, QUOTE, at the end of what is highlighted,
puts the words in the field as `> ` lines with a line after them for the
chief's own words. A press anywhere else, or another chat, lets the
selection go. A plain click on a link still follows it.

## Rewind

A conversation can be taken back to before one of the chief's messages.
That message and everything after it leave the active conversation, and
its words go back in the field to be changed and sent again.

**The record.** The server forks the conversation from the entry before
that message and keeps its stable conversation number. The client sends
`rewind` with the chat reference and the item's string ID. A `rewound`
reply gives the words back; a reset followed by a transcript replaces the
shown items. The server refuses a rewind while work is running or queued
and closes waits raised by the part taken back.

**What is not undone.** Files changed, commands run, memory, skills, files
attached and everything spent. The question says so before anything happens.

**In the window.** Each rewindable chief message carries a small mark,
shown while the pointer is on the row and no turn runs. A click asks
first, under the message, and fades every row the rewind would forget
while it asks.

## Pictures

A picture an agent names in a reply, `![words](place)`, is drawn where the
reply put it. Pictures a reply names are shown only: the window sends no
bytes of them back to the server or a model. Images a chief attaches to a
line are another thing (see Images on a line below).

| The place is | What is drawn |
|---|---|
| A local picture file a link may lead to (`png`, `jpg`, `jpeg`, `webp`, `gif`, `bmp`) | The picture. A GIF is its first frame |
| A file that is too large, is no picture, or is another kind | A one-line chip with its words, its path as a link to the file, and why it is not shown |
| No local file, or an unresolved relative path | A one-line chip with its words, its path and why it is not shown |
| An `http` or `https` address | A chip, and the picture only after the chief asks |

**A picture of the web is never fetched by itself.** A model wrote the
address, and a fetch tells that server whatever the address carries. An
agent that was fed bad instructions could put a secret in the address and
have the chief's machine send it just by showing the chat. So the chip
shows the words, the host and a button, SHOW PICTURE, and only the press
fetches. The fetch is bounded in time, size and redirects, takes only an
answer that is an image, and keeps the picture in memory only.

**Reading.** No picture file is read in a frame. A background task reads
the file, turns it as its EXIF says and scales it down to at most twice the
size it is shown at. A picture too large to decode safely is refused.

**Kept.** One image is kept for a file as it was last written, and every
row that names it shares it. One that is shown is never dropped. Those no
row shows are kept up to `picture_memory` bytes, the oldest dropped first.
The limits are `[chat]` in `assets/ui.toml`.

## The field and its chips

The field is fynix's `rich_field`: text with inline elements, Enter to send and
Shift+Enter for a new line, with "Message" shown while it is empty. Everything
attached to a line is a chip in it, put at the caret: a small picture of an
image, or an icon for a file or a folder, then its name and, dimmer, its size
or how far it has got. The chip is an `Atom` of the field, so the caret steps
over it, a selection takes it and Backspace removes it, and the attachment is
let go with it. A cross in the chip does the same. A chip copies as the name
of what is attached. `attach.rs` keeps what is attached in `Attached` and
follows the field's content, which says which chips are still in it. The field
grows with its lines up to eight, and scrolls past them with the caret kept in
view.

On send the field's plain text, which has no chip in it, is the line's words,
trimmed, and the byte each chip stood at in it is its attachment's `at`. A
reply puts its line before the words, so every `at` moves by the length of
that line. Escape leaves the field with its words and chips kept. A line that cannot be sent yet, because an attachment is
still going up, waits for a word or was refused, leaves the field as it is.

## Images on a line

A chief attaches PNG, JPEG, WebP and GIF images to a line (C33), animated
ones too. Drop files on the window, paste with Cmd+V on macOS or Ctrl+V
elsewhere in the field, or press the picture button beside the field and
choose them. A paste takes image files copied in a file manager, or a picture
such as a screenshot; words on the clipboard are pasted as words. Each image
is uploaded at once, shown as a chip in the field, and sent with the line,
which may hold only images. A line waits for every image to be kept; the chip
of one the server refused is marked, and says why in its tooltip, until it is
taken out. Files are read off the frame, and one over 20 MiB is refused
unread.

## Files and folders on a line

The same ways attach any file or folder (C33): drop, paste, or the file and
folder buttons beside the picture button. A folder goes with everything under
it, links left out, and is one chip with its name and its files' count and
size. Each is uploaded at once as the images are, one file at a time
(`put-file`). The agent's model is not given a file; the line names where each
is saved in the agent's `inbox/`, and the agent opens it with its tools. Any
model takes files, so the file button is never greyed out. Images are given to
the model as on any line and keep their own limits.

What is attached other than images is held at 32 MiB or 1000 files: its chip
is marked and says so in its tooltip, and a line over the field says it too,
with "Send anyway". It is uploaded only after the chief presses that. A
rewind puts the files back in the field, in their places in the line's words.

**Where an attachment sits.** A line's images and files may each carry a place
in its words, a byte offset on a character boundary (`at` in
[the protocol](protocol.md)). The server puts each one's caption in the words at
its place, for the model, and the transcript keeps the words alone with each
place beside them. The window sends the place of each chip; the terminal chat
places the ones its `@path` tokens name.

A model that takes no images has the button greyed out, and a drag over
the window says it cannot be dropped; a paste or drop is refused the same way.
The chip of an image already in the field when the model is changed is greyed
out with a tooltip that says so, and the server refuses the line (S30).
The catalogue entry of the agent's model tells which: a model the catalogue
does not list is taken to see, and the server refuses what it cannot (S30).
The number of images a line takes is the model's limit. A chief says that a
model of a custom endpoint takes images, and within what limits, in the model
catalogue in Settings, or with `hamlet models images`.

A sent line shows each image as a chip in its words, at its place, with a small
copy the server makes (S29) fetched by its id when the row is built and kept as
a picture like the rest (see Pictures above), and each file or folder as a chip
with its name and size. The words of a line with a place are drawn plain, so a
link or a quote in them is drawn as in any line, with the chips among the words;
the attachments of a line with no place follow its words in a row of chips. A
click on an image's chip fetches the image itself and opens it with the
program the system keeps for it. The window only reads the bytes of what is
attached; the server checks, resizes and keeps them. A rewind puts the images of the line back in the field with its words.

The system reports no pointer with a drop, so the hand holds each dropped
file until the pointer is seen on something. Over the chat, a file is
attached. A file that is seen on nothing within a moment is let lie.

## Questions to the chief

An agent asks the chief with `ask_chief`, and its turn waits for the
answer. One call holds every question the agent has, so a skill that
grills the chief asks them all at once:

```json
{"message": "Before I plan the shelf.",
 "questions": [
   {"question": "Paper or ebook?",
    "description": "Shipping depends on it.",
    "options": [{"title": "Paper", "description": "Three days"},
                {"title": "Ebook"}],
    "text_input": true}]}
```

A call has 1 to 10 questions, and a question at most 6 options with
titles of its own. `description` is optional on a question and on an
option. `text_input` lets the chief answer in words of their own and is
true unless the agent says otherwise; a question with no options always
takes words. `crates/hamlet_server/src/talk/asking.rs` reads and
checks a call, and a call it refuses tells the agent why.

**Where it is answered.** One card, `hamlet_app/src/asking.rs`, is shown
on the realm's page and over the field of the chat with the agent that asked,
and both read the same answers, so what is begun in one stands in the
other. The card shows one question at a time, with "1 of 3" and the way
between them at its head. The options are a list of rows, each with its
letter, its title and its description, and the one picked is highlighted. A
line under them, Other, takes words. Each question takes one option or
words, not both, and picking an option moves on to the next question.
SUBMIT, on the last question, sends the answers when each question has
one, and otherwise shows the first without one. DISMISS tells the agent
to go on with its best judgement.

**The answer is the chief's reply.** The agent is given it as words, each
question quoted over the option picked or the words typed:

```
The chief answers:
> Paper or ebook?
Paper
```

A line typed to the agent in the chat or the terminal still answers the
whole card at once, as `The chief answers: ...`. The tool's description
tells the agent not to write the questions in its reply too, and to take
them as answered. In the chat the answer is a bubble of the chief's own,
`Entry::Answered`, made from the `ask_chief` result both as it comes and
when the log is read back. It has no mark to rewind to, since it is a
tool's answer and not a message of the log. A card dismissed shows
nothing.

The `asking.questions` limit counts the calls of one turn, not
the questions in them.

## What it costs

The chat is put in order again only when a conversation's shape changes:
an entry added, a tool ended, the selection or a fold changed. A word
added to a reply changes no shape.

The chat is a virtual list, fynix's `virtual_list`. Only the rows within
1,000 pixels above and below the view are built, at most 199 of them. A
row scrolled further away is not removed: its slot shows another row.
Each slot is keyed by its row, so it builds its row again only when it
is given another one. A row counts as 60 pixels high (`ROW_ESTIMATE` in
`screens/chat/lines.rs`) until it is first laid out. Once the median
height of the rows laid out in a frame is below 0.8 or above 1.25 times
that, the median takes its place. The scrollbar can shift a little as
rows are measured.

When the rows change, `fold_lines` compares them with the rows before by
their keys and tells the list which span changed. Rows added or removed
above the view do not move what is being read. The view stays at the end
as rows are added or grow, until it is scrolled up, and again once it is
back at the end. Sending, opening another conversation and the button at
the chat's lower corner take it to the end. The button shows whenever the
view does not follow the end.

A past conversation read from History is not a virtual list. It builds
its last 40 rows, and "Earlier" builds 40 more; once every row it holds
is built, "Earlier" also asks the server for the page before them.

A reply that grows is read again from its last settled block, and the blocks
before it are shared, not parsed or compared again.

Bevy marks every text input changed each frame, which would set its size and
lay out everything above it again. `hamlet_ui/src/fields.rs` runs that
size system only when a font, a scale, a new field or the lines a field
asks for have changed.
