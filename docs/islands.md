# The two islands, and moving towns

The world is two islands with open water between them. The **city island**
holds the Hand's tower, the Library, the Hearth and a house for each
scholar. The **hamlet island** holds the hamlets, each a town of its hires'
houses joined by roads. A pier on each island faces the other, with a boat
moored at it and a dashed sea route between the two. The code is
`hamlet_realm/src/generator/`, `hamlet_realm/src/city/` and
`hamlet_realm/src/harbor.rs`.

`cargo run --release --example far_lands -- --out <dir>` draws both
islands, many towns, the city with houses and newcomers, and a town being
moved to stills. `--bench` times it.

## Where the islands lie

The city island is centred on the origin. The hamlet island lies along a
seeded bearing (`LandData::bearing`), far enough out that the two never
touch: its middle is the two islands' radii plus `[land] channel` of open
water from the origin.

An island reaches about its **shores**: its middle, then what stands on it,
the city's houses (`Land::fill_city`) or the hamlet island's towns
(`Land::settle`). Its land is a smooth union of rounds about those points
with a ragged coast.

An island's radius is rounded up to `[land] island_step`. When either
island grows across a step, the hamlet island moves out as a whole, every
site with it (`reshore`), and the ground is built again. Between steps
nothing moves.

**Positions are kept from an island's middle.** A town's place on the wire
and in the record, and a house's spot in the city, are measured from their
island's centre. The world turns them into world positions in one place.

**What each island has.** The hamlet island has the rivers, low flat-topped
hills and its towns. The city island is level, with no rivers or hills, so
houses can stand anywhere on it; its trees keep off the houses.

## The ground: tiles around the view

The land is a function: `LandData::height(p)` gives the height at any
point from the seed and from what stands on the islands. No part of it is
stored.

The ground that is drawn is cut into square tiles in six sizes, four of
one size under one of the next. `ground::wanted` picks the tiles the view
calls for, and splits each into its quarters while the camera is near it.
Tiles are built in background tasks, nearest first, and taken out when the
view has left them. Fog hides the edge of what is drawn.

A tree of tiles was chosen because it can be fine near the camera, coarse
at the horizon, and still split further just where a river or a town needs
it. Only ground a town stands on is levelled and cleared.

**No seam, no hole.**

- Where two tiles of different grain touch, the finer takes the height of
  its edge from the coarser, so the two meet with no step. A tile is built
  for the sizes of the tiles about it (`ground::rim_key`) and built anew
  when one of them is split or joined.
- A skirt under every edge covers what is left while that happens.
- A tile is kept until what replaces it is drawn.
- The ground is asked for where the camera is going, not only where it is.

**Everything stands on the ground as it is drawn.** `LandData::surface` is
the height of the finest tiles' flat triangles. Roads, robots, houses and
the piers take their height from `surface`, never from the smooth
`height`, so none of them dips under a triangle. A coarser tile is built
as a lower bound of the finest, so nothing that stands on the near ground
is covered by a tile seen from far.

**Trees** are cut into tiles of their own. Near the camera every tree
stands as itself, further off a tile's trees are one mesh of plain crowns,
and beyond that they are specks. Trees keep off the roads between towns.

**The same every time.** A tile's mesh and trees follow from the seed,
what stands on the islands, the tile's own place and, for the edge of the
mesh, the sizes of the tiles about it. Tests hold it to that.

## Always room for one more town

`Society::found_town` never fails. `Land::open_site` walks the grid of
cells outward from the middle of the hamlet island and takes the first one
that is dry, clear of rivers and far enough from every other site. The
island then grows about the new town; nothing that stood moves unless the
island crosses a step, and then all of it moves together.

Roads between towns run along the borders of the same grid of cells. A
cart takes at most `RoadSettings::ride_time` one way, however long the
road.

## The city

A scholar's house stands at a free spot on a golden-angle spiral out from
the city's middle, chosen by the server when the scholar is let in, its
door toward the middle. A chief sets it elsewhere with `/move-house`.

A scholar an agent makes may first wait on the city pier as a newcomer,
by the realm's `newcomers` setting (`docs/society.md`). It stands there
until a chief, or the Hand, lets it in, and then walks to its house.

## The harbor

Each pier stands where its island's coast faces the other island
(`Land::coast_toward_other`). The boats ride the swell on the processor
(`harbor::bob`), so whoever stands in one rides with it. A new hire walks
from its scholar's house to the city pier, crosses by boat and walks to
its house in its hamlet. The HARBOR control glides the camera to the other
pier.

## What bounds the cost

Tiles of ground and trees are bounded by the view, not by how far the
camera has been or how many towns there are. Houses and robots are drawn
only within `DetailSettings::town_far` and `robots_far`. What the society
holds is bounded by the society. The settings are in `assets/world.toml`,
and each is explained beside its field.

## The camera

The focus may go anywhere within `CameraSettings::pan` of a town, of the
city's places, or of the straight way between any two of them
(`pick::Leash`). Past that the slide stops, so a chief cannot slide into
open sea with no way to tell which way is home. F on a selection and Reset
Camera go back.

## Moving a town

The chief picks a town up from its common's menu or its page. A footprint
follows the pointer, gold where the town can be set down and red where it
cannot, with a few words saying why. A click sets it down, and Escape puts
it back. Each house then sinks where it stood and rises in its new place.

**Where a town may stand** (`Society::fits`): on the hamlet island's dry
ground away from rivers, far enough from every other town and claim, and
with every house on a dry plot.

**What moves.** The town keeps its site and the site moves
(`Land::move_site`). Everything about the town is keyed by its site and
its agents, so its houses, plots and roads are the same.

**How it is recorded.** The click in the world moves nothing itself. It
sends `/move-hamlet <hamlet> <x> <z>`, measured from the hamlet island's
middle, and the world moves the town when the server's state says so. A
place typed by hand that the ground cannot take is refused by the world,
and the town stays.

## Another kind of place

Moving is built on `society::Ground`, a thing that holds a round of ground
and can be picked up and set down. A town is one kind; the Hearth and the
Library are others. A kind of place needs four things:

1. **A claim on its ground** (`society.claim`), which keeps towns and new
   sites off it and lets the camera go to it.
2. **A way to start a move**: `moving::pick_up`. The footprint, the
   colours, the words and the click are the same code for every kind.
3. **A record**: an arm in `set_down` in `hamlet_realm/src/orders.rs`.
4. **A way to show it going.** The house-by-house move is for towns.

A claim only keeps towns away. The ground itself is held by `Land::hold`,
which keeps a round level and clear of trees.
