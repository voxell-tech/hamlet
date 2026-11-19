# The warmth of the panels

How the panels around the world glow and answer the hand.

## The idea

The panels are a dark room at night, and the world is its window. Nothing in
the room gives light but small flames: a control warms like a lamp being
turned up, and a panel that needs you has a lamp burning over it. The room
stays dark so that the world stays the bright thing on screen.

## Rules

- Soot, wood and brass (the panels' surfaces) never glow. Only a control
  the pointer is on, or one being typed in, holds a pool of amber light.
- The one good action of a screen is the only control glowing at rest.
- A refusing button glows red-brown, not amber. A solid button is filled with
  the hue of what it does: green restores or allows, purple merges, yellow
  tries again, red refuses.
- Teal is only a breath across the top of the Hub when it shows something
  else, and a tab's flash when something happened in a panel behind another.
  Nothing else in the panels is teal.
- A tab behind another carries its light and, while something waits in it, a
  badge with the count.
- An agent's or hamlet's state is an icon alone, its word under the pointer
  ([`icons.md`](icons.md)). A status pill, mark or solid button is in one hue
  with an icon.
- In the chat, a speaker's name and disc are in the paint of its robot. The
  world's colours mark speakers and nothing else.

## Where it lives

In `hamlet_ui/src/`:

- `theme.rs` holds the theme: one variant's palette, from a family under
  `assets/themes/`, in the sizes, spacing and motion of
  `assets/measures.toml`. A family's file holds `base0` to `base8`, a ramp
  from the background end to the text end, and `red`, `orange`, `yellow`,
  `green`, `blue` and `purple`, with an optional `accent` naming the hue of
  the brand colour. `theme/derive.rs` makes every role from them, and moves
  each ink in lightness until it reads on its grounds;
  `theme/contrast.rs` measures this for every shipped theme.
- `hamlet_app/src/appearance.rs` picks the theme from the chief's appearance
  and the computer's color scheme.
- `styles.rs` holds the styles of each kind of button and field.
- `widgets/effects/` holds the behaviour: a control's warmth, press, flame
  and sparks, and the light that falls into a panel. Its tuning is
  `EffectSettings`, from `assets/effects.toml`.

## Seeing it

`cargo run --example ui_gallery -- --scene controls --out <dir>` draws every
control in each state. The scene `waiting` shows the Hub with questions, an
approval and an edit waiting.
