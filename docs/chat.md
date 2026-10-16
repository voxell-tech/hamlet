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

## What it costs

The chat is put in order again only when a conversation's shape changes:
an entry added, a tool ended, the selection or a fold changed. A word
added to a reply changes no shape. Rows are keyed, so a row is built once.
