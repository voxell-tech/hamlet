# The chat

What an agent's chat holds, in what order, and how it is drawn as a group
chat. The drawing is `hamlet_ui/src/screens/chat/` and `markdown/`. The
conversations themselves are `hamlet_core/src/session.rs`.

## What is recorded

Every conversation is a session: an entity with a `SessionKey`, a
`Transcript` and `TalksTo(agent)`, and a log in the house of the agent it
is with.

| Who speaks | Session it lands in |
|---|---|
| The chief, to an agent | `app`, `<chief>`, that agent |
| An agent, with `tell_agent` (a letter, not waited on) | `hamlet`, `<sender>`, the agent written to |
| The answer to a letter | Carried back to the session the sender wrote from |
| An agent, with `ask_agent` (waited on) | `asked`, `<sender>`, the agent asked |

An address is an agent's name, or `mayor` for whoever holds the office.

The asked agent's turn runs inside the asking agent's worker. Its events
are mirrored to the `asked` session as they arrive, so that session is
live and reads the same from its log later.

## Which conversations a chat holds

`timeline::parts` picks them for the selected agent:

- **With it**: the chief's session with it, and every `hamlet` and `asked`
  session it is the listener of. All of it is shown.
- **Written by it**: the `hamlet` and `asked` sessions it sent. Its own
  messages are shown, and for a question the other agent's answer too.

Nothing is shown twice. An answer exists both where it was given and where
it was carried back, and each chat shows the copy that is its own.

New Session puts away only the chief's own session with the agent. The
conversations between agents go on, and the chat shows of them only what
was said since the chief's session began.

## Order and drawing

Every row is laid out by the time it was said. Inside one conversation the
transcript's order is kept even if a clock ran back.

Rows by one speaker to one listener that follow each other are a run: the
first is named and the last has the speaker's picture. The chief's own
messages are at the right on brass. Everyone else's are at the left, with
a disc in the paint of the speaker's robot. A message for anyone but the
chief or the chat's own agent says so beside the name. The steps of a turn
are folded into one line that opens.

## Links

A web address or a file named in the chat can be clicked.

- `[words](address)` is a link when the address is `http`, `https` or
  `mailto`, or a path whose file is there. Any other scheme is plain words.
- A bare `https://..` in running text is always a link.
- A path in running text or inline code is a link when its file is there.
- Nothing in a fenced code block is a link.

**Which files.** A path is looked for in the folder the hamlet works on,
then in the workspace and the house of the agent the chat is with. The
path is made canonical first, so `..` and symlinks are followed, and it is
a link only if it is still inside one of those folders.

**When the disk is asked.** Once for each distinct path when a paragraph
is built, never while reading. The answer is kept until a turn ends or the
folders change.

**A click.** A web address goes to the system's browser. A file is checked
once more and then shown by the system's own program, given the path as
one argument with no shell between. A file the system would run and not
show (an executable, a script, an app) is revealed in its folder,
unopened.

## Edits

An `edit` call shows what it changes, in place of the row of a tool call:
the path, the count of lines added and removed, and the lines themselves
in red and green.

`Change::of` in `hamlet_core/src/tools/change.rs` compares the call's
`old` and `new` line by line and keeps the first 200 lines, each cut at
240 characters. It is made again from the arguments the log keeps, so a
session read back shows its changes too.

A change is open when it arrives and shows its first `change_lines` lines
(`[chat]` in `assets/ui.toml`), with a button for the rest. An edit that
changed nothing says "Not applied" with the reason. An edit that waits for
the chief in manual mode shows the same view in Needs You.

A change made by a shell command is not shown: only `edit` says what it
changes.

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
Shift+Enter starts a new line in the field, and the field grows with its
lines, so a quote can be followed by the chief's own words.

## Selecting words

Any text of the panels is selected where it is drawn, as on a web page:
a press and a drag, a double-click for a word, a triple-click for a line.
`hamlet_ui/src/select.rs` holds it. An `Area` is selected as one across
all its text: the words of a message, and every Markdown document, so a
drag runs across the paragraphs, lists and code blocks of a reply or of
the Notes page. Any other text is selected on its own. What a button, a
field, a menu row or anything with a pointer of its own holds is left to
it, and a field selects its own words as Bevy's text box does.

Bevy draws text but does not select it, so the selection is made with
what Bevy keeps of each text node: the parley layout of its words
(`ComputedTextBlock::buffer`). Parley's `Selection` finds the words under
the pointer and the boxes they fill. Bevy also breaks that layout at
other widths while it measures, so it is broken again at the width the
node is drawn at before it is read.

What is selected is lit, in `chosen` of the palette, on one layer over
the panels, under a dragged tab, the menus and the tooltips, and cut to
what clips the text. Selectable text shows the text cursor. Cmd+C copies
it, Cmd+A selects the whole area and Esc lets it go. The parts of nodes
on one line, such as a bullet and its item, are joined with a space, and
those on different lines with a line. The room a bubble keeps for its
hour is never selected. In the chat, QUOTE, at the end of what is lit,
puts the words in the field as `> ` lines with a line after them for the
chief's own words. A press anywhere else, or another chat, lets the
selection go. A plain click on a link still follows it.

## Rewind

A conversation can be taken back to one of the chief's messages. That
message and everything after it are forgotten, and its words go back in
the field to be changed and sent again.

**The record.** The log only grows. A rewind is one more line,
`{"type":"rewind","to":N}`, where `N` is the line of a `user` message. The
lines from `N` up to the rewind are then taken out when the conversation
is read (`log::read_standing`). What counts spending reads every line, so
the tokens of a part that was taken out still count.

**What is not undone.** Files changed, commands run, letters sent, agents
made, roads built, votes, notes, skills and everything spent. The question
says so before anything happens.

**The request.** `session::Rewind` is refused while a turn is queued or
runs. The line goes to the session's worker behind anything queued, so no
turn can run between. Oversight closes what the session raised since the
message.

**In the window.** Each of the chief's own messages carries a small mark,
shown while the pointer is on the row and no turn runs. A click asks
first.

## Pictures

A picture an agent names in a reply, `![words](place)`, is drawn where the
reply put it. Pictures are shown only: nothing of them is in a log or goes
to a model.

| The place is | What is drawn |
|---|---|
| A picture file a link may lead to (`png`, `jpg`, `jpeg`, `webp`, `gif`, `bmp`) | The picture. A GIF is its first frame |
| A file that is too large, is no picture, or is another kind | Its words, as a link to the file |
| No file, or one outside the folders | Its words, plain |
| An `http` or `https` address | A card, and the picture only after the chief asks |

**A picture of the web is never fetched by itself.** A model wrote the
address, and a fetch tells that server whatever the address carries. An
agent that was fed bad instructions could put a secret in the address and
have the chief's machine send it just by showing the chat. So the card
shows the words, the host and a button, SHOW PICTURE, and only the press
fetches. The fetch is bounded in time, size and redirects, takes only an
answer that is an image, and keeps the picture in memory only.

**Reading.** No file is read in a frame. A background task reads the file,
turns it as its EXIF says and scales it down to at most twice the size it
is shown at. A picture too large to decode safely is refused.

**Kept.** One image is kept for a file as it was last written, and every
row that names it shares it. One that is shown is never dropped. Those no
row shows are kept up to `picture_memory` bytes, the oldest dropped first.

The limits are `[chat]` in `assets/ui.toml`.

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
takes words. A call of the older form, `{"question": "..."}`, is one
question that takes words. `hamlet_rules/src/chief/asking.rs` reads and
checks a call, and a call it refuses tells the agent why.

**Where it is answered.** One card, `hamlet_ui/src/asking.rs`, is shown
in Needs You and over the field of the chat with the agent that asked,
and both read the same answers, so what is begun in one stands in the
other. The card shows one question at a time, with "1 of 3" and the way
between them at its head. The options are a list of rows, each with its
letter, its title and its description, and the one picked is lit. A
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

`[asking] questions` in `limits.toml` counts the calls of one turn, not
the questions in them.

## What it costs

The chat is put in order again only when a conversation's shape changes:
an entry added, a tool ended, the selection or a fold changed. A word
added to a reply changes no shape. Rows are keyed, so a row is built once.

Only a window of rows is built: a page at first, up to three pages, then it
slides. Reading to the top builds the page before it and, once the window is
full, drops the page at its far end. Reading to the bottom turns back. The row
at the edge is held in place through the turn. Sending jumps to the end.

A reply that grows is read again from its last settled block, and the blocks
before it are shared, not parsed or compared again.

Bevy marks every text input changed each frame, which would set its size and
lay out everything above it again. `fields.rs` runs that size system only
when a font, a scale or a new field asks for it.
