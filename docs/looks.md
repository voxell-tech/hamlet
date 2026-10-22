# Looks: what agents and hamlets wear

Agents and hamlets wear looks: an **emote**, a **skin** and a **mesh**
each, at most one of each kind. Each is an asset with an id and a file.
The official assets are sold at the Agent Bazaar's costumier, the stall
LOOKS, and are made by the Hamlet team.

## Where they live

| Part | Where |
|---|---|
| The formats below, and how the bazaar tells the realm what is worn | `plugins/looks/` (`looks`, `looks::wardrobe`), a library and no plugin of its own |
| The reading of what is worn from the store | `plugins/realm/src/wardrobe/closet.rs` (`Closet`) |
| A worn `.glb`, read inside the plugin | `plugins/realm/src/wardrobe/glb.rs` (`Glb`) |
| The files worn, read for what wears them, and the looks of each agent and town | `plugins/realm/src/wardrobe.rs` (`Wardrobe`) |
| What a robot wears | `plugins/realm/src/folk/keeper/wear.rs`, put on in `rig.rs` |
| What a hamlet's houses wear | `plugins/realm/src/places/town/attire.rs`, built in `places/house.rs` (`dressed`) and `places/town/build.rs` |
| How the realm reads, plays and moves them | `plugins/realm/assets/looks.toml` |
| The official assets, defined in code, and their preview pictures | `hamlet-server/assets-src/` (`hamlet_assets_src`), below |

## How the realm finds what is worn

The bazaar keeps two types of asset in the engine's store
([`engine.md`](engine.md#assets)), and the realm reads both
(`assets.read` of `looks.worn` and `looks.file`, at version 1):

- `looks.worn`, one asset named `worn`: a `Worn` in JSON, what each
  agent wears by its name and each hamlet by its name on disk, as the
  file names of its skin, mesh and emote, and how many parts each file
  is in.
- `looks.file`, one asset per part of a file worn, named
  `<file>#<part>`, `looks::FILE_PART` bytes at most. A file is named by
  its asset's id and version, `agent-skin-sakura@1.0.0`, so a new
  version is a new file.

Once the realm is made, the closet lists `looks.worn`, holds `worn` so
the engine tells it of each change, and reads it again on each. It
lists `looks.file` for the parts of each file it does not have, reads
them, and joins them in their order. A file is read once; a file no
longer worn is let go, and read anew if it is worn again. A file whose
parts are not all in the store is given up, said in the log, and the
rest is worn. While the store holds no `worn`, or no plugin makes the
type, nothing is worn, the world is drawn as it always is, and the
store is listed again `[closet] relist` seconds later. The closet makes
at most `[closet] asks` requests a call and has at most `in_flight`
waiting.

When all of it is read, the realm reads each file for what wears it:
`Skin::read` and `Emote::read` for an agent or a hamlet, and each `.glb`
once. One that cannot be read is named in the log and not worn. Only
the agents and towns whose looks changed are dressed again; a look that
stays costs nothing a frame but what an emote moves.

The bazaar's event `hamlet.bazaar/emote` (payload `looks.emote`, the
agent's name as a `String`) has that agent's robot play its clip now.

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
smoothstep and added to whatever the agent is doing.

```json
{ "format": 1, "kind": "clip", "seconds": 2.4, "every": 45,
  "keys": [
    { "at": 0.0, "parts": {} },
    { "at": 0.6, "parts": { "arm_r": { "turn": [0, 0, 150] } } },
    { "at": 2.4, "parts": {} } ] }
```

Parts: `rig`, `body`, `head`, `hat`, `arm_l`, `arm_r`, `leg_l`, `leg_r`.
`turn` is degrees about X, Y and Z in turn; `shift` is a move along them.

A hamlet's emotes move about all its houses that are lived in:

- `smoke`: `colour`, `rate` (puffs a second), `size`, `rise`. Puffs leave
  each chimney top, grow, drift and fade as they rise `rise` high. At most
  `[attire] puffs` are in the air over one chimney.
- `lanterns`: `paper`, `light`, `count` (per house), `sway` (degrees).
  Paper lanterns hang in a row under each house's front eave, at most
  `[attire] lanterns` of them, swinging `sway` degrees either way. Their
  paper glows `light` after dark, as the windows do.
- `banners`: `colours` (the cloth and its stripe), `long`. Every house
  flies a banner of the leader's kind from a mast on its roof, at its
  `banner` spot; the leader's own banner takes these colours too.
- `petals`: `colours` (half the petals in each), `count` (per house),
  `size`, `fall` (seconds from the eave to the ground). Petals drift down
  from along each front eave, swinging as they fall, at most
  `[attire] petals` to a house.
- `fireflies`: `colour`, `count` (per house), `glow`. Fireflies wander in
  loops about each house, out to `[attire] firefly_reach` past its eaves,
  at most `[attire] fireflies` to a house; they glow `glow` times their
  colour after dark, and `[attire] firefly_day` of it by day.
- `pinwheels`: `colours` (the sails, in turn), `count` (per house),
  `speed` (turns a second). Pinwheels stand on sticks along each front
  eave, at most `[attire] pinwheels` to a house, turning clockwise.

## Meshes

glTF binary (`.glb`), read inside the plugin with the `gltf` crate.
Meshes are found by the names of their glTF meshes, every primitive of
triangles of one taken together, and spots by the names of empty nodes.
Meshes are read in their own coordinates; their nodes' transforms are
not applied, and a spot is its node's translation. Vertex colours
(`COLOR_0`) are the colour, white where there are none, drawn with the
world's shared matte material; normals left out are made from the
triangles. A file of more than 200 000 vertices is not worn.

## How a hamlet wears them

Everything a hamlet wears is worn by every house of its town, near and
far. Putting something on or taking it off builds that town's houses
anew, each drawn until its new one is, and its far mesh.

- **A skin** is a copy of the palette with its slots replaced: `roof`,
  `drift` and `banner` in every town style, `livery` for every door and
  shutter. The houses, their grounds and the far town are built in it.
- **A mesh** stands in for each house's walls, roof, windows and chimney,
  laid into the house's own mesh. The path, the garden, the yard, the
  door lantern's bracket and a banner's mast are still the house's own.
- **An emote** hangs about each house that is lived in. Its moving parts
  are copies of one mesh to a kind (`Instances`) under the town, moved on
  `tick` while the town is shown near and within `[detail] robots_far`,
  and given no copy otherwise. Banners are built with the houses, and a
  pinwheel's stick is laid again only when the houses change.

## Hamlet meshes

A hamlet's `.glb` holds:

| Name | Kind | What it is |
|---|---|---|
| `walls` | mesh | The house, vertex-coloured. Needed. |
| `panes` | mesh | The window glass, drawn with the world's window material, which burns at night. |
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
  about 1.25 x 1.05 to 1.7 x 1.3, the leader's hall 2 x 1.4) and keeps its
  height. A mesh is the whole house: storeys are its own, and the hall
  wears it too, without its tower. Merged houses add no wings to it.

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

Each robot of an agent wears what that agent wears, found by the agent's
name. A robot is drawn part by part, each part an entity the realm moves
itself (no skinned mesh), so a look changes what each part's entity
draws. Taking a look off puts the robot's own back.

- **A skin** recolours the parts drawn in each slot. Its materials are
  made once per skin and shared by every robot wearing it.
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
- **A mesh** stands in for the parts it names, below, each drawn on the
  entity of that part's first mesh, the others left empty. The joints
  stay, so every motion still moves them. Parts it does not name keep
  their own meshes and the skin. A robot wears it once all its meshes are
  handed over, within the realm's budgets.
- **A clip** plays on top of the robot's motion, eased in and out over
  `[wear] emote_blend` seconds:
  - while the robot is idle, first after a wait of up to `every` seconds
    that differs from robot to robot, then again `every` to
    `every * (1 + emote_spread)` seconds after it last began. An `every`
    shorter than the clip counts as its length.
  - at once, whatever the robot is doing, on the bazaar's
    `hamlet.bazaar/emote` for its agent.

  A clip's `turn` is applied after the part's pose, about its own axes:
  X, then Y, then Z, each about the axes the turn before left
  (`EulerRot::XYZ`). `shift` moves the part's joint in its parent's frame:
  `rig` in the units of the world, the others in a robot's units below.

## Agent meshes

An agent's `.glb` holds a mesh for each part it replaces, named below
(`wear::MESH_PARTS`). A part with a side, such as `arm_l`, is found by
its own name first and then by the name both sides share, `arm`.

A replacement is drawn with the world's shared matte material in its
vertex colours, and the skin does not recolour it; `glass` and
`lamp_glass` are drawn in the robot's light instead, which the vertex
colours tint. Where a part is made of two meshes, paint or wood and
brass, the replacement stands in for both.

The robot faces +Z with Y up. `_l` is the side at -X, `_r` the side at
+X. Units are those of a robot of size 1, about 1 tall to the top of its
head; each robot is scaled by its size, 0.84 to 1.02, the leader 1.12.
Each part is in its joint's frame, which the motion moves; the joint's
place is given in its parent's frame. Sizes are the default meshes'
bounds, the leader's.

| Name | Joint, in its parent | Default bounds, min to max |
|---|---|---|
| `torso` | the hips, (0, 0.3, 0) over the feet | (-0.25, 0, -0.25) to (0.25, 0.48, 0.25) |
| `key` | (0, 0.25, 0) in the torso; it spins about Z | (-0.02, -0.08, -0.5) to (0.02, 0.08, -0.2) |
| `emblem` | the torso's own | (-0.02, 0.17, 0.24) to (0.02, 0.24, 0.26) |
| `mantle` | the torso's own; the leader only | (-0.28, 0.32, -0.27) to (0.28, 0.45, 0.27) |
| `arm`, `arm_l`, `arm_r` | the shoulder, (-0.27 or 0.27, 0.35, 0) in the torso; the hand is at y -0.3 | (-0.06, -0.36, -0.06) to (0.06, 0.06, 0.06) |
| `leg`, `leg_l`, `leg_r` | the hip, (-0.11 or 0.11, 0.3, 0) over the feet; the sole is at y -0.3 | (-0.09, -0.31, -0.09) to (0.09, 0.04, 0.16) |
| `wheels`, `wheels_l`, `wheels_r` | the axle, (0, -0.23 to -0.26, 0.035) in the leg; it turns about X | (-0.11, -0.06, -0.06) to (0.11, 0.06, 0.06) |
| `head` | the neck, (0, 0.44, 0) in the torso | (-0.17, 0.03, -0.17) to (0.17, 0.28, 0.17) |
| `glass` | (0, 0.17, 0) in the head | (-0.14, -0.11, -0.13) to (0.14, 0.11, 0.13) |
| `eye`, `eye_l`, `eye_r` | (0, 0.17, 0) in the head, turned 22 degrees about Y to its side and squashed in Y to blink | (-0.02, -0.02, 0.13) to (0.02, 0.05, 0.15) |
| `hat` | (0, 0.28, 0) in the head | (-0.19, -0.02, -0.19) to (0.19, 0.46, 0.24) |
| `pennant` | (0, 0.42, 0) in the hat; the leader only | (0, -0.13, -0.27) to (0, 0, 0) |
| `pole` | the crook's own, 0.42 over its foot; the right hand holds it along Y | (-0.03, -0.42, -0.03) to (0.03, 1.2, 0.21) |
| `lamp` | the hook, (0, 1.14, 0.2) in the crook | (-0.1, -0.37, -0.1) to (0.1, 0, 0.1) |
| `lamp_glass` | (0, -0.26, 0) in the lamp | (-0.07, -0.08, -0.07) to (0.07, 0.08, 0.07) |

## Baking the official assets

The official assets are defined in code in `hamlet-server/assets-src/`:
the catalogue in `catalog.rs`, the skins in `skins.rs`, the emotes in
`emotes.rs`, and the meshes of agents and hamlets in `agents.rs` and
`hamlets.rs`, built with the realm's own mesh builder and written as
`.glb` by `glb.rs`. Raise `catalog::VERSION` when a file changes, so
owners are sent the new one.

```sh
cargo run --release -p hamlet_assets_src -- [--out DIR] [--only ID,ID] [--no-previews]
```

It writes into `DIR`, by default `hamlet-server/assets-src/out/`, which
git ignores: `files/<id>.json` or `files/<id>.glb`, `previews/<id>/<n>.png`
and `catalog.json`, the catalogue hamlet-server publishes, which lists how
many pictures each asset has.

Each asset has 24 pictures, PNGs of 512 by 512, each under the server's
400 KiB. A picture is the asset worn by the realm's own robot or house on
a plinth, made by the realm plugin's code: `plugins/realm/src/bake.rs`
gathers the builders that make meshes and looks without the engine, and
the baker puts them together as the realm does (`Rig::make` and
`Rig::dress` for a robot, `Built::of` and `Emotes::stir` for a house) and
draws them offscreen with Bevy as the engine would, lit by the realm's
sky. The realm's source is built natively by
`hamlet-server/assets-src/realm/Cargo.toml`, as the plugin itself is a
`cdylib` for wasm32-wasip2.

A skin or a mesh is seen turning about it. An emote is seen at moments
of it as the view turns a little: a clip over its length, and a hamlet's
from before its front, its lanterns and fireflies after dark.

## The costumier

The bazaar's sixth stall, LOOKS, is Madame Perpetua's costumier's booth
(`plugins/realm/src/places/bazaar/build/stall/costumier.rs`): a rail of
robes, a dress form, bolts of cloth and pots of paint, a looking glass,
and a little robot turning on a turntable. A click on it sends the
realm's `stall` with `assets`, which the bazaar takes as its Asset stall,
and a `traded` of `assets` cheers a sale there as at every stall.

## Buying and wearing them

| Part | Where |
|---|---|
| The catalogue, what is owned and worn, buying | `plugins/bazaar/src/wardrobe.rs` |
| What is kept in the store for the bazaar and the realm | `plugins/bazaar/src/wardrobe/closet.rs` (`Closet`) |
| An asset's turntable | `plugins/bazaar/src/wardrobe/turntable.rs` (`Turning`) |
| The stall drawn: its shelf, an asset's page, the fitting | `plugins/bazaar/src/view/wardrobe.rs`, `view/wardrobe/turntable.rs`, `view/wardrobe/fitting.rs` |
| The routes of the bazaar's server | `plugins/bazaar/src/market/client.rs`; `hamlet-server/docs/assets.md` |
| An agent's and a hamlet's LOOKS | `plugins/inspector/src/agent.rs`, `hamlet.rs` |

### The stall

LOOKS has two modes, SHELF and FITTING, under who is signed in to the
bazaar's server (SIGN IN WITH GITHUB, with what it is for under it).

- **The shelf** is the shelf every stall shares, of the server's
  catalogue (`GET /v1/assets`), asked for the first time the stall is
  shown and again on LOOK AGAIN. Its chips narrow it by what wears an
  asset (FOR AGENTS, FOR HAMLETS) and by its kind (EMOTES, SKINS,
  MESHES), and it is put in order by kind or by price. A row shows an
  asset's title, its price, OWNED or WORN, and what it is.
- **An asset's page** shows its pictures on a turntable, what it is and
  its version, its summary, the box it is bought from and, once it is
  owned, WEAR IT.
- **The turntable.** An asset has `frames` pictures turned evenly about
  it, at `previewUrl` with `{frame}` filled in. The page shows one,
  the first standing in while it comes; LEFT and RIGHT turn it a
  picture by hand, and SPIN turns it by itself, a picture each eighth
  of a second, skipping those not yet come. Pictures are fetched as
  the listings' covers are (`covers.rs`): only those shown are wanted,
  the one shown and the first, or all of them while it spins; checked
  from their headers, 400 KiB and 2048 pixels a side at most; kept by
  their address, 24 at most and 24 MiB shown; let go of when the page
  closes or the turntable stops; and handed to the engine 400 KiB an
  update at most. While it spins the plugin asks to be called each
  frame (`Animate`).
- **Buying.** BUY, or GET for one that is free, asks the server
  (`POST /v1/assets/:id/purchase`): a free asset, or one owned already
  (`409`), is the chief's at once; a paid one opens Stripe's page and
  is asked after (`GET /v1/asset-purchases/:id`) every five seconds for
  half an hour, the same steps as the market's deals (`pay` in
  `market/deal.rs`). A purchase asked while another waits on Stripe
  takes its place; signing out stops the wait. Once it is the chief's
  it is marked owned at once, the realm cheers (`traded` with
  `assets`), and what is owned is asked again.
- **Wearing.** WEAR IT chooses an agent of the roster, or a hamlet,
  among those that may wear it and puts it on with WEAR; each who
  wears it is listed with TAKE OFF, and how its file stands: in the
  realm, downloading, waiting, or not downloaded and asked again
  shortly. Only an owned asset of the wearer's target and of the kind
  of the slot is put on.
- **The fitting** lists every living agent and every hamlet with what
  each wears. One opened has a dropdown a kind, None and the owned
  assets that fit, PLAY EMOTE for an agent wearing an emote, and SHOP,
  which shows the shelf narrowed to what it wears. An emote put on an
  agent is played.
- **From the Inspector.** An agent's and a hamlet's LOOKS send the
  Inspector's `dress` (payload `bazaar.dress`, a `looks::Dress` of its
  target and name), and the bazaar brings its panel to the front on
  the fitting of that agent or hamlet. The realm's costumier's booth
  sends `stall` with `assets`, which shows the stall.

### What is owned and worn

The chief owns what the server says: `GET /v1/me/assets` once a chief
is signed in, and each asset the catalogue marks `owned`. What is
owned is kept as last said; signing out forgets it, so nothing of it is
worn. What each agent (by its name) and hamlet (by its name on disk)
wears is a `looks::Outfit` of asset ids, chosen whether owned or not;
only the owned are worn.

### What the bazaar keeps in the store

The closet keeps three types of asset in the engine's store, and makes
one write at a time, each once the one before is answered:

- `bazaar.wardrobe`, one asset named `wardrobe`: what is owned and
  what each wears, as JSON, written after each change. It is read as
  the plugin starts, so both outlast a restart; a saved resource
  outlasts only a reload. Which mode the stall shows and whose looks
  the fitting shows are saved resources.
- `looks.file`, the parts of the file of each asset worn and owned,
  named `<id>@<version>#<part>`. A file is downloaded once
  (`GET /v1/assets/:id/file` with `offset` and `limit`), one part of
  `looks::FILE_PART` bytes a fetch, within the 512 KiB a fetch may
  bring, through the market's line; each part is written before the
  next is asked for, so one part at most is held. A part shorter than
  asked for is the last; an empty one past the first says the one
  before was; a server that answers the whole file at once has it cut
  into parts. A file that cannot be downloaded is asked for again 30
  seconds later, or at once when it is worn again. Files download one
  at a time, while a chief is signed in.
- `looks.worn`, one asset named `worn`: the `looks::Worn` the realm
  reads, written once the files it names are whole in the store. It
  names, for each agent and hamlet, the file of each owned asset worn
  whose parts are all there, and how many parts each is in. It is
  written after a file comes, and after each change of what is owned
  or worn; nothing is written until the store was read as the plugin
  starts.
- **Then** the parts no `worn` names, such as those of an asset taken
  off, no longer owned or of an older version, are deleted, a part at
  a time. A file is whole at the start when the `worn` read names it
  and all its parts are in the store; one whose parts were not all
  written is deleted and downloaded again.

An emote put on an agent, or PLAY EMOTE, sends the bazaar's `emote`
(`looks::PLAY`, payload `looks.emote`, the agent's name) once the
`worn` written names that emote for that agent, so the realm has it to
play.
