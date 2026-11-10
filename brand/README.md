# Brand

Every logo here comes from the two drawings in `source/`: the logo on
its dark ground and on its light one. Change those, then make the rest
from them again.

| Folder | What is in it |
|---|---|
| `source/` | `hamlet-dark.svg` and `hamlet-light.svg`, the drawings everything else comes from |
| `logo/` | The house over the word: on its ground, and `-transparent` without it; `-light` for light pages |
| `mark/` | The house alone: `-light` for light pages, `-dark` on its own dark square |
| `icon/` | The app icon: the house on a rounded square, inside the margin macOS gives an icon. `.github/scripts/package.sh` makes the macOS icon from its PNG, and `crates/hamlet_desktop/src/icon.png` is the same at 256 pixels |
| `screens/` | Pictures of the window, for the README |

The PNGs are the SVGs beside them drawn with `rsvg-convert`, at the
sizes they are now.
