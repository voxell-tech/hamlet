# The chief in the world

The human a realm belongs to has a presence of their own: a place, a mascot
that is them, and a magic that carries the mascot to whoever they talk to. The
chief is a wizard, so the house wears a pointed hat, the mascot carries a bare
flame, and its travel is a spell.

The world's side is `hamlet_realm/src/chief/`. The panel and the kept look are
`hamlet_app/src/chief.rs`. `cargo run --example chief_sheet -- --out <dir>`
draws all of it to stills.

## The Hearth

A chief's place is a **Hearth**: a round, plump house under a roof like a wide
pointed hat, in a walled garden. It belongs to a chief and not to a hamlet,
and stands on the city island ([`islands.md`](islands.md)). It answers the
pointer as `Thing::ChiefPlace`, and `chief::place::stand` is the one place
that decides where it stands.

## The mascot

A small cloaked person with a round face and a **wick**, a bare flame over
its head. Each motion is a pure function of the scene's clock
(`mascot::Pose::of`), eased into from the pose before it.

## Wickstep

The magic is called **Wickstep**. The mascot goes out like a candle where it
stands and is lighted again somewhere else: to the agent the chat is pointed
at when it is selected, and home when nobody is. It is only drawn, and never
touches the keyboard or the pointer.

## The look

The cloak, the light, the hat and what is held are chosen on the chief's
page in the Hub, and kept in `chiefs/<name>.toml` in the window's folder, one file per
chief, not in `view.toml`, which is how one window on one machine is laid out:

```toml
cloak = "river"
light = "candle"
hat = "pointed"
holds = "quill"
```

`ChiefSettings` in `hamlet_realm/src/chief.rs` is read from
`assets/world.toml`.
