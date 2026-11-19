# Comment convention

## As less as possible

No comment if the code already explains itself, and when it is a toss-up
whether one earns its place, remove it. Public APIs need doc comments, but
only to describe exactly what the thing is.

A simple type's doc comment names what it is, linking the real type it wraps,
and stops there. The reason belongs inline, at the code that depends on it.

Not: `/// One resource. Bevy parks each resource on an entity of its own...`
Just: `/// Inspector for a [`Resource`].`

## If the LSP already shows it, skip it

Do not restate a signature, return type, trait bound or visibility; write
what tooling cannot show: intent, a non-obvious constraint, a reason.

## Describe the current code

A comment describes the code as it is, not the diff that produced it: no
"was X, now Y".

## Plain text

Use plain English, with `:`, `;` and `-` sparingly, and ASCII only: no em
dashes or smart quotes.

## What a doc comment covers

A doc comment (`///`, `//!`) describes the thing as a whole, what it is for
and how to use it, and does not list its callers, setters or generic
instantiations ("set at `new`, changed through `theme_mut`"): such a list
goes stale. Prefer the version that stays true after nearby code changes.

## State it one way

Say what a thing is, not what it is not.

Not: "This is xyz instead of abc."
Just: "This is xyz."

## Open with a noun phrase

No "what X does/wants" openers; head the sentence with the thing itself.

## Where call-site behavior belongs

Behavior of one call site or branch is a `//` comment at that code, not in the
function's doc comment.

## Formatting

Write comments in a single line (unless separating paragraphs); the nightly's
`cargo fmt` wraps them.
