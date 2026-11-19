# How the robots move

The robots' motions, how one eases into the next and how robots differ. The
code is `hamlet_realm/src/keeper/`. Everything is procedural: no models,
textures or animation files.

See it with `cargo run --example style_test`, then press N to step through the
sets.

## How it works

A motion is a pure function: `pose(act, time) -> Pose`. Nothing is simulated
from frame to frame, so any moment can be drawn without the one before it: a
still at a frozen clock time is the same on every run, and a robot that was
off screen for an hour costs nothing to catch up. A foot never goes below the
ground: when the hips drop, the leg draws up into the body.

## The motions

`Act` in `acts.rs` names them, and `acts/` holds them by occasion. Some loop
until told otherwise and the others play once. The chores at home are in
[`working-alone.md`](working-alone.md).

The colour of the lantern says what the agent is doing: candle at rest, cyan
working, orange waiting for the chief, red fault, moon reflecting.

The motions under the chief's hand (`Startle`, `Sulk`, `Wiggle`, `Dangle`,
`Land`, `Greet`, `Wave`) mean nothing about the agent's work. Each plays as a
`Passage`, so the robot's chore waits and is taken up again afterwards.

A robot moved faster than `Motion::roll_above` rolls instead of stepping.
When a motion starts, the robot eases from the pose it was in; `Appear` does
not blend.

## Many robots

Each robot has a seed, from which come a clock offset and a tempo, so no two
step together. Seed 0 is the plain robot, used for stills. Every robot
uses the same meshes and materials from one `Kit`, so they batch.

`Who::agent(seed)` picks a robot's hat, paint, emblem and size from the seed.
Paint is matte and never cyan, orange, red or candle: colour that means
something stays in the light. The lead (`Who::lead()`) is the one robot with
the brass dome hat and a cream mantle.

## Using it

```rust
let robot = spawn_agent(&mut commands, &kit, &mut materials,
    Who::agent(7), at, Some(clock.t)); // Some: it plays Appear
keeper.play(Act::Think, clock.t);
commands.entity(robot).insert(Trip::new(road_points, clock.t, &motion));
```

- A `Trip` moves the robot along a line of points, with steps timed to the
  ground covered. Play `Walk` or `Carry` with it.
- A letter: both robots face each other `TALK_GAP` apart, then `HandOver` on
  one and `Receive` on the other at the same clock time. A merge is the same
  with `MERGE_GAP`, `MergeInto` and `MergeHost`.

Houses coming in and going out are `generator/shift.rs`. A house waits for
its robot's `Leave` or `MergeInto` before it falls (`Agent::remains`).
