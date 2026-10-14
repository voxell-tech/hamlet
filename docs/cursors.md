# Cursors

The pointer is the chief's taper: a brass-handled taper whose flame leans
toward what it points at. The tip of the flame is the hot spot. What is
held is held in the chief's glove, the same hand that lifts a robot in the
world.

`cargo run --example cursor_sheet` writes every cursor to one sheet, large
and at its own size.

## The modes

| Mode | Picture | When |
|---|---|---|
| `Point` | The lit taper | Nothing else holds |
| `Press` | The flame flares | Over a button, a tab, a link, or something of the world that takes a click |
| `Pick` | A glass on a brass handle | Over a thing of the world that can be selected |
| `Grab` | The open glove | Over a robot that can be lifted, or a dock tab |
| `Carry` | The closed glove | A robot is off the ground, or a dock tab is being dragged |
| `Slide` | Four arrowheads | The left button drags the ground |
| `Turn` | Two curved arrows | The right button turns the view |
| `Write` | An upright bar | Over a text field |
| `Resize(axis)` | A two-headed arrow | At an edge of the window, a divider, or a number field that is dragged |
| `Refuse` | A barred ring | A panel refuses |
| `Wait` | A flame with fireflies going round it | An action is about to run |

The resize cursors are ours and not the system's, because they also show
on dividers in the middle of the panels, where a white system arrow would
break the set.

## Which mode wins

`Mode::choose` in `hamlet_ui/src/cursor.rs` decides, and the first of
these that holds is shown:

1. Something waits: `Wait`.
2. The hand holds a robot off the ground: `Carry`, drawn faint so the
   robot is seen through it.
3. A drag moves the camera: `Slide` or `Turn`.
4. The panels ask for a cursor. Fynix sets a system cursor on the window,
   and `Mode::of_system` turns it into a mode.
5. The hand could take what is under the pointer: `Grab`.
6. Something of the world takes the click: `Press`.
7. A thing of the world is under the pointer: `Pick`.
8. Otherwise `Point`.

`cursor::dress` runs in `PostUpdate`, after fynix and the world have had
their say, and replaces the window's cursor only when the wanted picture
differs from the one shown.

## Tuning

- **Colours** come from the theme through `cursor::inks()`.
- **Shapes** and **hot spots** are in `cursor/art.rs`. Each mode is a
  short list of shapes in a square of 32 pixels.
- **Size.** A cursor is 32 by 32 pixels, and winit takes no scale. On a
  retina screen the system doubles it, so it is softer than a system
  cursor there. A larger image would be a larger cursor, not a sharper
  one.

After a change, look at the small swatches on the sheet: a cursor that
only reads at eight times its size does not read.
