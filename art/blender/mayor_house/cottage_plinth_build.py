"""Stonework on the cottage and its tower, laid with Brick Cover straight on their walls: a plinth band
around the bottom and, on the house, corner stones (quoins) up every corner.

Run it from Blender's Text Editor with cottage_tower_roof_tiles.blend open, then save. It changes
only the open file. Run again, it keeps what is there: it rebuilds Brick Cover (and Tile Cover)
with every modifier's settings kept and adds only what is missing.

What it does:
* the five hand-made stones (Brick01 ... Brick05) stay where they stand, in a
  "Plinth Stones" collection that shows in the viewport but not in renders; it is the modifiers'
  Bricks. Edit a stone's mesh and every stone laid from it changes with it (each is stretched to its
  slot, so its shape counts, not its size). A tag beside each (collection "Labels", viewport only)
  says which Brick Mix slot it is: Brick 1 ... Brick 5, in name order;
* the stones get the "Stone (procedural)" material, which paints each one with the brick_color the
  modifier gives it, so the modifier's Color panel (base, accent, shade) sets their colours;
* "Walls | Ground floor plaster" gets a Brick Cover modifier: full courses up to PLINTH_HEIGHT (Band
  Height), corner stones above (Corner Stones), nothing on its top and bottom faces (Skip Faces, the
  no_bricks face attribute). The tower wall ("Circle") gets one too, first in its stack so its
  Solidify leaves the stones alone, with Flip Sides on (its normals point inwards);
* cutouts: the house's courses stop at the tower and at the front door (closed meshes in "House
  plinth cutouts", the house modifier's Exclude Objects); the tower's stop inside the house's walls
  ("Tower plinth cutouts"). The door's box is parented to the door's Empty, so it moves with the door.
  Any closed mesh put in one of those collections cuts that wall's stones the same way.
* paving: "Paving slab 1 ... 3" are linked copies of the square roof tiles (Roof tile 1, 2, 5: same mesh,
  so editing a tile reshapes its slabs; the tiles' material paints them with Paving's colours), in a "Paving Slabs" collection in
  front of the roof tiles; "Ground | Paving left", a flat strip along the house's left wall, gets a
  Paving modifier laying them in a grid (Slab Size, Gap, Row Offset, a random turn and tilt for each);
* an earlier version laid the plinth on separate band meshes ("Plinth | house", "Plinth | tower");
  their modifier settings move to the walls and the band meshes go.

Brick Cover itself lives in C:/develop/projects/website/brick_cover/brick_cover_build.py (set
BRICK_COVER_DIR to use another copy).
"""
import importlib
import math
import os
import sys

import bmesh
import bpy

sys.path.insert(0, os.environ.get("BRICK_COVER_DIR", "C:/develop/projects/website/brick_cover"))
import brick_cover_build as B  # noqa: E402
import tile_cover_build as T  # noqa: E402

# Blender keeps modules imported by an earlier run of a script: load the current code
B = importlib.reload(B)
T = importlib.reload(T)
G = B.G
HOUSE_WALLS = "Walls | Ground floor plaster"
TOWER_WALL = "Tower | Wall"     # "Circle" before organize_scene.py
DOOR_PART = "Door | Front door | Leaf"   # the door leaf ("door.001" before organize_scene.py); its parent
#                                          Empty carries the door, its arch and lantern
STONES = ["Brick01", "Brick02", "Brick03", "Brick04", "Brick05"]
PLINTH_HEIGHT = 0.9
PLINTH = {"Course Height": 0.3, "Brick Length": 0.6, "Length Variation": 0.35, "Depth": 0.14,
          "Mortar Gap": 0.025, "Surface Offset": -0.04, "Rotation Jitter": math.radians(1.5),
          "Depth Jitter": 0.015}
# the stones' own colours: Warm Beige, with the lighter sandstone as the accent
COLORS = {"Base Color": (0.532, 0.388, 0.238, 1.0), "Accent Color": (0.71, 0.539, 0.367, 1.0),
          "Accent Chance": 0.3, "Shade Variation": 0.1}
HOUSE = {"Band Height": PLINTH_HEIGHT, "Corner Stones": True, "Corner Long": 0.7, "Corner Short": 0.4,
         "Skip Faces": "attr:" + B.SKIP_ATTRIBUTE}
TOWER = {"Band Height": PLINTH_HEIGHT, "Flip Sides": True}
# the five stones (Brick01 ... Brick05 are Brick 1 ... 5), as often as each other
BRICK_MIX = {f"Brick {i}": 100.0 for i in range(1, 11)}
STONE_LOOK = 3          # bumped when BRICK_MIX changes: walls laid by an older run get it once
HOUSE_CUTOUTS = "House plinth cutouts"
TOWER_CUTOUTS = "Tower plinth cutouts"
# what earlier versions of this script called them
OLD_CUTOUTS = {"Plinth Cutters | tower": HOUSE_CUTOUTS, "Plinth Cutters | house": TOWER_CUTOUTS}
OLD_BANDS = {"Plinth | house": HOUSE_WALLS, "Plinth | tower": TOWER_WALL}


def collection(name, viewport=True):
    """A collection under the scene that never renders; viewport: whether it shows in the viewport."""
    coll = bpy.data.collections.get(name) or bpy.data.collections.new(name)
    if name not in bpy.context.scene.collection.children:
        bpy.context.scene.collection.children.link(coll)
    coll.hide_render = True
    coll.hide_viewport = not viewport
    return coll


def remove(obj):
    me = obj.data
    bpy.data.objects.remove(obj)
    if me is not None and me.users == 0:
        bpy.data.meshes.remove(me)


def move_to(obj, coll):
    for c in list(obj.users_collection):
        c.objects.unlink(obj)
    coll.objects.link(obj)


def mesh_object(name, verts, faces, loc, colls):
    me = bpy.data.meshes.new(name)
    me.from_pydata(verts, [], faces)
    bm = bmesh.new()
    bm.from_mesh(me)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bm.to_mesh(me)
    bm.free()
    me.update()
    o = bpy.data.objects.new(name, me)
    for c in colls:
        c.objects.link(o)
    o.location = loc
    return o


def world_box(objs):
    ws = [o.matrix_world @ v.co for o in objs for v in o.data.vertices]
    return ([min(w[i] for w in ws) for i in range(3)], [max(w[i] for w in ws) for i in range(3)])


def tower_axis_radius():
    """The tower wall's axis and outer radius (its Solidify grows it inwards, so the mesh is the outside)."""
    t = bpy.data.objects[TOWER_WALL]
    ws = [t.matrix_world @ v.co for v in t.data.vertices]
    cx = sum(w.x for w in ws) / len(ws)
    cy = sum(w.y for w in ws) / len(ws)
    return cx, cy, max(math.hypot(w.x - cx, w.y - cy) for w in ws)


def box_sides(lo, hi, z0, z1):
    verts = [(x, y, z) for z in (z0, z1) for x, y in ((lo[0], lo[1]), (hi[0], lo[1]), (hi[0], hi[1]), (lo[0], hi[1]))]
    faces = [(i, (i + 1) % 4, 4 + (i + 1) % 4, 4 + i) for i in range(4)] + [(3, 2, 1, 0), (4, 5, 6, 7)]
    return verts, faces


def ring(r, z0, z1, sides):
    verts = [(r * math.cos(2 * math.pi * i / sides), r * math.sin(2 * math.pi * i / sides), z)
             for z in (z0, z1) for i in range(sides)]
    faces = [(i, (i + 1) % sides, sides + (i + 1) % sides, sides + i) for i in range(sides)]
    faces += [tuple(reversed(range(sides))), tuple(range(sides, 2 * sides))]
    return verts, faces


def build_groups():
    """(Re)build Brick Cover, and Tile Cover with it when the file has it (they share their first
    stages), keeping every modifier's settings."""
    if bpy.data.node_groups.get(T.NAME) is not None:
        T.rebuild_covers(bricks=True, paving=True)
    elif bpy.data.node_groups.get(B.NAME) is not None:
        B.update_open_file()
    else:
        B.build().asset_mark()
    return bpy.data.node_groups[B.NAME]


def cutters():
    """The two cutout meshes, as tall as the walls (the corner stones run up them too)."""
    walls = bpy.data.objects[HOUSE_WALLS]
    lo, hi = world_box([walls])
    z0, z1 = lo[2] - 0.2, hi[2] + 0.2
    cx, cy, r = tower_axis_radius()
    depth = 0.2      # past the outside of the tower's stones
    house_cut, tower_cut = collection(HOUSE_CUTOUTS, False), collection(TOWER_CUTOUTS, False)
    for old in ("Plinth cutter | tower", "Plinth cutout | tower", "Plinth cutter | house", "Plinth cutout | house"):
        o = bpy.data.objects.get(old)
        if o is not None:
            remove(o)
    # the house's courses stop at the outside of the tower's stones
    cut = mesh_object("Plinth cutout | tower", *ring(r + depth, z0, z1, 48), (cx, cy, 0.0), [house_cut])
    cut.display_type = "WIRE"
    # the tower's courses stop a little inside the house's walls, behind the house's stones
    inset = 0.05
    cut = mesh_object("Plinth cutout | house", *box_sides([lo[0] + inset, lo[1] + inset],
                                                          [hi[0] - inset, hi[1] - inset], z0, z1),
                      (0, 0, 0), [tower_cut])
    cut.display_type = "WIRE"


def skip_caps(obj):
    """no_bricks on the faces that are not walls (the top and bottom of the box)."""
    me = obj.data
    attr = me.attributes.get(B.SKIP_ATTRIBUTE) or me.attributes.new(B.SKIP_ATTRIBUTE, "BOOLEAN", "FACE")
    for p in me.polygons:
        attr.data[p.index].value = abs(p.normal.z) > 0.7


def lay_on_walls(tree, stones):
    """A Brick Cover modifier on the house wall and on the tower wall, with the settings of the band
    meshes an earlier run made (then removed) or else PLINTH and COLORS."""
    old = {}
    for md, vals in T.modifier_settings(B.NAME):
        if md.id_data.name in OLD_BANDS:
            old[OLD_BANDS[md.id_data.name]] = vals
    for name, own, excl, seed in ((HOUSE_WALLS, HOUSE, HOUSE_CUTOUTS, 3), (TOWER_WALL, TOWER, TOWER_CUTOUTS, 7)):
        obj = bpy.data.objects[name]
        if obj.modifiers.get(B.NAME) is not None:
            continue
        if name == HOUSE_WALLS:
            skip_caps(obj)
        md = obj.modifiers.new(B.NAME, "NODES")
        md.node_group = tree
        if name == TOWER_WALL:     # before Solidify, which would also thicken the wall's stones' layer
            with bpy.context.temp_override(object=obj):
                bpy.ops.object.modifier_move_to_index(modifier=md.name, index=0)
        vals = old.get(name) or {**PLINTH, **COLORS, "Seed": seed}
        G.set_inputs(md, {**vals, "Bricks": stones, "Exclude Objects": bpy.data.collections[excl], **own})
        print(f"stonework: Brick Cover on {name!r}")
    for name in (HOUSE_WALLS, TOWER_WALL):
        obj = bpy.data.objects[name]
        md = obj.modifiers.get(B.NAME)
        if md is not None and obj.get("stone_look", 0) < STONE_LOOK:
            G.set_inputs(md, BRICK_MIX)
            obj["stone_look"] = STONE_LOOK
    for band in OLD_BANDS:
        o = bpy.data.objects.get(band)
        if o is not None:
            remove(o)
            print(f"stonework: removed the old band mesh {band!r}")


def colour_stones(stones):
    """Give the stones the material that shows the modifier's colours."""
    mat = B.stones_material(fallback=COLORS["Base Color"])
    for o in stones.objects:
        if o.type == "MESH" and list(o.data.materials) != [mat]:
            o.data.materials.clear()
            o.data.materials.append(mat)


def door_cutout():
    """A closed box over the front door's arch, parented to the door's Empty so it follows the door."""
    if bpy.data.objects.get("Plinth cutout | door") is not None:
        return None
    leaf = bpy.data.objects.get(DOOR_PART)
    if leaf is None or leaf.parent is None:
        print("stonework: no door found, no door cutout")
        return None
    rig = leaf.parent
    parts = [o for o in rig.children_recursive if o.type == "MESH" and o.name.split(".")[0] in ("stones", "door")]
    lo, hi = world_box(parts)
    walls_lo, walls_hi = world_box([bpy.data.objects[HOUSE_WALLS]])
    # through the whole wall layer: from under the ground to the arch's top, from inside the wall to past the arch
    y0 = min(lo[1], walls_lo[1]) - 0.4
    y1 = walls_lo[1] + 0.3 if hi[1] < walls_lo[1] + 0.3 else hi[1]
    cut = mesh_object("Plinth cutout | door", *box_sides([lo[0], y0], [hi[0], y1], walls_lo[2] - 0.2, hi[2]),
                      (0, 0, 0), [collection(HOUSE_CUTOUTS, False)])
    cut.display_type = "WIRE"
    cut.parent = rig
    cut.matrix_parent_inverse = rig.matrix_world.inverted()
    print(f"stonework: door cutout under {rig.name!r}")
    return cut


PAVING = "Ground | Paving left"
PAVING_SLABS = "Paving Slabs"
PAVING_FROM = ["Roof tile 1", "Roof tile 2", "Roof tile 5"]    # the square roof tiles the slabs are shaped like
PAVING_WIDTH = 1.8          # out from the wall: three slabs
PAVING_SETTINGS = {"Slab Size": 0.6, "Gap": 0.06, "Thickness": 0.08, **COLORS}


def paving_slabs():
    """Paving Slab 1 ... 3: linked copies of the square roof tiles (same mesh, so editing a tile reshapes
    its slabs), standing in a row in front of the roof tiles. Laid, they keep the tiles' material, which
    paints them with the stone colour Paving gives each (the copies standing as models stay the tiles'
    colour). None when the tiles are not in the file."""
    coll = bpy.data.collections.get(PAVING_SLABS)
    if coll is not None:
        return coll
    tiles = [bpy.data.objects.get(n) for n in PAVING_FROM]
    if None in tiles:
        print("paving: the square roof tiles (Roof tile 1, 2, 5) are not in the file")
        return None
    coll = collection(PAVING_SLABS)
    for i, t in enumerate(tiles):
        o = bpy.data.objects.new(f"Paving slab {i + 1}", t.data)
        o.matrix_world = t.matrix_world.copy()
        o.location.y -= 1.1
        coll.objects.link(o)
    return coll


def paving_bed():
    m = bpy.data.materials.get("Paving bed")
    if m is None:
        m = bpy.data.materials.new("Paving bed")
        m.use_nodes = True
        bsdf = next(n for n in m.node_tree.nodes if n.type == "BSDF_PRINCIPLED")
        bsdf.inputs["Base Color"].default_value = (0.13, 0.17, 0.07, 1.0)     # grass showing between the slabs
        bsdf.inputs["Roughness"].default_value = 0.95
    return m


def paving(slabs):
    """A flat strip of ground along the house's left wall (-X), the wall's length and PAVING_WIDTH out
    from its plinth, with a Paving modifier laying the slabs over it. Reshape or move the strip to move
    the paving. Kept as it is when it is there already."""
    if bpy.data.objects.get(PAVING) is not None or slabs is None:
        return None
    lo, hi = world_box([bpy.data.objects[HOUSE_WALLS]])
    x1 = lo[0] - PLINTH["Depth"] - 0.06
    x0 = x1 - PAVING_WIDTH
    y0, y1 = lo[1] - 0.3, hi[1] + 0.3
    ob = mesh_object(PAVING, [(x0, y0, 0), (x1, y0, 0), (x1, y1, 0), (x0, y1, 0)], [(0, 1, 2, 3)],
                     (0, 0, lo[2]), list(bpy.data.objects[HOUSE_WALLS].users_collection))
    ob.data.materials.append(paving_bed())
    md = ob.modifiers.new(T.PAVING_NAME, "NODES")
    md.node_group = bpy.data.node_groups[T.PAVING_NAME]
    G.set_inputs(md, {"Slabs": slabs, **PAVING_SETTINGS})
    print("paving: laid slabs along the left wall")
    return ob


def main():
    tree = build_groups()
    for old, new in OLD_CUTOUTS.items():
        c = bpy.data.collections.get(old)
        if c is not None and bpy.data.collections.get(new) is None:
            c.name = new
    stones = collection("Plinth Stones")
    for n in STONES:
        o = bpy.data.objects.get(n)
        if o is not None and stones not in o.users_collection:
            move_to(o, stones)
    colour_stones(stones)
    B.label_slots(stones, "Brick")
    cutters()
    door_cutout()
    lay_on_walls(tree, stones)
    slabs = paving_slabs()
    if slabs is not None:
        if bpy.data.materials.get("Roof tile (procedural)") is not None:
            T.tiles_material()      # the tiles' material, so it also reads the colour Paving gives a slab
        paving(slabs)
        B.label_slots(slabs, "Slab", size=0.08)
        B.fit_slots(T.PAVING_NAME, "Slabs", "Slab")


if __name__ == "__main__":
    main()
