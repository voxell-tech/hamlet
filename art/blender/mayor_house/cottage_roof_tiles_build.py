"""Tile the cottage's main roof with Tile Cover and save the result as a new file.

    blender -b cottage_tower_v1.blend -P cottage_roof_tiles_build.py [-- --force]

Reads cottage_tower_v1.blend (left untouched) and writes cottage_tower_roof_tiles.blend next to it.
It stops if that file already exists, so a saved file is never replaced; pass --force to replace it.

Run from Blender's Text Editor instead (with cottage_tower_roof_tiles.blend open), it updates the open
file in place: rebuilds Tile Cover, keeps every modifier's settings and names the tiles Roof tile 1 ... 6
(left to right) so the Tile Mix shares match them. Save afterwards.
Tile Cover itself lives in C:/develop/projects/website/brick_cover/tile_cover_build.py (set
TILE_COVER_DIR to use another copy).

What it does:
* the six hand-drawn tiles (Cube, Cube.002 ... Cube.006) are renamed Roof tile 1 ... 6, left to right, and go in a hidden "Roof Tiles" collection and share
  one material that paints each tile with its tile_color;
* each roof slab (Main Roof | Barge board | front left / right) has its normals recalculated outwards
  (the left one was inside out), then gets a no_tiles face attribute (true on
  every face but its big outer face) and a Tile Cover modifier that skips those faces;
* a hidden box around the chimney, in the "Roof Cutters" collection, is the modifier's Exclude Objects,
  so the courses end flush against the chimney;
* the tower: the seven green tiles modelled beside the house (Plane ... Plane.008) are named Tower tile
  1 ... 7 (left to right, shortest to longest) and go in a "Tower Tiles" collection; its hand-placed
  tiles (Supplied tile row ...) go in a hidden "Tower tiles (hand placed)" collection, and a new open
  cone over its roof backing, "Tower | Cone roof | Tile Cover", gets a Cone Tiles modifier with the
  tower tiles: each tile is bent round the cone and narrowed towards the tip, its lower edge resting
  on the course below; courses shrink towards the top (Top Scale) and each takes the tile whose length
  fits it (Pick By Size), so the long tiles lie at the eave and the short ones near the cap;
* the ridge: the three caps placed by hand on it (Roof.017, Roof.019, Roof.023) are named Ridge piece
  1 ... 3 and go in a "Ridge Pieces" collection beside the roof tiles, and a curve along the ridge,
  "Main Roof | Ridge", gets a Ridge Row modifier laying them end to end, cut at the chimney (Roof
  Cutters). Move or extend the curve to move the row. The pieces get the roof tiles' material and Ridge
  Row the main roof's Tile Cover colours (once; change them on the modifier afterwards);
* the tile collections show in the viewport (not in renders) where the tiles were modelled, with a tag
  beside each saying which Tile Mix slot it is (collection "Labels"). Edit a tile's mesh and every tile
  laid from it changes with it (each is stretched to its slot, so its shape counts, not its size).
"""
import importlib
import math
import os
import sys

import bmesh
import bpy

sys.path.insert(0, os.environ.get("TILE_COVER_DIR", "C:/develop/projects/website/brick_cover"))
import brick_cover_build as B  # noqa: E402
import tile_cover_build as T  # noqa: E402

# Blender keeps modules imported by an earlier run of a script: load the current code
B = importlib.reload(B)
T = importlib.reload(T)

G = T.G
HERE = os.path.dirname(os.path.abspath(bpy.data.filepath)) if bpy.data.filepath else os.getcwd()
OUT = os.path.join(HERE, "cottage_tower_roof_tiles.blend")
# left to right as they stand in the file: square, square, split, narrow, cracked, cracked
TILE_OBJECTS = ["Cube", "Cube.002", "Cube.003", "Cube.004", "Cube.006", "Cube.005"]
SLABS = ["Main Roof | Barge board | front left", "Main Roof | Barge board | front right"]


def hidden_collection(name, viewport=True):
    """A collection under the scene that never renders; viewport: whether it shows in the viewport."""
    coll = bpy.data.collections.get(name) or bpy.data.collections.new(name)
    if name not in bpy.context.scene.collection.children:
        bpy.context.scene.collection.children.link(coll)
    coll.hide_render = True
    coll.hide_viewport = not viewport
    return coll


def move_to(obj, coll):
    for c in list(obj.users_collection):
        c.objects.unlink(obj)
    coll.objects.link(obj)


def face_outwards(obj):
    """The left slab's faces are wound inside out (its outer face points down into the house), so its
    tiles would lie underneath it. Recalculate the normals outwards."""
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bm.to_mesh(obj.data)
    bm.free()
    obj.data.update()


def outer_face(obj):
    """The roof slab's big outer face: the largest face that points up."""
    best, area = None, 0.0
    for p in obj.data.polygons:
        if p.normal.z > 0.1 and p.area > area:
            best, area = p.index, p.area
    return best


def chimney_cutter():
    objs = [o for o in bpy.data.objects if o.name.startswith("Chimney | Chimney ashlar")
            or o.name == "Chimney | Chimney cap"]
    lo, hi = G.world_bbox(objs)
    pad = 0.03
    verts = [(x, y, z) for x in (lo[0] - pad, hi[0] + pad) for y in (lo[1] - pad, hi[1] + pad)
             for z in (lo[2], hi[2])]
    me = bpy.data.meshes.new("Chimney cutter")
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=1.0)
    bm.to_mesh(me)
    bm.free()
    o = bpy.data.objects.new("Chimney cutter", me)
    o.location = ((lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2, (lo[2] + hi[2]) / 2)
    o.scale = (hi[0] - lo[0] + 2 * pad, hi[1] - lo[1] + 2 * pad, hi[2] - lo[2])
    o.display_type = "WIRE"
    return o


TOWER = "Tower | Cone roof"
TOWER_BACKING = TOWER + " | Recessed roof backing"
TOWER_CAP = TOWER + " | Green pointed roof cap"
TOWER_TILED = TOWER + " | Tile Cover"
TOWER_OLD_TILES = "Tower tiles (hand placed)"
# Neat rings like a storybook turret, laid with Cone Tiles: each tile bent round the cone and narrowed
# towards the tip, its lower edge resting on the course below; about eight courses, the long tiles at
# the eave and the short ones by the cap (Top Scale is about the shortest tile's length over the
# longest's, 0.65 / 1.04), now and then a terracotta one. Sizes in world units, the bottom course's.
TOWER_SETTINGS = {"Course Height": 0.45, "Tile Width": 0.6, "Overlap": 1.6, "Thickness": 0.06, "Gap": 0.03,
                  "Eave Overhang": 0.06, "Top Scale": 0.6, "Pick By Size": True, "Straighten": 0.35,
                  "Accent Color": (0.55, 0.22, 0.1, 1.0), "Accent Chance": 0.03, "Shade Variation": 0.06}
TOWER_LOOK = 3          # bumped when the tower's look changes: a cone laid by an older run gets it once
UNDERLAY = "Tower roof underlay"     # what shows in the gaps between the tiles: dark, like a shadow
TOWER_TILES = "Tower Tiles"
TOWER_TILE_OBJECTS = ["Plane", "Plane.001", "Plane.002", "Plane.003", "Plane.004", "Plane.007", "Plane.008"]


def tower_cone():
    """An open cone over the tower's roof backing, from its eave up to just under the pointed cap, with
    its origin on the axis (Tile Cover's rings need that). Not parented: the Cone roof empty is scaled
    0.6 and would shrink the tiles too."""
    back = bpy.data.objects[TOWER_BACKING]
    ws = [back.matrix_world @ v.co for v in back.data.vertices]
    z0 = min(w.z for w in ws)
    apex = max(w.z for w in ws)
    rim = [w for w in ws if w.z < z0 + 0.02]
    cx = sum(w.x for w in rim) / len(rim)
    cy = sum(w.y for w in rim) / len(rim)
    r0 = max(math.hypot(w.x - cx, w.y - cy) for w in rim)
    cap = bpy.data.objects[TOWER_CAP]
    z1 = min((cap.matrix_world @ v.co).z for v in cap.data.vertices) + 0.08   # the top course tucks under the cap
    r1 = r0 * (apex - z1) / (apex - z0)
    sides = 64
    verts = [(r * math.cos(2 * math.pi * i / sides), r * math.sin(2 * math.pi * i / sides), z)
             for r, z in ((r0, 0.0), (r1, z1 - z0)) for i in range(sides)]
    faces = [(i, (i + 1) % sides, sides + (i + 1) % sides, sides + i) for i in range(sides)]
    me = bpy.data.meshes.new(TOWER_TILED)
    me.from_pydata(verts, [], faces)
    me.update()
    o = bpy.data.objects.new(TOWER_TILED, me)
    for c in back.users_collection:
        c.objects.link(o)
    o.location = (cx, cy, z0)
    return o


def tower_tiles():
    """The tower's tiles, named Tower tile 1 ... 7 left to right (shortest to longest) in their own
    collection, painted with the tile_color the modifier gives them. None when they are not in the file."""
    coll = bpy.data.collections.get(TOWER_TILES)
    if coll is None:
        objs = [bpy.data.objects.get(n) for n in TOWER_TILE_OBJECTS]
        if None in objs:
            print("tower: its tiles (Plane ... Plane.008) are not in the file")
            return None
        coll = hidden_collection(TOWER_TILES)
        for i, o in enumerate(sorted(objs, key=lambda o: o.matrix_world.translation.x)):
            move_to(o, coll)
            o.name = f"Tower tile {i + 1}"
    green = bpy.data.materials.get("Tower cap") or bpy.data.materials.get("Olive_green")
    base = (0.2, 0.3, 0.14, 1.0)
    if green is not None and green.node_tree is not None:
        bsdf = next((n for n in green.node_tree.nodes if n.type == "BSDF_PRINCIPLED"), None)
        if bsdf is not None:
            base = tuple(bsdf.inputs["Base Color"].default_value)
    mat = T.tiles_material("Tower tile (procedural)", fallback=base)
    for o in coll.objects:
        if list(o.data.materials) != [mat]:
            o.data.materials.clear()
            o.data.materials.append(mat)
    return coll, base


def underlay():
    m = bpy.data.materials.get(UNDERLAY)
    if m is None:
        m = bpy.data.materials.new(UNDERLAY)
        m.use_nodes = True
        bsdf = next(n for n in m.node_tree.nodes if n.type == "BSDF_PRINCIPLED")
        bsdf.inputs["Base Color"].default_value = (0.035, 0.05, 0.025, 1.0)
        bsdf.inputs["Roughness"].default_value = 0.9
    return m


def tile_tower(tiles, base):
    """Hide the hand-placed tower tiles and lay Cone Tiles over the tower's cone instead. A cone an
    older run laid (with Tile Cover) gets the current look once; after that, changes made on it are kept."""
    cone = bpy.data.objects.get(TOWER_TILED)
    if cone is None:
        old = hidden_collection(TOWER_OLD_TILES, viewport=False)
        for o in [o for o in bpy.data.objects if o.name.startswith(TOWER + " | Supplied tile row")]:
            move_to(o, old)
        cone = tower_cone()
    elif cone.get("tower_look", 0) >= TOWER_LOOK:
        return None
    for md in [md for md in cone.modifiers if md.type == "NODES"]:
        cone.modifiers.remove(md)
    md = cone.modifiers.new(T.CONE_NAME, "NODES")
    md.node_group = bpy.data.node_groups[T.CONE_NAME]
    G.set_inputs(md, {"Tiles": tiles, **TOWER_SETTINGS, "Base Color": base})
    cone.data.materials.clear()
    cone.data.materials.append(underlay())
    cone["tower_look"] = TOWER_LOOK
    print("tower: tiled its cone roof")
    return cone


RIDGE = "Main Roof | Ridge"
RIDGE_PIECES = "Ridge Pieces"
RIDGE_PIECE_OBJECTS = ["Roof.017", "Roof.019", "Roof.023"]    # the three caps placed by hand on the ridge
RIDGE_SHELF = (-7.9, -1.26, 2.14)    # where the pieces stand as models: right of the roof tiles, on the ground


def ridge_pieces():
    """The ridge caps placed by hand, named Ridge piece 1 ... 3 (as they lay along the ridge) in their
    own collection and moved beside the roof tiles. Returns (collection, how they sat on the ridge:
    centre height, average length, width and height), or None when they are not in the file."""
    coll = bpy.data.collections.get(RIDGE_PIECES)
    if coll is not None:
        return coll, None
    objs = [bpy.data.objects.get(n) for n in RIDGE_PIECE_OBJECTS]
    if None in objs:
        print("ridge: its pieces (Roof.017, Roof.019, Roof.023) are not in the file")
        return None
    boxes = [G.world_bbox([o]) for o in objs]
    n = len(objs)
    sat = {"z": sum((lo[2] + hi[2]) / 2 for lo, hi in boxes) / n,
           "length": sum(hi[1] - lo[1] for lo, hi in boxes) / n,
           "width": sum(hi[0] - lo[0] for lo, hi in boxes) / n,
           "height": sum(hi[2] - lo[2] for lo, hi in boxes) / n}
    coll = hidden_collection(RIDGE_PIECES)
    for i, (o, (lo, hi)) in enumerate(sorted(zip(objs, boxes), key=lambda t: t[1][0][1])):
        move_to(o, coll)
        o.name = f"Ridge piece {i + 1}"
        target = (RIDGE_SHELF[0] + 0.6 * i, RIDGE_SHELF[1], RIDGE_SHELF[2])
        o.location = (o.location[0] + target[0] - (lo[0] + hi[0]) / 2,
                      o.location[1] + target[1] - (lo[1] + hi[1]) / 2,
                      o.location[2] + target[2] - lo[2])
    return coll, sat


def ridge_row(pieces, sat):
    """A curve along the main roof's ridge with a Ridge Row modifier laying the ridge pieces end to end,
    cut at the chimney. Kept as it is when it is there already."""
    if bpy.data.objects.get(RIDGE) is not None or sat is None:
        return None
    lo, hi = G.world_bbox([bpy.data.objects[n] for n in SLABS])
    x = (lo[0] + hi[0]) / 2
    cu = bpy.data.curves.new(RIDGE, "CURVE")
    cu.dimensions = "3D"
    sp = cu.splines.new("POLY")
    sp.points.add(1)
    sp.points[0].co = (x, lo[1], sat["z"], 1.0)
    sp.points[1].co = (x, hi[1], sat["z"], 1.0)
    ob = bpy.data.objects.new(RIDGE, cu)
    for c in bpy.data.objects[SLABS[0]].users_collection:
        c.objects.link(ob)
    md = ob.modifiers.new(T.RIDGE_NAME, "NODES")
    md.node_group = bpy.data.node_groups[T.RIDGE_NAME]
    G.set_inputs(md, {"Pieces": pieces, "Exclude Objects": bpy.data.collections.get("Roof Cutters"),
                      "Piece Length": sat["length"], "Width": sat["width"], "Height": sat["height"]})
    print("ridge: laid the ridge pieces along", RIDGE)
    return ob


RIDGE_LOOK = 1      # bumped when the ridge's look changes: a ridge laid by an older run gets it once
RIDGE_MATERIAL = "Ridge piece (procedural)"


def ridge_colors(pieces):
    """The ridge pieces painted like the roof tiles: the roof tiles' material (reading the colour Ridge Row
    gives each piece) and the main roof's Tile Cover colours on Ridge Row. Once; later changes are kept."""
    ob = bpy.data.objects.get(RIDGE)
    if ob is None or ob.get("ridge_look", 0) >= RIDGE_LOOK:
        return
    roof = next(md for md, _ in T.modifier_settings(T.NAME) if md.id_data.name == SLABS[0])
    cols = dict(next(vals for md, vals in T.modifier_settings(T.NAME) if md == roof))
    look = {k: cols[k] for k in ("Base Color", "Accent Color", "Accent Chance", "Shade Variation")}
    mat = T.tiles_material(RIDGE_MATERIAL, fallback=look["Base Color"], attribute=B.COLOR_ATTRIBUTE)
    for o in pieces.objects:
        o.data.materials.clear()
        o.data.materials.append(mat)
    G.set_inputs(ob.modifiers[T.RIDGE_NAME], look)
    ob["ridge_look"] = RIDGE_LOOK
    print("ridge: painted like the roof", look)


# what earlier versions of this script named the tiles (from Cube, Cube.002 ...), in the new order
OLD_TILE_NAMES = ["Roof tile 000", "Roof tile 002", "Roof tile 003", "Roof tile 004", "Roof tile 006", "Roof tile 005"]


def update_open_file():
    renamed = []
    for i, n in enumerate(OLD_TILE_NAMES):
        o = bpy.data.objects.get(n)
        if o is not None:
            o.name = f"Roof tile {i + 1}"
            renamed.append(f"{n} -> {o.name}")
    saved = [(md, vals) for md, vals in T.rebuild_covers(cone=True, ridge=True) if md.node_group.name == T.NAME]
    print("renamed:", ", ".join(renamed) or "nothing")
    for md, vals in saved:
        print(" ", md.id_data.name, "kept:", {k: v for k, v in vals.items() if not k.startswith("Tile ")})
    roof = hidden_collection("Roof Tiles")
    B.label_slots(roof, "Tile", size=0.08)
    tower = tower_tiles()
    if tower is not None:
        B.label_slots(tower[0], "Tile", size=0.12)
        tile_tower(*tower)
    ridge = ridge_pieces()
    if ridge is not None:
        ridge_row(*ridge)
        ridge_colors(ridge[0])
        B.label_slots(ridge[0], "Piece", size=0.08)
        T.B.fit_slots(T.RIDGE_NAME, "Pieces", "Piece")


def main():
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    if os.path.exists(OUT) and "--force" not in argv:
        print(f"{OUT} exists: not replacing it (pass -- --force to replace it, or run this script "
              "from Blender's Text Editor with that file open to update it in place)")
        return
    tree = T.build()
    if tree.asset_data is None:
        tree.asset_mark()

    tiles = hidden_collection("Roof Tiles")
    mat = T.tiles_material()
    # named in that order, so the modifier's Tile 1 ... Tile 6 shares match them left to right
    for i, n in enumerate(TILE_OBJECTS):
        o = bpy.data.objects[n]
        move_to(o, tiles)
        o.name = f"Roof tile {i + 1}"
        o.data.materials.clear()
        o.data.materials.append(mat)

    cutters = hidden_collection("Roof Cutters")
    cut = chimney_cutter()
    cutters.objects.link(cut)

    for n in SLABS:
        slab = bpy.data.objects[n]
        face_outwards(slab)
        T.apply_skip(slab, [outer_face(slab)])
        md = slab.modifiers.new(T.NAME, "NODES")
        md.node_group = tree
        G.set_inputs(md, {"Tiles": tiles, "Exclude Objects": cutters, "Skip Faces": "attr:" + T.SKIP_ATTRIBUTE})
    bpy.ops.wm.save_as_mainfile(filepath=OUT)
    print("saved", OUT)
    for n in SLABS:
        slab = bpy.data.objects[n]
        md = slab.modifiers[T.NAME]
        r = G.check(md)
        print("slab", n, r["ok"], r["invalid_links"], r["warnings"], "tiles:", len(G.instance_transforms(slab)))


if __name__ == "__main__":
    if bpy.app.background:
        main()
    else:
        update_open_file()
