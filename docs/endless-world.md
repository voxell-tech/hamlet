# The endless world, and moving towns

The land has no edge and can always take one more town, and a town or the
chief's own place can be picked up and set down somewhere else. The code
is `hamlet_world/src/generator/`.

`cargo run --release --example far_lands -- --out <dir>` draws the land,
many towns and a town being moved to stills. `--bench` times it.

## The ground: tiles around the view

The land is a function: `LandData::height(p)` gives the height at any
point from the seed and from where the sites lie. No part of it is stored.

The ground that is drawn is cut into square tiles in six sizes, four of
one size under one of the next. `ground::wanted` picks the tiles the view
calls for, and splits each into its quarters while the camera is near it.
Tiles are built in background tasks, nearest first, and taken out when the
view has left them. Fog hides the edge of what is drawn.

A tree of tiles was chosen because it can be fine near the camera, coarse
at the horizon, and still split further just where a river or a town needs
it. Only ground a town stands on is levelled and cleared, so the land is a
function of the seed and the sites.

**No seam, no hole.**

- Where two tiles of different grain touch, the finer takes the height of
  its edge from the coarser, so the two meet with no step. A tile is built
  for the sizes of the tiles about it (`ground::rim_key`) and built anew
  when one of them is split or joined.
- A skirt under every edge covers what is left while that happens.
- A tile is kept until what replaces it is drawn.
- Under everything lies a plain green sheet, which shows where no tile is
  drawn yet.
- The ground is asked for where the camera is going, not only where it is.

**Everything stands on the ground as it is drawn.** `LandData::surface` is
the height of the finest tiles' flat triangles. Roads, robots, houses and
the chief's place take their height from `surface`, never from the smooth
`height`, so none of them dips under a triangle. A coarser tile is built
as a lower bound of the finest, so nothing that stands on the near ground
is covered by a tile seen from far. Each of its points drops only by as
much as its triangles would rise over the ground between their corners,
so a far slope keeps its height.

**Trees** are cut into tiles of their own. Near the camera every tree
stands as itself, further off a tile's trees are one mesh of plain crowns,
and beyond that they are specks. Trees keep off the roads between towns.

**The same every time.** A tile's mesh and trees follow from the seed, the
sites, the tile's own place and, for the edge of the mesh, the sizes of
the tiles about it, whatever was built before it. Tests hold it to that.

## Always room for one more town

`Society::found_town` never fails. When the first sites are taken,
`Land::open_site` walks the grid of cells outward and takes the first one
that is dry, clear of rivers, out of the mountains and far enough from
every other site. Nothing that stood moves.

Roads between towns run along the borders of the same grid of cells. A
cart takes at most `RoadSettings::ride_time` one way, however long the
road.

## What bounds the cost

Tiles of ground and trees are bounded by the view, not by how far the
camera has been or how many towns there are. Houses and robots are drawn
only within `DetailSettings::town_far` and `robots_far`. What the society
holds is bounded by the society.

On the owner's machine a frame takes 2 to 3.5 ms from 8 towns seen near to
42 towns at the widest view. The settings are in `assets/world.toml`, and
each is explained beside its field.

## The camera

The focus may go anywhere within `CameraSettings::pan` of a town, of the
chief's place, or of the straight way between any two of them
(`pick::Leash`). Past that the slide stops. With no limit a chief can
slide into empty land with no way to tell which way is home. The fog hides
every town well before the limit, so the land still reads as endless. F on
a selection and Reset Camera go back.

## Moving a town

The chief picks a town up from its common's menu or its page. A footprint
follows the pointer, gold where the town can be set down and red where it
cannot, with a few words saying why. A click sets it down, and Escape puts
it back. Each house then sinks where it stood and rises in its new place.

**Where a town may stand** (`Society::fits`): on dry ground away from
rivers and mountains, far enough from every other town and claim, and with
every house on a dry plot.

**What moves.** The town keeps its site and the site moves
(`Land::move_site`). Everything about the town is keyed by its site and
its agents, so its houses, plots and roads are the same.

**How it is recorded.** A move is a chief's act in `realm.jsonl`:

```json
{"v":1,"type":"hamlet_moved","ts":1,"name":"mill","by":"alice","x":412.5,"z":-96.0}
```

The click in the world moves nothing itself. It sends the command
`/move-hamlet`, and the world moves the town when the record says so. A
launch replays it the same way. A place typed by hand that the ground
cannot take is recorded and refused by the world, and the town stays.

## Another kind of place: the chief's

Moving is built on `society::Place`, a thing that holds a round of ground
and can be picked up and set down. A town is one kind and the Hearth is
another. A kind of place needs four things:

1. **A claim on its ground** (`society.claim`), which keeps towns and new
   sites off it and lets the camera go to it.
2. **A way to start a move**: `moving::pick_up`. The footprint, the
   colours, the words and the click are the same code for every kind.
3. **A record**: an arm in `set_down` in `hamlet_ui/src/hand.rs`.
4. **A way to show it going.** The house-by-house move is for towns.

A claim only keeps towns away. The ground itself is held by `Land::hold`,
which keeps a round level and clear of trees.
