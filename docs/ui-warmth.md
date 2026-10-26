# The warmth of the panels

How the panels around the world glow and answer the hand.

## The idea

The panels are a dark room at night, and the world is its window. Nothing
in the room gives light but small flames: a control warms like a lamp
being turned up, and a panel that needs you has a lamp lit over it. The
room stays dark so that the world stays the bright thing on screen.

## Vocabulary

| What | How it looks | What it means |
|---|---|---|
| Soot, wood and brass | Warm near-black panels, walnut buttons, brass frames and lettering | The room. It never glows |
| A flame in the glass | A pool of amber light low inside a control | The pointer is here, or you are typing here |
| A lamp that burns low | The one good action of a screen is the only control lit at rest | This is what the screen is for |
| A hand over the lamp | A held button sinks, shrinks a little and goes dark | Your press has it |
| The flame catching | On release the flame flares, and the good action throws a few sparks | It was done |
| A coal | A refusing button glows red-brown under the pointer | This stops, refuses or throws away |
| Candle under a word | A low pool of light under a menu title or a tab | Where you are in a menu or a set of tabs |
| A lamp lit over a panel | Lamplight falls from the top of Needs You while something waits, and swells when something new arrives | Something waits for you |
| A breath of magic | A faint teal light crosses the top of the Inspector when the selected agent changes | You are with another agent now. This and its tab's flash are the only teal in the panels |
| A lit tab | A panel behind another carries its light on its tab: lamplight rising from the tab's foot, or a teal flash | Something happened in a panel you are not looking at |
| A badge | A small amber dot burns on the tab of a panel behind another while its lamp is lit | Something waits for you there |
| Amber words | Status words are lamp amber | Live, or waiting |
| A speaker's paint | In the chat, a speaker's name and disc are in the paint of its robot | Who said it. The world's colours mark speakers and nothing else |
| An icon | A small line drawing before a word, in the word's own tone | What the word is, found sooner |

## Where it lives

In `hamlet_ui/src/`:

- `theme.rs` holds the theme, whose values are in `assets/theme.toml`.
  The panels are built only once that file has loaded; one not found or
  not readable gives way to the copy built into the app. A test holds
  every text tone to a contrast of 4.5:1 or better on every surface it
  meets.
- `looks.rs` holds the looks of each kind of button and field.
- `widgets/feel/` holds the behaviour: a control's warmth, press, flame and
  sparks, the light that falls into a panel and the light its tab
  carries while it is behind another. Its tuning is
  `FeelSettings`, from `assets/ui.toml`.

## Seeing it

`cargo run --example ui_gallery -- --scene controls --out <dir>` draws
every control in each state. The scene `waiting` shows Needs You with
questions, an approval and an edit waiting.
