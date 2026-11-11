"""Lay the mayor house's procedural plinth stones around the agent house's walls.

The agent house's two wall blocks ("Walls | Ground floor plaster.001", the main house, and ".002",
the wing) already carry a Brick Cover modifier, but with no node group in it. This script fills it
with the mayor house's ground floor settings: a band of stones along the foot of the walls and
corner stones (quoins) up every corner, laid from the same stones (Parts > Plinth Stones).

Where the two blocks meet, each block's stones would run inside the other. A hidden copy of each
block, in Parts > Cutters, cuts the other's courses there (Brick Cover's Exclude Objects), so the
stones stop flush against the wall they meet.

The walls were scaled unevenly (1.019, 1.084, 1.084), which would stretch the stones; the scale is
applied to their meshes first, so they keep their shape and the stones come out the mayor house's
size.

Run it from Blender's Text Editor (Run Script). Running it again updates the settings.
"""

import bpy
from mathutils import Matrix

# wall: (seed, the other wall, whose copy cuts this wall's stones)
WALLS = {
    "Walls | Ground floor plaster.001": (56, "Walls | Ground floor plaster.002"),
    "Walls | Ground floor plaster.002": (31, "Walls | Ground floor plaster.001"),
}

# the mayor house's "Walls | Ground floor plaster" Brick Cover settings
BRICK_COVER = {
    "Brick 1": 79.05, "Brick 2": 100.0, "Brick 3": 368.77, "Brick 4": 52.57, "Brick 5": 171.15,
    "Brick 6": 103.95, "Brick 7": 111.86, "Brick 8": 100.0, "Brick 9": 100.0, "Brick 10": 100.0,
    "Course Height": 0.36, "Brick Length": 0.63, "Length Variation": 0.256, "Depth": 0.19,
    "Mortar Gap": 0.0, "Surface Offset": 0.0, "Rotation Jitter": 0.0, "Depth Jitter": 0.015,
    "Band Height": 0.75, "Corner Stones": True, "Corner Long": 0.77, "Corner Short": 0.58,
    "Corner Variation": 0.0569, "Corner Height": 2.9, "Corner Offset": -0.09,
    "Base Color": (0.533, 0.386, 0.238, 1.0), "Accent Color": (0.71, 0.539, 0.367, 1.0),
    "Accent Chance": 0.2447, "Shade Variation": 0.3174,
    "Only Faces": True, "Keep Inside": False, "Cap Angle": 0.7854, "Smooth Angle": 0.5236,
    "Flip Sides": False,
}


def modifier_input(md, name):
    item = next((it for it in md.node_group.interface.items_tree
                 if getattr(it, "in_out", None) == "INPUT" and it.name == name), None)
    if item is None:
        raise KeyError(f"{md.node_group.name} has no input {name!r}")
    return getattr(md.properties.inputs, item.identifier)


def collection(name, parent):
    c = bpy.data.collections.get(name)
    if c is None:
        c = bpy.data.collections.new(name)
        parent.children.link(c)
    return c


def apply_scale(obj):
    """The object's scale into its mesh: the same shape, scale 1."""
    if tuple(obj.scale) == (1.0, 1.0, 1.0):
        return
    if obj.data.users > 1:
        obj.data = obj.data.copy()
    obj.data.transform(Matrix.Diagonal((*obj.scale, 1.0)))
    obj.scale = (1.0, 1.0, 1.0)


def cutter(wall, coll):
    """A hidden copy of wall's block (no modifiers) that cuts the other wall's stones."""
    name = "Plinth cutout | " + wall.name.split(" | ")[-1]
    c = bpy.data.objects.get(name)
    if c is None:
        c = bpy.data.objects.new(name, wall.data.copy())
        coll.objects.link(c)
    else:
        old = c.data
        c.data = wall.data.copy()
        if old.users == 0:
            bpy.data.meshes.remove(old)
    c.data.name = name
    c.matrix_world = wall.matrix_world.copy()
    try:
        c.hide_set(True)
    except RuntimeError:                             # not in the view layer: hidden anyway
        pass
    c.hide_render = True
    c.display_type = "WIRE"
    return c


def main():
    cutters = collection("Cutters", bpy.data.collections["Parts"])
    stones = bpy.data.collections["Plinth Stones"]
    for name in WALLS:
        apply_scale(bpy.data.objects[name])
    for name, (seed, other) in WALLS.items():
        wall = bpy.data.objects[name]
        cuts = collection(f"Agent plinth cutouts | {name.split(' | ')[-1]}", cutters)
        cutter(bpy.data.objects[other], cuts)

        md = next((m for m in wall.modifiers if m.type == "NODES" and m.name == "Brick Cover"), None)
        if md is None:
            md = wall.modifiers.new("Brick Cover", "NODES")
            wall.modifiers.move(len(wall.modifiers) - 1, 0)
        md.node_group = bpy.data.node_groups["Brick Cover"]
        values = dict(BRICK_COVER, Bricks=stones, Seed=seed)
        values["Exclude Objects"] = cuts
        for k, v in values.items():
            modifier_input(md, k).value = v
        skip = modifier_input(md, "Skip Faces")          # as on the mayor house: not the top and
        skip.type = "ATTRIBUTE"                          # bottom faces, marked no_bricks
        skip.attribute_name = "no_bricks"
        print(f"agent plinth: {name} (seed {seed}), cut by {other}")


if __name__ == "__main__":
    main()
