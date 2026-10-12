# The chief in the world

The human a hamlet belongs to has a presence of their own: a place, a
mascot that is them, and a magic that carries the mascot to whoever they
talk to. The chief is a wizard, so the house wears a pointed hat, the
mascot carries a bare flame, and its travel is a spell.

The world's side is `hamlet_world/src/chief/`. The panel and the kept look
are `hamlet_ui/src/chief.rs`. `cargo run --example chief_sheet -- --out
<dir>` draws all of it to stills.

## The Hearth

A chief's place is a **Hearth**: a round, plump house under a roof like a
wide pointed hat, in a walled garden. It is the house on the app's icon.
No cottage or hall has a round wall or a curved roof, so its outline is
its own.

The garden holds a waystone, where the mascot leaves from and comes home
to, a letterbox, a flower bed, a bench under a tree, a gate and an orb on
the roof's tip. The roof takes the colour of the chief's cloak, and the
orb and the waystone's ring the colour of their light.

It belongs to a chief and not to a hamlet. It answers the pointer as
`Thing::ChiefPlace`, and selecting it shows the chief in the Inspector.
`chief::place::stand` is the one place that decides where it stands.

## The mascot

The chief in the world is a small cloaked person with a round face and a
**wick**, a bare flame that floats over its head. The agents are brass
robots whose light is shut in a lantern. The chief is soft, has a face,
and carries its light bare.

At home it goes about its grounds on foot: it stands at the waystone,
reads at the letterbox, tends the flower bed and looks at the sky from the
bench. Beside an agent it stands turned partly to the camera and listens.
When a hamlet's common is selected it steps there and looks it over.

Each motion is a pure function of the scene's clock (`mascot::Pose::of`),
eased into from the pose before it.

## Wickstep

The magic is called **Wickstep**. The mascot goes out like a candle where
it stands and is lit again somewhere else. Agents are bound to roads; the
chief is not, and this is how that looks.

When the chat is pointed at an agent and that agent is selected, the
mascot goes to it. When nobody is selected any more it goes home. The
whole step takes about a second: a circle of runes closes where it stood,
it is drawn up thin into its wick, and beside the agent a circle springs
open and it pours down out of the flame.

It is only something drawn, and never touches the keyboard or the pointer.
With the scene's clock frozen, the mascot is simply there.

## The look

Four choices: the cloak, the light, what is on the head and what is in the
hand. The first of each is what a chief has until they choose. They are
changed in the Inspector, and a press changes the mascot and the house in
that frame.

The look is kept in `chiefs/<name>.toml` in the home, one file per chief:

```toml
cloak = "river"
light = "candle"
hat = "pointed"
holds = "quill"
```

It is not in `view.toml`, because that is how one window on one machine is
laid out, and a look belongs to a person.

## Settings

`ChiefSettings` in `hamlet_world/src/chief.rs`, from `assets/world.toml`.
The colours are in the world's palette.
