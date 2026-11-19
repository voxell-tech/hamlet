# Cursors

The pointer is the chief's taper: a brass-handled taper whose flame leans
toward what it points at. The tip of the flame is the hot spot. What is
pressed or held is in the chief's glove, the same hand that lifts a robot in
the world.

`cargo run --example cursor_sheet` writes every cursor to one sheet, large and
at its own size. `Mode` in `hamlet_app/src/cursor.rs` names them.

The resize cursors are ours and not the system's, because they also show on
dividers in the middle of the panels, where a white system arrow would break
the set.

## Which mode wins

`Mode::choose` in `hamlet_app/src/cursor.rs` decides, and the first rule that
holds is shown, in the order it lists them: a pending first-run request,
a robot held (`Carry`, drawn faint so the robot is seen through it), a camera
drag, the panels' own cursor (fynix sets a system cursor and
`Mode::of_system` turns it into a mode), a robot the hand could take, a
thing that takes a click, and otherwise the taper.

`cursor::dress` runs in `PostUpdate`, after fynix and the world have had their
say.

## Changing one

- **Colours** come from the dark variant of the theme picked, through
  `cursor::inks()`, in light mode too, so the cursor reads over every panel.
- **Shapes** and **hot spots** are in `cursor/art.rs`, each mode a short list
  of shapes in a square of 32 pixels.

After a change, look at the small swatches on the sheet: a cursor that only
reads at eight times its size does not read.
