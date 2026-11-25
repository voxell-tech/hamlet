# The icons of the panels

The small pictures beside the words on buttons, tabs, menus and headings: one
family of line drawings made in code, with no icon font or file to load.

`hamlet_ui/src/widgets/icons.rs` is the only place an icon is drawn, and
`Glyph` names every icon. `glyph(Glyph::Bell, 14.0)` is the icon as a view; it
takes `.tone(...)` like a label.

## The family

Every icon is drawn in a square of 16 units from a short list of `Shape`s.

- One round pen, a tenth of the icon's side wide, kept between 1.25 and 1.8
  pixels.
- Ink fills the square to about a unit from its edge and never touches it, so
  every icon has the same optical size.
- Outline, not silhouette. A solid part is small.
- No colour of its own. An icon is white and takes a text tone.

## Where an icon goes

An icon goes before a word that names an action or a state. It does not go
on every row of a form, on the answers of a dialog, or in place of a word the
chief reads; where it does replace a word, it has a tooltip that names it.

An agent's state has one glyph, tone and word wherever it is shown, set in
`hamlet_core/src/standing.rs`.

An icon means one thing across the app. Before giving one a new use, search
for `Glyph::<Name>` and keep to its meaning; draw a new one when none fits.

## Adding one

1. Add its name to `Glyph` and to `Glyph::ALL`, in the same order.
2. Give it shapes in `Glyph::shapes`, by the family's rules.
3. Look at it with `cargo run --example ui_gallery -- --scene icons --out
   <dir>`, and judge it at 12 and 14 pixels.
