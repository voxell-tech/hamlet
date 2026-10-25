# Looks: what agents and hamlets wear

Agents and hamlets wear looks: an **emote**, a **skin** and a **mesh**
each, at most one of each kind. Each is an official asset of the Agent
Bazaar's server, named by its id and version
(`agent-skin-sakura@1.0.0`), with a file. The Hamlet team makes them,
defines them in code in the private repository
`voxell-tech/private-hamlet-assets`, and sells them at the bazaar's
sixth stall, LOOKS.

## Where they live

| Part | Where |
|---|---|
| What each agent and hamlet wears, kept in the hamlet's record and checked by form | `hamlet_server/src/looks.rs`; `record::Hamlet::looks` and `worn`, by agent number |
| The `looks` edit and `Looks` in the state | `hamlet_protocol/src/client/looks.rs`; [`protocol.md`](protocol.md#looks) |
| What the window received each wears | `hamlet_client::realm::Outfit`, on the agent's and the hamlet's entities |
| The formats below | `hamlet_realm/src/wardrobe.rs` |
| What is worn by name, the files of what is worn in memory, and `PlayEmote` | `hamlet_realm/src/dress.rs` (`Looks`, `Worn`), filled by `hamlet/wearing.rs` and the bazaar's closet |
| The files read for the world; meshes handed to the asset server from memory; a `.glb` writer | `hamlet_realm/src/dress.rs` (`Dressing`), `dress/glb.rs` |
| What a hamlet's houses wear, and the hamlet each town is | `hamlet_realm/src/generator/attire.rs`, `hamlet/wearing.rs` |
| What an agent's robot wears, and the agent each robot is | `hamlet_realm/src/keeper/wear.rs`, `hamlet/wearer.rs` |
| The LOOKS stall, the fitting and the files downloaded | `hamlet_app/src/screens/bazaar/wardrobe.rs`, `wardrobe/`, `view/wardrobe.rs`, `view/wardrobe/` |
| A look shown in the viewer, worn by a robot or a house | `hamlet_realm/src/viewer/dummy.rs` (`Dummy`); fetched by `hamlet_app/src/screens/bazaar/wardrobe/viewing.rs` |
| The costumier's booth in the world | `hamlet_realm/src/bazaar/build/stall/costumier.rs` |
| The baker | `voxell-tech/private-hamlet-assets`, private |

What is worn belongs to the home: the server keeps it in each hamlet's
record, and every window of the chief shows the same once it has the
files. The server checks only that an asset's id fits the part and the
wearer, `<agent|hamlet>-<skin|mesh|emote>-<name>`; it cannot see what the
chief owns at the bazaar's server, so the window offers only what is
owned.

In the window, `hamlet/wearing.rs` fills the resource `Looks` from each
entity's `Outfit`: in `agents` by the agent's name, in `hamlets` by the
hamlet's id, the `id@version` worn of each kind (`Worn`). The bazaar's
closet puts the file of each in `files`, by that name, kept in memory
only. The realm reads the file of a name once, when `Looks` changes, into
its `Dressing`; one that cannot be read is named in the log and not worn.
A worn mesh reaches Bevy's asset server through the `wardrobe://` source,
which the window registers before the asset plugin
(`dress::register_source`). Nothing is worn while a file is missing, and
the world is drawn as it always is.

`cargo run --example chief_sheet -- --looks` draws a hamlet and an agent
in their looks, by day and by night, and an emote played.

## Skins

JSON. Colours of the world's palette worn in place of its own, as
`"#rrggbb"` in sRGB, and for an agent, how its paint takes the light.

```json
{ "format": 1,
  "colours": { "paint": "#f4b6c2", "brass": "#e9d8a6" },
  "finish": { "metallic": 0.0, "roughness": 0.5, "glow": 0.0 } }
```

- An agent's slots: `paint`, `brass`, `steel`, `glass`, `lamp`.
- A hamlet's slots: the palette fields its houses are built of
  (`plaster`, `plaster_worn`, `whitewash`, `ochre`, `limestone`,
  `limestone_shade`, `timber`, `timber_dark`, `terracotta`, `slate_roof`,
  `moss`), its town's `roof`, `drift` and `banner`, and `livery` for every
  door and shutter.

## Emotes

JSON, `kind` saying which.

An agent's `clip`: poses of its parts at times, eased between with a
smoothstep and added to whatever the agent is doing. It plays now and then
while the agent is idle, at most every `every` seconds, and when asked.

```json
{ "format": 1, "kind": "clip", "seconds": 2.4, "every": 45,
  "keys": [
    { "at": 0.0, "parts": {} },
    { "at": 0.6, "parts": { "arm_r": { "turn": [0, 0, 150] } } },
    { "at": 2.4, "parts": {} } ] }
```

Parts: `rig`, `body`, `head`, `hat`, `arm_l`, `arm_r`, `leg_l`, `leg_r`.
`turn` is degrees about X, Y and Z in turn; `shift` is a move along them.

A hamlet's emotes move about all its houses:

- `smoke`: `colour`, `rate` (puffs a second), `size`, `rise`. Puffs leave
  each chimney top, grow, drift and fade as they rise `rise` high. At most
  `[attire] puffs` are in the air over one chimney.
- `lanterns`: `paper`, `light`, `count` (per house), `sway` (degrees).
  Paper lanterns hang in a row under each house's front eave, at most
  `[attire] lanterns` of them, swinging `sway` degrees either way. Their
  paper glows `light` after dark, as the windows do.
- `banners`: `colours` (the cloth and its stripe), `long`. Every house
  flies a banner of the mayor's kind from a mast on its roof, at its
  `banner` spot; the mayor's own banner takes these colours too.
- `petals`: `colours` (half the petals in each), `count` (per house),
  `size`, `fall` (seconds from the eave to the ground). Petals drift down
  from along each front eave, swinging as they fall, at most
  `[attire] petals` to a house.
- `fireflies`: `colour`, `count` (per house), `glow`. Fireflies wander in
  loops about each house, out to `[attire] firefly_reach` past its eaves,
  at most `[attire] fireflies` to a house; they glow `glow` times their
  colour after dark, as the windows do.
- `pinwheels`: `colours` (the sails, in turn), `count` (per house),
  `speed` (turns a second). Pinwheels stand on sticks along each front
  eave, at most `[attire] pinwheels` to a house, turning clockwise.

What an emote moves, it moves only while it is seen, and keeps frames
coming only then. Their sizes and speeds are `[attire]` in `assets/world.toml`.

## Meshes

glTF binary (`.glb`). Meshes are found by the names of their glTF meshes,
and spots by the names of empty nodes. Vertex colours (`COLOR_0`) are the
colour, drawn with the world's shared matte material; a skin is still worn
over a mesh where its slots apply.

## How a hamlet wears them

Everything a hamlet wears is worn by every house of its town, near and
far. The code is `hamlet_realm/src/generator/attire.rs`; what each town
wears is found in `Dressing` by the hamlet's id in
`hamlet_realm/src/hamlet/wearing.rs`. Putting something on or taking it
off builds the town's houses anew.

- **A skin** is a copy of the palette with its slots replaced: `roof`,
  `drift` and `banner` in every town style, `livery` for every door and
  shutter. The houses, their grounds and the far town are built in it.
- **A mesh** stands in for each house's walls, roof, windows and chimney.
  The path, the garden, the yard, the door lantern's bracket and a banner's
  mast are still the house's own.
- **An emote** hangs about each house that is lived in, as above.

A house can be raised on its own, without `Looks`, by
`generator::attire::raise(world, &attire, hall, at)`, with an `Attire`
made by hand and a worn mesh read by `Shell::of`.

## Hamlet meshes

A hamlet's `.glb` holds:

| Name | Kind | What it is |
|---|---|---|
| `walls` | mesh | The house, vertex-coloured. Needed. |
| `panes` | mesh | The window glass, drawn with the world's window material, which burns at night, and casting no shadow. |
| `door` | empty node | The foot of the front door. |
| `lamp` | empty node | The middle of the door lantern's glass. |
| `banner` | empty node | The foot of a banner's mast. |
| `chimney` | empty node | The chimney's top, where smoke leaves. |
| `eaves` | empty node | The middle of the front eave, which lanterns hang under. |

The frame it is authored in:

- Units are the world's: a door is about 1 tall, and the eave of a house
  of one storey about 1.9 over the ground.
- The ground is y 0 at the house's middle. Ground may slope, so carry the
  walls a little below 0.
- The front faces +Z, and the door is on the front.
- The footprint is 1 from the middle each way: walls from x -1 to 1 and z
  -1 to 1. Each house stretches it in x and z to its own footprint (from
  about 1.25 x 1.05 to 1.7 x 1.3, the mayor's hall 2 x 1.4) and keeps its
  height. A mesh is the whole house: storeys are its own, and the hall
  wears it too, without its tower. Merged houses add no wings to it.
- Meshes are read in their own coordinates; their nodes' transforms are
  not applied. Spots are their nodes' translations.

`generator::house::plain(&land)` is the world's own house in this frame,
and `Plain::glb` writes it as a hamlet mesh, which makes a start.

What is left out falls back:

- no `panes`: no windows.
- no `door`: the middle of the front of the walls' bounds, on the ground.
- no `lamp`: 0.6 aside, 1 up and 0.3 out from the door.
- no `banner` or `chimney`: the top of the walls' bounds over the middle.
- no `eaves`: 0.5 over the lamp at the front of the walls' bounds.

A skin does not recolour a mesh's own vertex colours, but does the
grounds about it and the town seen from far. From far a house that wears
a mesh is a box the size of its walls' bounds, its sides their mean colour
and its top the town's roof colour, which a skin sets. A ruin is the
house's own ruin, whatever is worn.

## How an agent wears them

An agent's robot wears what is in its `Wears` component: a skin, a mesh
and a clip. The code is `hamlet_realm/src/keeper/wear.rs`. Each robot of
an agent carries a `Wearer`, the agent's `Name`, kept by
`hamlet_realm/src/hamlet/wearer.rs`; from it and `Dressing` its `Wears`
is kept in step. Taking a look off puts the robot's own back.

- **A skin** recolours the parts made of each slot. Its materials are made
  once per skin and slot and shared by every robot wearing it.
  - `paint`: the painted torso and hat. `finish` sets their `metallic`
    and `roughness`, and `glow` makes them glow of their colour, 1 as
    bright as a lit lantern. A finish without a `paint` keeps each
    robot's own paint.
  - `brass`: every brass fitting: the rings, joints, head cage, wheels,
    key, hat trims, crook fittings and lamp frame.
  - `steel`: the arms, legs and crook.
  - `glass`: the head's glass and the carried lantern's, unlit. Lit, it
    pales toward white.
  - `lamp`: the light's colour at rest, in place of the candle. The
    colours that tell the agent's state stay.
- **A mesh** stands in for the parts it names, below. The joints stay, so
  every motion still moves them. Parts it does not name keep their own
  meshes and the skin.
- **A clip** plays on top of the robot's motion, eased in and out over
  `[motion] emote_blend` seconds in `assets/world.toml`:
  - while the robot is idle, first after a wait of up to `every` seconds
    that differs from robot to robot, then again `every` to
    `every * (1 + emote_spread)` seconds after it last began. An `every`
    shorter than the clip counts as its length. Never on a frozen clock.
  - at once, whatever the robot is doing, when
    `dress::PlayEmote(name)` is triggered, for the
    robots of every agent so named.

  A clip's `turn` is applied after the part's pose, about its own axes:
  X, then Y, then Z, each about the axes the turn before left
  (`EulerRot::XYZ`). `shift` moves the part's joint in its parent's frame:
  `rig` in the units of the world, the others in a robot's units below.

A robot can wear a look on its own, without `Looks`:
`keeper::spawn_dressed(commands, kit, materials, who, at, wears)`, or
insert a `Wears` on any robot. `Wears::of(&look)` makes one from a
`dress::Look`. `Gesture::play(clock_time)` on the robot plays its clip
from that clock time.

## Agent meshes

An agent's `.glb` holds a mesh for each part it replaces, named below.
Only its first primitive is read, in its own coordinates: nodes'
transforms are not applied. A part with a side, such as `arm_l`, is found
by its own name first and then by the name both sides share, `arm`.
`keeper::MESH_PARTS` lists every name.

A replacement is drawn with the world's shared matte material in its
vertex colours, and the skin does not recolour it; `glass` and
`lamp_glass` are drawn in the robot's light instead, which the vertex
colours tint. Where a part is made of two meshes, paint or wood and
brass, the replacement stands in for both.

The robot faces +Z with Y up. `_l` is the side at -X, `_r` the side at
+X. Units are those of a robot of size 1, about 1 tall to the top of its
head; each
robot is scaled by its size, 0.84 to 1.02, the mayor 1.12. Each part is
in its joint's frame, which the motion moves; the joint's place is given
in its parent's frame. Sizes are the default meshes' bounds, the mayor's.

| Name | Joint, in its parent | Default bounds, min to max |
|---|---|---|
| `torso` | the hips, (0, 0.3, 0) over the feet | (-0.25, 0, -0.25) to (0.25, 0.48, 0.25) |
| `key` | (0, 0.25, 0) in the torso; it spins about Z | (-0.02, -0.08, -0.5) to (0.02, 0.08, -0.2) |
| `emblem` | the torso's own | (-0.02, 0.17, 0.24) to (0.02, 0.24, 0.26) |
| `mantle` | the torso's own; the mayor only | (-0.28, 0.32, -0.27) to (0.28, 0.45, 0.27) |
| `arm`, `arm_l`, `arm_r` | the shoulder, (-0.27 or 0.27, 0.35, 0) in the torso; the hand is at y -0.3 | (-0.06, -0.36, -0.06) to (0.06, 0.06, 0.06) |
| `leg`, `leg_l`, `leg_r` | the hip, (-0.11 or 0.11, 0.3, 0) over the feet; the sole is at y -0.3 | (-0.09, -0.31, -0.09) to (0.09, 0.04, 0.16) |
| `wheels`, `wheels_l`, `wheels_r` | the axle, (0, -0.23 to -0.26, 0.035) in the leg; it turns about X | (-0.11, -0.06, -0.06) to (0.11, 0.06, 0.06) |
| `head` | the neck, (0, 0.44, 0) in the torso | (-0.17, 0.03, -0.17) to (0.17, 0.28, 0.17) |
| `glass` | (0, 0.17, 0) in the head | (-0.14, -0.11, -0.13) to (0.14, 0.11, 0.13) |
| `eye`, `eye_l`, `eye_r` | (0, 0.17, 0) in the head, turned 22 degrees about Y to its side and squashed in Y to blink | (-0.02, -0.02, 0.13) to (0.02, 0.05, 0.15) |
| `hat` | (0, 0.28, 0) in the head | (-0.19, -0.02, -0.19) to (0.19, 0.46, 0.24) |
| `pennant` | (0, 0.42, 0) in the hat; the mayor only | (0, -0.13, -0.27) to (0, 0, 0) |
| `pole` | the crook's own, 0.42 over its foot; the right hand holds it along Y | (-0.03, -0.42, -0.03) to (0.03, 1.2, 0.21) |
| `lamp` | the hook, (0, 1.14, 0.2) in the crook | (-0.1, -0.37, -0.1) to (0.1, 0, 0.1) |
| `lamp_glass` | (0, -0.26, 0) in the lamp | (-0.07, -0.08, -0.07) to (0.07, 0.08, 0.07) |

`keeper::default_parts(&palette, who)` gives the meshes a robot has by
default, by the names both sides share, in these frames and coloured as
drawn. Built on with `meshes::Meshing` and written with `dress::Glb`, they make
a start.

## The costumier

The bazaar's sixth stall, LOOKS, is Madame Perpetua's costumier's booth:
a rail of robes, a dress form, bolts of cloth and pots of paint, a
looking glass, and a little robot turning on a turntable. Clicked, it
opens the panel at the stall, and a sale there is cheered as at every
stall.

## Buying and wearing them

LOOKS is laid out as every stall of the panel is ([`bazaar.md`](bazaar.md)):
its shelf, with FITTING under its head, and at its foot SIGN IN to the
bazaar's server, or SIGN OUT beside who is signed in.

- **The shelf** is the server's catalogue (`GET /v1/assets`), asked for
  the first time the stall is shown and again on LOOK AGAIN. Beside its
  order, by kind or by price, it is narrowed by what wears an asset and
  by its kind. A row is its title and price, its summary and what it is
  (Skin for agents), and You own this or Worn in the accent.
- **An asset's page** has its title over its summary, what it is and its
  version; the asset in the viewer, with RESET and BIGGER or SMALLER
  under it; then its price and BUY FOR, GET IT FOR FREE or SIGN IN while
  it is not the chief's, or once it is, Goes on, the choice of who wears
  it, and WEAR IT; then WORN BY, each who wears it with TAKE OFF, and
  how its file stands.
- **The viewer.** Opening an asset's page fetches its file veiled
  (`GET /v1/assets/:id/view`, signed in or not) and unveils it in memory
  (`wardrobe/viewing.rs`); nothing is written to disk. The realm's own
  robot, or a house for a hamlet's look, wears it on a round stand
  (`viewer::Dummy`), read through `dress::try_on` as worn looks are, a
  mesh through the `wardrobe://` source under its own name. A skin or a
  mesh is seen worn; an agent's clip plays over and over; a hamlet's emote
  hangs about the house. Dragging turns it, the wheel brings it nearer,
  and left alone it turns by itself; the tuning is `[viewer]` in
  `assets/world.toml`. Leaving the page lets the file go.
- **Buying.** BUY FOR, or GET IT FOR FREE, asks the server
  (`POST /v1/assets/:id/purchase`): a free asset, or one owned already
  (`409`), is the chief's at once; a paid one opens Stripe's page and is
  asked after (`GET /v1/asset-purchases/:id`) every five seconds for half
  an hour, by the market's own steps (`pay` in `market/deal.rs`). Once it
  is the chief's, the realm cheers and what is owned is asked again
  (`GET /v1/me/assets`), as it is when a chief signs in; signing out
  forgets it.
- **Wearing.** WEAR IT, TAKE OFF and the fitting's dropdowns send a
  `looks` edit for the one part they change. Only an owned asset of the
  wearer's target and kind is put on. An emote put on an agent is played
  at once.
- **The fitting** lists every living agent and every hamlet, a row each
  of its name, Agent or Hamlet, and what it wears. One opened has its
  name over what it wears, PLAY THE EMOTE for an agent wearing an emote,
  SHOP FOR LOOKS, which shows the shelf narrowed to what it wears, and
  WEARS: Emote, Skin and Mesh, each with a dropdown of None and the owned
  assets that fit. An agent's page and a hamlet's in the Inspector have
  CHANGE LOOKS, which opens the fitting at it.
- **The files.** The closet (`wardrobe/closet.rs`) downloads the file of
  each look worn whose asset the chief owns, one at a time, 512 KiB a
  request (`GET /v1/assets/:id/file` with `offset` and `limit`), and
  puts it whole in the realm's `Looks` under the name worn. One that did
  not come is asked for again 30 seconds later. A file no one wears is
  let go of. The file sent is the asset's newest, whatever version is
  named worn.

`cargo run --example ui_gallery -- --scene looks` (and `looks-asset`,
`looks-asset-bigger`, `looks-asset-emote`, `looks-asset-mesh`, `looks-asset-owned`,
`looks-fitting`, `looks-fitting-one`) draws the stall, an asset's page
and the fitting; the files viewed are given by hand.

## Baking the official assets

The official assets are not in this repository, so a fork does not have
them. They are defined in code in the private repository
`voxell-tech/private-hamlet-assets`, which bakes each look's file with
`hamlet_realm`'s own meshes (`meshes::Meshing`, written with
`dress::Glb`) and the formats above, and loads them into the bazaar's
server with `cargo populate`. Its `docs/making-looks.md` is the guide
for modellers.
