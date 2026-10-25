# How the robots move

The robots' motions, how one eases into the next, what a robot costs and
how robots differ. The code is `hamlet_realm/src/keeper/`. Everything is
procedural: no models, textures or animation files.

See it with `cargo run --example style_test`, then press N to step through
the sets.

## How it works

A motion is a pure function: `pose(act, time) -> Pose`. A `Pose` is about
thirty numbers, one for each thing the rig can move. Nothing is simulated
from frame to frame, so any moment can be drawn without the one before it.
A still at a frozen clock time is the same on every run, and a robot that
was off screen for an hour costs nothing to catch up.

The rig moves the whole figure, the hips and body, the head, hat and eyes,
two arms, two legs that can draw up into the body, a pair of wheels at
each foot, the crook with its lantern, one prop in the free hand, and the
lights.

Two rules keep it honest. A foot never goes below the ground: when the
hips drop, the leg draws up into the body. And the crook passes through
the hand: the hand is placed by the arm, and the crook from the hand.

## The motions

`Act` in `acts.rs` names them, and `acts/` holds them by occasion. Some
loop until told otherwise and the others play once.

The colour of the lantern says what the agent is doing: candle at rest,
cyan working, orange waiting for the chief, red fault, moon reflecting.

| Occasion | Motions |
|---|---|
| At rest | `Idle`, `Sleep`, `Reflect` |
| On the road | `Walk`, `Carry`, `HandOver`, `Receive` |
| Working | `Talk`, `Listen`, `Think`, `Hammer`, `Lift` |
| Waiting for the chief | `Wait`, `Object` |
| Gone wrong | `Fault`, `Failed` |
| Coming and going | `Appear`, `Leave`, `MergeInto`, `MergeHost` |
| The hamlet's life | `Celebrate`, `Vote` |
| Under the chief's hand | `Startle`, `Sulk`, `Wiggle`, `Dangle`, `Land`, `Greet`, `Wave` |

The motions under the chief's hand mean nothing about the agent's work.
Each plays as a `Passage`, so the robot's chore waits and is taken up
again afterwards.

## Rolling

A robot moved faster than `Motion::roll_above` stops stepping and rolls.
This is part of the walks, not a motion of its own, so whoever moves the
robot only says how far it has come and how fast. The wheels turn by the
ground covered over their radius, so they never slip.

## Blending

When a motion starts, the robot keeps the pose it was in and eases from it
to the new motion over 0.45 s, with a small dip of the hips halfway. Some
changes have their own timing: a fault lands in 0.14 s because it is a
knock, and `Appear` does not blend at all. A change in the middle of a
change works, since the start is whatever was drawn.

Anticipation lives in the motions themselves: a crouch comes before each
hop, and the mallet rises slowly and falls fast.

## Many robots

Each robot has a seed. From it come a clock offset, a tempo, and how much
it bounces and sways, so no two blink, breathe or step together. Seed 0 is
the plain robot, used for stills.

A robot costs about 0.4 microseconds of CPU a frame, 29 entities and 3,800
triangles. Every robot uses the same meshes and materials from one `Kit`,
so they batch. Its material and light are written only when the colour
changes.

## Telling robots apart

`Who` holds a robot's hat, paint, emblem, size and seed, and
`Who::agent(seed)` picks all of them from the seed.

- **Hat** is the main one, because it changes the outline and reads at 40
  pixels.
- **Paint** is matte and is never cyan, orange, red or candle. Colour that
  means something stays in the light.
- **Emblem** on the chest plate is only readable close up.

**The mayor** (`Who::mayor()`) is the one robot with the brass dome hat,
a pennant, a cream mantle and a larger size. Cream is kept for the mayor.

## Wearing a look

A robot with a `Wears` wears a skin, a mesh and a clip (`wear.rs`,
[`looks.md`](looks.md)). A skin swaps the materials of its parts for ones
shared by every robot in that skin; a mesh swaps the meshes of the parts
it names and keeps their joints; a clip adds turns and shifts to the pose
of eight parts before it is drawn. An agent's robot carries a `Wearer`,
its agent's name, and wears what the agent wears.

## Using it

```rust
let robot = spawn_agent(&mut commands, &kit, &mut materials,
    Who::agent(7), at, Some(clock.t)); // Some: it plays Appear
keeper.play(Act::Think, clock.t);
commands.entity(robot).insert(Trip::new(road_points, clock.t, &motion));
```

- A `Trip` moves the robot along a line of points, with steps timed to the
  ground covered so the feet do not slide. Play `Walk` or `Carry` with it.
- A letter: both robots face each other `TALK_GAP` apart, then `HandOver`
  on one and `Receive` on the other at the same clock time.
- A merge: `MERGE_GAP` apart, `MergeInto` and `MergeHost` together.

## Houses coming in and going out

The code is `generator/shift.rs`, tuned by `HouseMotion`. A house is one
mesh, and only its transform and visibility move. Like a robot's motion,
each part is a pure function of the time since it began.

- **In.** The house stands up out of the ground with a small overshoot,
  its furniture pops up after it, and the robot plays `Appear` on the
  porch.
- **Out.** The house waits for its robot's `Leave` or `MergeInto`, then
  shudders and falls flat, and what it rests as stands up under it.
- **A house that moves town** sinks on its old plot and rises on the new.

What a house rests as is decided in one place: `Agent::remains` in
`society.rs`, read from `HouseSettings`.

A house that is there when its town is first drawn does not move, which
covers everything at load.
