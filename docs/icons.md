# The icons of the panels

The small pictures beside the words on buttons, tabs, menus, headings and
the status bar. They are one family of line drawings made in code, so
there is no icon font, no file to load and no licence to carry.

## Where they live

`hamlet_sdk/src/icons.rs` is the only place an icon is drawn.

- `Glyph` names every icon. A name says what the picture is (`Moon`,
  `Bell`, `Check`) unless the picture is the hamlet's own thing (`Mayor`,
  `Hamlet`, `Hearth`).
- `glyph(Glyph::Bell, 14.0)` is the icon as a view. It takes `.tone(...)`
  like a label, and a button's look tints it with its lettering.
- `icons::draw` draws every icon once at startup, at a few sizes and twice
  the pixels.

`hamlet_sdk/src/looks.rs` holds the helpers that set an icon beside words, such as
`marked` and `action`.

## The family

Every icon is drawn in a square of 16 units from a short list of `Shape`s.

- One round pen, a tenth of the icon's side wide.
- Ink fills the square to about a unit from its edge and never touches it,
  so every icon has the same optical size. A test holds the edge clear.
- Outline, not silhouette. A solid part is small.
- No colour of its own. An icon is white and takes a text tone.

## Where an icon goes

An icon goes before a word that names an action or a state, where it lets
the eye find the word sooner. It does not go on every row of a form, on
the answers of a dialog, or in place of a word the chief reads. Only the
status bar drops a word for an icon, and there the icon has a tooltip.

The states use the same four everywhere: `Moon` at rest, `Bolt` at work,
`Bell` waiting for the chief, `Gone` removed.

## Adding one

1. Add its name to `Glyph` and to `Glyph::ALL`, in the same order.
2. Give it shapes in `Glyph::shapes`, by the family's rules.
3. Look at it with `cargo run --example ui_gallery -- --scene icons --out
   <dir>`, and judge it at 12 and 14 pixels.
