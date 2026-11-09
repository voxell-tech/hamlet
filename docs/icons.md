# The icons of the panels

The small pictures beside the words on buttons, tabs, menus, headings and
the status bar. They are one family of line drawings made in code, so
there is no icon font, no file to load and no licence to carry.

## Where they live

`hamlet_ui/src/widgets/icons.rs` is the only place an icon is drawn.

- `Glyph` names every icon. A name says what the picture is (`Moon`,
  `Bell`, `Check`) unless the picture is the hamlet's own thing (`Mayor`,
  `Hamlet`, `Hearth`).
- `glyph(Glyph::Bell, 14.0)` is the icon as a view. It takes `.tone(...)`
  like a label, and a button's look tints it with its lettering.
- `icons::draw` draws every icon once at startup, at a few sizes and twice
  the pixels.

`styles.rs` holds the helpers that set an icon beside words, such as
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
the answers of a dialog, or in place of a word the chief reads. Two places
drop a word for an icon, and there the icon has a tooltip that names it:
the status bar, and a tight stack of sliders for one thing's look, such
as the volumetric fog on the Sight page of Settings.

An agent's state has one glyph, tone and word wherever it is shown, set
in `hamlet_app/src/standing.rs`: `Bell` waiting for the chief, `Stop`
stopped, `Bolt` at work, `Moon` at rest, `Gone` gone.

## One meaning each

An icon means one thing across the app, so the eye can trust it. Those
that name a part of Hamlet:

- `Tree` the Hub, `Globe` the world and the realm, `Hamlet` a hamlet,
  `Scholar` a scholar and the city, `Anchor` the harbor, `Signpost`
  roads, `Hearth` the chief's hearth.
- `Tower` the Hand, `Mayor` a mayor and the leads, `Robot` a hire or a
  worker, `Face` an agent's character, `Notes` what it is told in
  writing (its prompt, memory and the charter), `Book` skills, the
  library and the bylaws.
- `Chip` a model, `Fast` the fast tier, `Key` a provider's key, `Signal`
  a connection or a sign-in, `Shield` the sandbox and the workspace.
- `Lens` a search, `Sliders` settings, `Speaker` sound, `Eye` how
  something looks or showing it, `Sun` the Sight page.
- `Power` an agent's tools, `Leave` closing the window.

Before giving an icon a new use, search for `Glyph::<Name>` and keep to
its meaning; draw a new one when none fits.

## File icons

The icon beside each entry of the Files panel's tree is not one of these.
It is a file icon from Catppuccin's VS Code icons, in its own colours, kept
with their licence in `hamlet_ui/src/widgets/file_icons/` and drawn by
`file_icons.rs` the first time it is shown. A file takes the icon of its
whole name, else of its longest ending, as VS Code finds it. They are for
that tree alone. `file_icons/import.mjs` brings in a newer release.

## Adding one

1. Add its name to `Glyph` and to `Glyph::ALL`, in the same order.
2. Give it shapes in `Glyph::shapes`, by the family's rules.
3. Look at it with `cargo run --example ui_gallery -- --scene icons --out
   <dir>`, and judge it at 12 and 14 pixels.
