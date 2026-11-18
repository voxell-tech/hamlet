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
replace an item, append text, close a tool, remove an item, change status
or usage, or reset the transcript. The client applies those operations; it
runs no agent and reads no conversation files. Each item has a stable string ID, a time and
a kind: chief input, assistant text, reasoning, tool, notice, error,
answered, summary or merged.

## One conversation

The Hand and each hire have one conversation, shown alike by every client.
The chief's lines, other agents' messages and its workers' answers all go
into it, one turn at a time. A worker has its own.

A message from another agent carries `author` and `from` instead of
`chief`, and `message`: notification, request, response or error
response. The window draws it as a card headed by its kind and sender's
chip, folded to its first line; a press or Enter opens it (`bubble.rs`).
The model reads it under a bracketed heading, as
`[A request from hire#3 (Mira); your final answer goes back to it as the response]`.
A `notify` or `request` call is a step that names the agent it reached in
`agent`. An error response starts no turn; its card is edged in red and
reads "It could not answer: " and the cause in plain words.

**Queued lines.** An input waiting its turn is one of the last items, with
`queued`, in run order: the chief's lines and retries first, then agents'
messages. Taken up, it is replaced in place (`add` with `replaces`). The
window draws it dimmed, marked "Queued". The chief takes one back with
`withdraw`, and its row leaves with `remove`.

**Stop holds the queue.** Stop ends the running turn and holds the queue
until the chief sends a line, which runs first, or resumes it (`resume`,
Resume in the chat, `/resume` in the terminal). `status` carries `held`,
and the chat ends with "Stopped · 3 waiting" and Resume. Workers go on,
and their answers queue under the hold (`conversations/chats.rs`).

**Compaction.** pi summarizes the earlier conversation when the context
fills (C28), or when the chief asks with "Compact now" or `/compact`. The
summary is a `summary` item, a line between turns that opens to it.

**A merge.** The kept hire's conversation gets a `merged` item whose
`from` is the absorbed hire, a line that opens that hire's conversation in
place. It stays as it was, so a split gives it back.

**Lines between turns.** A change to the agent is an `event` item for
people alone (`chats::event`), drawn as a centred line (`card.rs`): a
model or effort change, a stop or resume, a dream, becoming a scholar of
the city, a rename, the fast tier.

## Brought in from another program

A hire made for a conversation brought in from another program (C35)
has it as its conversation. The card "Bring in your
projects" (`hamlet_app/src/welcome/projects.rs`) lists the projects
found on the window's machine, or in an export file chosen in the
system's dialog; it opens once when first-run setup ends on a realm with
no hamlet, and from Settings. `hamlet_import` reads the conversations on
the chief's machine, off the frame (`hamlet_client/src/imports.rs`), the
client sends each as a file and the project as `import-project`, and the
server (`conversations/imports.rs`) founds or finds the hamlet, makes the hires
and holds each conversation's entries until the runtime makes it with
them, which waits for a model to be set up. Until then its chat shows
the entries held, and its watchers get its transcript anew once the
runtime keeps them. It opens with a notice of where it came from. The chief's messages
in it are the chief's rows, not rewindable; the other program's tool
calls are tool rows of their tool and summary, kept for people alone.

## Order and drawing

Rows follow the transcript's order, even if a clock ran back. Rows by one
speaker to one listener that follow each other are a run: the first is
named and has the speaker's picture beside its name. The chief's own
messages are at the right on brass. Another agent's are at the left, with
a disc in the paint of its robot.

**A turn.** Everything the agent does between one input and the next is
one turn (`lines.rs`, `Turn`): its thinking, tool calls and words between
them are steps, and its last words are the reply. A line between turns
that comes during it stands among its steps. The turn runs from the input
the agent was seen to take up. While it runs, a header reads "Working for
13s" or "Thinking", shimmering (`hamlet_ui/src/widgets/shimmer.rs`), with
every step under it, the reply's words too, so nothing moves. Each step is
a quiet line with an icon that opens to its details.

Once the turn ends the header reads "Worked for 2m 49s", with " · 1
failed" in red when a step failed, and folds its steps away. The time runs
from the first step to the end of the last. Thinking folds to "Thought for
6s". The reply follows across the chat with no bubble, then a card of the
files the turn's edits changed (`edits.rs`). Queued lines stand under the
running turn.

Only the rows near the view are built. Each row is shown whole in the
frame it grows or is built, and the view kept at the end or at the first
row shown (`anchor.rs`). The view moves only when the chief scrolls, sends
a line or asks for the end.

When a transcript has an earlier page, scrolling to the top, or a chat
that fits without scrolling, sends `open` with `before` set to the first
item's ID. Only one page request is outstanding, and the returned items go
above the rows already there, which stay where they are on screen.

Sending a line triggers `ChiefMessage`, which the client sends as `say` to
the selected address. Typed slash lines are refused locally with directions
to use the window's controls; those controls send commands to the server.
Unaddressed server faults and error replies to the chat's own requests show
under the field and play the Error sound. Fault waits stay on the realm's page
in the Hub. Faults with a `setup` cause, no model or refused credentials,
share one toast per cause and model, as "5 mayors can't use Claude Sonnet
5.5" (`toasts.rs`); a fault's words come from `conversations/failures.rs`.

## Links

A web address, a file or an agent named in the chat can be clicked.

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

## Agents named in the chat

An agent's or a scholar's address in a line is drawn as a chip: the glyph
of what it is, its current name, and the address in its tooltip. A click
opens the agent's chat or the scholar's page.

- `hire#<id>`, `worker#<id>`, `scholar#<id>` and `mayor@<hamlet>` are
  chips in running text, inline code and link addresses, as
  `[Mira](hire#3)`.
- `hand` is one only in inline code or as a link's address.
- A copied address's name, `hire#3 (Mira)`, is part of the chip.
- An address that names no one stays words, as does anything in a fenced
  code block.

While an answer grows, its addresses are words in the link's colour, and
become chips once it is whole (`unsettle` in
`hamlet_ui/src/markdown/draw.rs`): a chip in a reflowing line would jump.
A line with a chip of unknown size stays hidden until the chip is laid
out (`hamlet_ui/src/markdown/chips.rs`).

A table's columns are each at least as wide as their longest word or chip,
and share the rest in proportion to their widest cell.

"Copy Agent ID" in an agent's menu copies `hire#3 (Mira)`; "Copy Hamlet
ID" copies `hamlet <id> (<name>)`.

In the field, an address becomes a chip once written whole: pasted, or
typed and followed by a space, a mark or a chip (`screens/chat/field.rs`).
A bracketed name must be its current one, and `hand` needs its name. The
chip is an atom of the field and is sent as its words.

## Edits

An `edit` or `write` call carries its change: the path, the count of
lines added and removed, and the lines themselves in red and green, shown
in its step and the turn's card of changed files.
`crates/hamlet_server/src/tools/edit_diff.rs` computes the preview and
keeps at most 200 lines, each cut at 240 characters. It arrives in the tool's
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
hour is never selected. In the chat, Quote, at the end of what is highlighted,
puts the words in the field as `> ` lines with a line after them for the
chief's own words. It stands over the window, so a selection never lays
the message out again. A press anywhere else, or another chat, lets the
selection go. A plain click on a link still follows it.

## Rewind

A conversation can be taken back to before one of the chief's messages.
That message and everything after it leave the active conversation, and
its words go back in the field to be changed and sent again.

**The record.** The server forks the conversation from the entry before
that message and makes the fork the agent's conversation, trusting
nothing yet. The client sends
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
shows the words, the host and a button, Show Picture, and only the press
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

Each chat keeps its own draft, words and chips, for the window's life
(`Unsent` in `screens/chat/field.rs`). An attachment belongs to its draft
and keeps going up while another chat is shown.

## Images on a line

A chief attaches PNG, JPEG, WebP and GIF images to a line (C33), animated
ones too. Drop files on the window, paste with Cmd+V on macOS or Ctrl+V
elsewhere in the field, or press the picture button in the composer's
toolbar and choose them. The toolbar fits any width (`hub/thread.rs`,
`crowd`): the model's name is cut first, then the icon buttons fold into
one "More" menu, then the hamlet's mode shows its shield alone. A paste
takes image files copied in a file manager, or a picture
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

A sent line shows its images over its words as a wrapping row of square
tiles of the small copy the server makes (S29), fetched by id and kept as a
picture like the rest. A tile keeps its size while its copy comes. Each
image and file is also a chip in the words at its place; the attachments
of a line with no place follow its words. The window only reads the bytes
of what is attached; the server checks, resizes and keeps them. A rewind
puts the images back in the field.

A chip is as high as a line of the words around it and shares their
baseline.

**The viewer.** A click on an image's chip or tile, or its zoom button,
shows the image over the window on a veil (`screens/viewer.rs`): the small
copy first, then the image itself once read off the frame, fitted in the
window and never drawn past one screen pixel to each of its own. Escape, a
click on the veil or the close button closes it.

The system reports no pointer with a drop, so the hand holds each dropped
file until the pointer is seen on something. Over the chat, a file is
attached. A file that is seen on nothing within a moment is let lie.

## Questions to the chief

An agent puts questions to the chief with `request_chief`, and its turn
waits for the answer. It asks from any turn of its conversation; a worker
or an errand asks only in the turn of a chief's own line. One call holds
every question the agent has:

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
takes words. `crates/hamlet_server/src/conversations/questions.rs` reads and
checks a call, and a call it refuses tells the agent why.

**Where it is answered.** One card, `hamlet_app/src/asking.rs`, is shown
on the realm's page and over the field of the chat with the agent that asked,
and both read the same answers, so what is begun in one stands in the
other. The card shows one question at a time, with "1 of 3" and the way
between them at its head. The options are a list of rows, each with its
letter, its title and its description, and the one picked is highlighted. A
line under them, Other, takes words. Each question takes one option or
words, not both, and picking an option moves on to the next question.
Submit, on the last question, sends the answers when each question has
one, and otherwise shows the first without one. Dismiss tells the agent
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
`Entry::Answered`, made from the `request_chief` result both as it comes and
when the log is read back. It has no mark to rewind to, since it is a
tool's answer and not a message of the log. A card dismissed shows
nothing.

The `asking.questions` limit counts the calls of one turn, not
the questions in them.

## What it costs

The chat is put in order again only when a conversation's shape changes:
an entry added, a tool ended, the agent beginning or ending its answer,
the selection or a fold changed. A word
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
back at the end. Sending and the button at the chat's lower corner take it
to the end. The button shows whenever the view does not follow the end.

Each chat is shown again where the chief left it, for the window's life,
by `Place`: at its end, or with the row first in view. A chat not shown
before opens at its end.

A reply that grows is read again from its last settled block, and the blocks
before it are shared, not parsed or compared again.

Bevy marks every text input changed each frame, which would set its size and
lay out everything above it again. `hamlet_ui/src/fields.rs` runs that
size system only when a font, a scale, a new field or the lines a field
asks for have changed.
