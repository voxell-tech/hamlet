# The two islands, and moving hamlets

The world is two islands with open water between them. The **city island**
holds the Hand's tower, the Library, the Hearth and a house for each
profile. The **hamlet island** holds the hamlets. The code is
`hamlet_realm/src/generator/`, `hamlet_realm/src/city/` and
`hamlet_realm/src/harbor.rs`. The generator's type for what it draws a
hamlet as is `Town` (`society.found_town`).

`cargo run --release --example far_lands -- --out <dir>` draws both islands
to stills. Settings are in `assets/world.toml`.

## Where the islands lie

An island's radius is rounded up to `[land] island_step`. When either island
grows across a step, the hamlet island moves out as a whole, every site with
it (`reshore`), and the ground is built again. Between steps nothing moves.

Positions on the wire and in the record are kept from an island's middle;
the world turns them into world positions in one place.

## The ground and the trees

The land is a function: `LandData::height(p)` gives the height at any point
from the seed and from what stands on the islands. No part of it is stored.
The ground and the sea are drawn in patches (`hamlet_vfx::patch`), chosen
each frame.

Everything stands on the ground as it is drawn. `LandData::surface` is the
height of the finest patches' flat triangles. Roads, robots, houses and the
piers take their height from `surface`, never from the smooth `height`, so
none dips under a triangle. A coarser patch is drawn `tile_sink` under the
true ground, so a patch seen from far never covers what stands on the near
ground.

Trees are cut into tiles (`ground::wanted`). A tile's trees follow from the
seed, what stands on the islands and the tile's own place, so they are the
same every time; tests hold it to that.

## Always room for one more hamlet

`Society::found_town` never fails. `Land::open_site` walks a grid of cells
outward from the middle of the hamlet island and takes the first one that is
dry, clear of rivers and far enough from every other site. The island then
grows about the new hamlet; nothing that stood moves unless the island
crosses a step.

## The city and the harbor

A profile's house stands at a free spot on a spiral out from the city's
middle ([`society.md`](society.md)). Each pier stands where its island's
coast faces the other (`Land::coast_toward_other`).

What is drawn is bounded by the view and by `DetailSettings::town_far` and
`robots_far`, not by how many hamlets there are. The camera's focus is kept
within `CameraSettings::pan` of a place or the way between two
(`pick::Leash`), so a chief cannot slide into open sea.

## Moving a hamlet

A hamlet may stand (`Society::fits`) on the hamlet island's dry ground away
from rivers, far enough from every other hamlet and claim, with every house
on a dry plot.

The hamlet keeps its site and the site moves (`Land::move_site`); everything
about the hamlet is keyed by its site and its agents.

The click in the world moves nothing itself. It sends `/move-hamlet <hamlet>
<x> <z>`, measured from the hamlet island's middle, and the world moves the
hamlet when the server's state says so. A place the ground cannot take is
refused by the world.

## Another kind of place

Moving is built on `Ground`, which names a place that holds a round of
ground and can be picked up and set down. A hamlet (`Ground::Town`) is one
kind; a profile's house, the Hearth (`Ground::Chief`) and the Library are
others. A kind of place needs:

1. **A claim on its ground** (an entry of `society.claims`), which keeps
   hamlets and new sites off it and lets the camera go to it.
2. **A way to start a move**: `moving::pick_up`. The footprint, colours,
   words and click are the same code for every kind.
3. **A record**: an arm in `set_down` in `hamlet_realm/src/orders.rs`.
4. **A way to show it going.** The house-by-house move is for hamlets.

A claim only keeps hamlets away. The ground itself is held by `Land::hold`,
which keeps a round level and clear of trees.
