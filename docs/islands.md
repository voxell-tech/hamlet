# The two islands, and moving hamlets

The world is two islands with open water between them. The **city island**
holds the Hand's tower, the Library, the Hearth and a house for each
scholar. The **hamlet island** holds the hamlets, each drawn as a group of
its hires' houses joined by roads. A pier on each island faces the other,
with a boat moored at it and a dashed sea route between the two. The code is
`hamlet_realm/src/generator/`, `hamlet_realm/src/city/` and
`hamlet_realm/src/harbor.rs`. The generator names what it draws a hamlet
as by its type, `Town` (`society.found_town`); the words here say hamlet.

`cargo run --release --example far_lands -- --out <dir>` draws both
islands, many hamlets, the city with houses and newcomers, and a hamlet
being moved to stills. `--bench` times it.

## Where the islands lie

The city island is centred on the origin. The hamlet island lies along a
seeded bearing (`LandData::bearing`), far enough out that the two never
touch: its middle is the two islands' radii plus `[land] channel` of open
water from the origin.

An island reaches about its **shores**: its middle, then what stands on it,
the city's houses (`Land::fill_city`) or the hamlet island's hamlets
(`Land::settle`). Its land is a smooth union of rounds about those points
with a ragged coast.

An island's radius is rounded up to `[land] island_step`. When either
island grows across a step, the hamlet island moves out as a whole, every
site with it (`reshore`), and the ground is built again. Between steps
nothing moves.

**Positions are kept from an island's middle.** A hamlet's place on the wire
and in the record, and a house's spot in the city, are measured from their
island's centre. The world turns them into world positions in one place.

**What each island has.** The hamlet island has the rivers, low flat-topped
hills and its hamlets. The city island is level, with no rivers or hills, so
houses can stand anywhere on it; its trees keep off the houses.

## The ground and the trees

The land is a function: `LandData::height(p)` gives the height at any
point from the seed and from what stands on the islands. No part of it is
stored.

The ground is drawn in **patches** (`hamlet_vfx::patch`): a quadtree of
squares fixed on the map, chosen each frame, each cut into four while the
eye is near it, so every patch is the same grid and about as large to the
eye. Where a patch meets a larger one its points slide onto the larger
one's grid, so there is no seam and none pops as it splits. The sea is drawn
in the same patches. A compute pass makes pages of the land about the eye,
one for each size of patch (`generator/relief.rs`, `relief.wesl`), with the
heights `LandData::height_wide` gives, and the ground's shader raises its
patches from them. A coarser patch gives each river a wider bed, so it does
not break into steps.

**Everything stands on the ground as it is drawn.** `LandData::surface` is
the height of the finest patches' flat triangles. Roads, robots, houses and
the piers take their height from `surface`, never from the smooth
`height`, so none of them dips under a triangle. A coarser patch is drawn
`tile_sink` under the true ground, so nothing that stands on the near
ground is covered by a patch seen from far.

**Trees** are cut into tiles of their own (`ground::wanted`): four tiles of
one size lie under one of the next. Near the camera every tree stands as
itself, further off a tile's trees are one mesh of plain crowns, and beyond
that they are specks. Tiles are built in background tasks, nearest first,
and taken out when the view has left them; one is kept until what replaces
it is drawn. Trees keep off the roads between hamlets.

**The same every time.** A tile's trees follow from the seed, what stands
on the islands and the tile's own place. Tests hold it to that.

## Always room for one more hamlet

`Society::found_town` never fails. `Land::open_site` walks the grid of
cells outward from the middle of the hamlet island and takes the first one
that is dry, clear of rivers and far enough from every other site. The
island then grows about the new hamlet; nothing that stood moves unless the
island crosses a step, and then all of it moves together.

Roads between hamlets run along the borders of the same grid of cells. A
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

Patches and tiles of trees are bounded by the view, not by how far the
camera has been or how many hamlets there are. Houses and robots are drawn
only within `DetailSettings::town_far` and `robots_far`. What the society
holds is bounded by the society. The settings are in `assets/world.toml`,
and each is explained beside its field.

## The camera

The focus may go anywhere within `CameraSettings::pan` of a hamlet, of the
city's places, or of the straight way between any two of them
(`pick::Leash`). Past that the slide stops, so a chief cannot slide into
open sea with no way to tell which way is home. F on a selection and Reset
Camera go back.

## Moving a hamlet

The chief picks a hamlet up from its common's menu or its page. A footprint
follows the pointer, gold where the hamlet can be set down and red where it
cannot, with a few words saying why. A click sets it down, and Escape puts
it back. Each house then sinks where it stood and rises in its new place.

**Where a hamlet may stand** (`Society::fits`): on the hamlet island's dry
ground away from rivers, far enough from every other hamlet and claim, and
with every house on a dry plot.

**What moves.** The hamlet keeps its site and the site moves
(`Land::move_site`). Everything about the hamlet is keyed by its site and
its agents, so its houses, plots and roads are the same.

**How it is recorded.** The click in the world moves nothing itself. It
sends `/move-hamlet <hamlet> <x> <z>`, measured from the hamlet island's
middle, and the world moves the hamlet when the server's state says so. A
place typed by hand that the ground cannot take is refused by the world,
and the hamlet stays.

## Another kind of place

Moving is built on `society::Ground`, a thing that holds a round of ground
and can be picked up and set down. A hamlet is one kind; the Hearth and the
Library are others. A kind of place needs four things:

1. **A claim on its ground** (`society.claim`), which keeps hamlets and new
   sites off it and lets the camera go to it.
2. **A way to start a move**: `moving::pick_up`. The footprint, the
   colours, the words and the click are the same code for every kind.
3. **A record**: an arm in `set_down` in `hamlet_realm/src/orders.rs`.
4. **A way to show it going.** The house-by-house move is for hamlets.

A claim only keeps hamlets away. The ground itself is held by `Land::hold`,
which keeps a round level and clear of trees.
