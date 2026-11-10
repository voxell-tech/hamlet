# The warmth of the panels

How the panels around the world glow and answer the hand.

## The idea

The panels are a dark room at night, and the world is its window. Nothing
in the room gives light but small flames: a control warms like a lamp
being turned up, and a panel that needs you has a lamp burning over it. The
room stays dark so that the world stays the bright thing on screen.

## Vocabulary

| What | How it is drawn | What it means |
|---|---|---|
| Soot, wood and brass | Warm near-black panels, walnut buttons, brass frames and lettering | The room. It never glows |
| A flame in the glass | A pool of amber light low inside a control | The pointer is here, or you are typing here |
| A lamp that burns low | The one good action of a screen is the only control glowing at rest | This is what the screen is for |
| A hand over the lamp | A held button sinks, shrinks a little and goes dark | Your press has it |
| The flame catching | On release the flame flares, and the good action throws a few sparks | It was done |
| A coal | A refusing button glows red-brown under the pointer | This stops, refuses or throws away |
| Candle under a word | A low pool of light under a menu title or a tab | Where you are in a menu or a set of tabs |
| A lamp over a panel | Lamplight falls from the top of the Hub while something waits, and swells when something new arrives | Something waits for you |
| A breath of magic | A faint teal light crosses the top of the Hub when what it shows changes, another page or another agent | The Hub shows something else now. This and its tab's flash are the only teal in the panels |
| A glowing tab | A panel behind another carries its light on its tab: lamplight rising from the tab's foot, or a teal flash | Something happened in a panel that is not in front |
| A badge | A small amber pill with a number burns on the tab of a panel behind another while something waits in it | Something waits for you there, and how many |
| A state icon | An icon alone beside a name, with its word under the pointer: an orange bell for what needs you, a red stop for a fault, an ochre bolt with a soft round lamp glow under it while it works, a cool grey moon at rest, a purple merge mark, a red struck ring for a refusal | What an agent or a hamlet is doing |
| A status pill | A solid pill in one hue with an icon and light words | How something that is gone ended, such as merged into another |
| A status mark | An icon in a solid circle with a soft halo of its hue, before a bold title; work in progress is an open ring on one tinted circle | The outcome of something: a toast, a wait, a failed call |
| A solid button | A button filled with its hue: green restores or allows, purple merges, yellow tries again, red refuses | This completes the action, in the hue of what it does |
| A blue chip | A small blue box in the code face | A name that commands use, such as a hamlet's id |
| A speaker's paint | In the chat, a speaker's name and disc are in the paint of its robot | Who said it. The world's colours mark speakers and nothing else |
| An icon | A small line drawing before a word, in the word's own tone | What the word is, found sooner |

## Where it lives

In `hamlet_ui/src/`:

- `theme.rs` holds the theme: one variant's palette, from a family
  under `assets/themes/`, in the sizes, spacing and motion of
  `assets/measures.toml`. A family's file holds fifteen colours for
  each variant: `base0` to `base8`, a ramp from the background end to
  the text end, and `red`, `orange`, `yellow`, `green`, `blue` and
  `purple`, with an optional `accent` naming the hue of the brand
  colour, yellow unless set. `theme/derive.rs` makes every role of the
  palette from them: surfaces and the text ladder from the ramp, the
  accent from its hue, and a status from each of the others, each
  with an ink, a fill, and a solid form for pills, circles and
  buttons. Each ink is moved in lightness until it reads on its
  grounds. `theme/themes.rs` reads the families, and
  `theme/contrast.rs` measures how well one colour reads on another
  for every shipped theme. The panels are built once the first theme
  is shown.
- `hamlet_app/src/appearance.rs` picks the theme to show from the
  chief's appearance and the computer's color scheme. The Appearance
  page of Settings is `screens/settings/appearance.rs`.
- `styles.rs` holds the styles of each kind of button and field.
- `widgets/effects/` holds the behaviour: a control's warmth, press, flame and
  sparks, the light that falls into a panel and the light its tab
  carries while it is behind another. Its tuning is
  `EffectSettings`, from `assets/effects.toml`.

## Seeing it

`cargo run --example ui_gallery -- --scene controls --out <dir>` draws
every control in each state. The scene `waiting` shows the Hub with
questions, an approval and an edit waiting.
