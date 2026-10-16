# The society and the realm

Agents run on the server. The window receives their hamlets, roads,
departures, votes, library and waits through protocol v0; it reads no
records from the home's disk. The glossary is [`CONTEXT.md`](../CONTEXT.md).

## Agents and roads

The mayor creates agents with a purpose and reason, and can give each a
model, character and roads. Every living agent has a road to its hamlet's
office, whoever holds it. Built roads cost the hamlet and the task that
built them; restored roads are not charged again.

A letter returns its reply later as a new input to the sending
conversation. A question waits for the other agent's answer. Each receiver
has separate conversations for the sender's letters and questions, both
kept across restarts. The task follows their work and charges. A reply is
delivered even when the message cap has been reached. Messages between
hamlets are paid by the sender's hamlet.

The inspector's AGENT CHATS page opens those conversations read-only,
choosing letters or questions. Their authors are shown as agents, never
as the chief. See [`chat.md`](chat.md) for paging and watches.

## Leaving and returning

Removal keeps the agent's house where it is and archives its record.
A merge keeps the absorbed house too, copies its files into the kept
house through the sandbox, and keeps a manifest in storage for splitting.
Clashing files are kept aside and the notes are combined. A chief's
unfinished instruction protects an agent from a mayor's removal or merge.

A chief can remove, merge, restore, split or erase through the agent's
menus or terminal commands. `/undo` reverses the latest reversible removal
or merge. Restoration keeps the agent's number. Erasure deletes the
archived house for good. A letter to a departed agent is refused with its
farewell, or the agent to talk to after a merge.

## Votes, the record and the rules

A chief or an agent can call a vote on replacing the mayor, and agents can
propose founding another hamlet. The voters are fixed when it is called.
Each ballot is a durable errand with only `cast_ballot`, a reason and
evidence checked against that voter's own notes or conversations. Sealed
ballots run together; unsealed ballots run in order. A replacement vote
hears the current mayor first. A voter that does not answer is not a yes.

The server reports votes and their results in the chief's mayor chat.
The hamlet's RECORD tab shows received vote reports, with OVERTURN when
allowed, and asks the server for `/record` in words. It refreshes after
changes and reconnects. An already-open mayor chat follows the office.

BYLAWS edits the hamlet's vote limits: the pass fraction, least voters,
sealing, grounded evidence and time limits. The realm's limits appear only
on `home`'s BYLAWS page: the hamlet cap, roads between hamlets and how many
mayors must ask for another hamlet's removal. Only a chief changes limits.

## The shared library and chief oversight

The library starts with the shipped skills. An agent offers a skill using
`submit_skill`; the librarian judges it in one model call, with its notes
shown but no tools. `ACCEPT` as the first word shelves it. Other replies
refuse it. `ask_librarian` asks about the shelf, and `/unshelve` takes a
skill out. The shelf and each decision are kept by the server.

A chief's instruction is recorded before it starts. In a chief
conversation an agent may object once and wait for go or withdraw. The
mayor's configured review runs beside a direct instruction to another
agent and may raise its own objection. An agent overseen by a chief can
ask a question and wait for reply or dismiss. These waits appear in Needs
You and survive a server restart.

## Several hamlets

`/found <hamlet> <mayor> <purpose>` founds a hamlet. Its id is never reused,
even after erasure, and living agent names are unique across the realm.
`mayor@mill` addresses mill's office; `/hamlet mill` switches the terminal
to it, and `/agent <name>` finds a living agent anywhere in the realm.

Mayors have roads to one another. Another road, or an agent's move,
requires both hamlets' mayors to agree. A move carries the agent's house,
character and conversations to the next number in its new hamlet. It keeps
only the roads both mayors named and takes back its lasting permissions.
The record commit precedes the house rename; a pending rename is finished
on reopening the home.

Mayors can ask for a hamlet to close, but only a chief confirms removal.
A ready request is a notice naming `/remove-hamlet <id>`, not a new wait.
The first hamlet, `home`, cannot be removed. Closed hamlets retain their
houses and records and appear under CLOSED DOWN in the overview.
RESTORE brings one back. DELETE FOR GOOD asks first and permanently erases
its houses; that question is discarded on restore or leaving the page.

The server sends whole replacement states when the library shelf or
closed-hamlet list changes, or a hamlet leaves the live list. The client
reconciles surviving entities and removes those no longer sent. The
terminal also replaces its addresses from those states.
