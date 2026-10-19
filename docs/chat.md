# The chat

An agent's watched conversation, its rows and the links and pictures they
hold. The drawing is `hamlet_ui/src/chat/` and `markdown/`. The client
transcript is `hamlet_client/src/chats.rs`; the server keeps the
conversations through pi-durable. [Protocol v0](protocol.md) describes the
wire messages, watch rules and page boundaries; this page covers the window.

## The conversation and its watch

A chat entity has `Chat(ChatRef)`, `Transcript`, `Status`, `Usage` and a
`TalksTo(agent)` relationship. Its reference names a hamlet and an agent,
with `mayor` for whoever holds the office. Adding the chat sends `open`;
removing it sends `close`. Reconnecting opens every watched chat again.
An open office chat follows the new mayor when the office changes.

The server answers with a `transcript` page. Live `turn` operations add or
replace an item, append text, close a tool, change status or usage, or reset
the transcript. The client applies those operations; it runs no agent and
reads no conversation files. Each item has a stable string ID, a time and
a kind: chief input, assistant text, reasoning, tool, notice or error.

The chief's chat with an agent is shared by every client. New Session sends
`/new` through `say` and moves to a new numbered conversation while the old
one finishes its queue. History comes from `read history`; Read opens a
numbered past conversation read-only, and Resume sends `/resume <n>`.
Every conversation is kept. Search matches its title and who began it.

AGENT CHATS in the inspector lists the selected agent's conversations with
other agents, with their counts and last activity. Choosing letters or
questions opens a reference with `from` and `kind`; without a kind the
server selects letters. These watches have paging and live updates, but no
input field, stop control or rewind. CHIEF CHAT returns to the chief's own
conversation. The server also refuses edits of peer conversations.

A peer input carries `author`, its sender's address, rather than `chief`.
The client keeps it as an agent entry, including a letter's reply arriving
in the chief's chat. It is never drawn as the chief's own words.

## Order and drawing

Rows follow the transcript's order, even if a clock ran back. Rows by one
speaker to one listener that follow each other are a run: the first is
named and the last has the speaker's picture. The chief's own messages are
at the right on brass. The agent's are at the left, with a disc in the
paint of its robot. The steps of a turn are folded into one line that opens.

Rows build 24 at a time. When a transcript has an earlier page, scrolling
to the top sends `open` with `before` set to the first item's ID. Only one
page request is outstanding, and the returned items go above the rows
already there. The hint reads "Earlier lines. Scroll up to read them."

Sending a line triggers `UserMessage`, which the client sends as `say` to
the selected address. Typed slash lines are refused locally with directions
to use the window's controls; those controls send commands to the server.
Unaddressed server faults and error replies to the chat's own requests show
under the field and play the Error sound. Fault waits stay in Needs You.

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
check. The optional received `houses` folder is excluded after resolving
links, for absolute and relative paths alike. No file under an agent's
house becomes a link or a local picture. The work folder is a path on the
server's machine; with `hamlet dev` it is local too. Nothing downloads a
server file. House-relative paths such as an agent's `NOTES.md` or
`skills/` stay plain text unless they name a file in the local work folder.

**When the disk is asked.** Once for each distinct path when a paragraph
is built, never while reading. The answer is kept until a turn ends or the
selected chat, work folder or received houses folder changes.

**A click.** A web address goes to the system's browser. A file is checked
once more and then shown by the system's own program, given the path as
one argument with no shell between. A file the system would run and not
show (an executable, a script, an app) is revealed in its folder,
unopened.

## Edits

An `edit` or `write` call can show its change in place of the tool row:
the path, the count of lines added and removed, and the lines themselves
in red and green. `crates/hamlet-server/src/change.rs` computes the preview and keeps
at most 200 lines, each cut at 240 characters. It arrives in the tool's
`change` field, both live and when the transcript is read back.

A change is open when it arrives and shows its first `change_lines` lines
(`[chat]` in `assets/ui.toml`), with a button for the rest. An edit that
was not applied says so with the reason. An edit that waits for the chief
in manual mode shows the same view in Needs You. A change made by a shell
command is not shown.

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

**What is not undone.** Files changed, commands run, notes, skills, gifts
and everything spent. The question says so before anything happens.

**In the window.** Each rewindable chief message carries a small mark,
shown while the pointer is on the row and no turn runs. A click asks
first.

## Pictures

A picture an agent names in a reply, `![words](place)`, is drawn where the
reply put it. Pictures are shown only: the window sends no picture bytes
back to the server or a model.

| The place is | What is drawn |
|---|---|
| A local picture file a link may lead to (`png`, `jpg`, `jpeg`, `webp`, `gif`, `bmp`) | The picture. A GIF is its first frame |
| A file that is too large, is no picture, or is another kind | Its words, as a link to the file |
| No local file, or an unresolved relative path | Its words, plain |
| An `http` or `https` address | A card, and the picture only after the chief asks |

**A picture of the web is never fetched by itself.** A model wrote the
address, and a fetch tells that server whatever the address carries. An
agent that was fed bad instructions could put a secret in the address and
have the chief's machine send it just by showing the chat. So the card
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

## What it costs

The chat is put in order again only when a conversation's shape changes:
an entry added, a tool ended, the selection or a fold changed. A word
added to a reply changes no shape. Rows are keyed, so a row is built once.
