"""Lay the mayor house's procedural roof tiles on the agent house's two roofs.

The agent house's roofs ("Main Roof | Barge board | front left.001", the main roof, and ".002",
the wing) are the mayor house's barge board, mirrored to make both slopes. This script gives each
the mayor house's Tile Cover modifier, with its settings, after the Mirror (so the tiles cover both
slopes and are not mirrored copies of one another), and a ridge along its top: a curve with the
mayor house's Ridge Row modifier, laying the same ridge pieces.

The tiles, ridge pieces and their materials are the ones already in this file (Parts > Roof Tiles,
Parts > Ridge Pieces), copied from the mayor house: edit them there and both roofs follow.

The roofs are scaled by 1.084, so every length is divided by their scale: the tiles come out the
mayor house's size.

Run it from Blender's Text Editor (Run Script). Running it again updates the settings instead of
adding a second modifier or ridge.
"""

import bpy
from mathutils import Vector

# roof object: (name of its ridge, seed of its tiles, seed of its ridge)
ROOFS = {
    "Main Roof | Barge board | front left.001": ("Main Roof | Ridge | main", 4, 117),
    "Main Roof | Barge board | front left.002": ("Main Roof | Ridge | wing", 25, 58),
}

# the mayor house's "Main Roof | Barge board | front left" Tile Cover settings
TILE_COVER = {
    "Tile 1": 705.357, "Tile 2": 517.857, "Tile 3": 42.878, "Tile 4": 285.714, "Tile 5": 267.857,
    "Tile 6": 44.643, "Tile 7": 100.0, "Tile 8": 100.0, "Tile 9": 100.0, "Tile 10": 100.0,
    "Course Height": 0.93, "Tile Width": 0.81, "Tile Length": 0.96, "Thickness": 0.14, "Gap": 0.0,
    "Surface Offset": -0.03, "Top Scale": 1.0, "Pick By Size": False, "Tilt": 0.0178,
    "Eave Overhang": 0.03, "Width Variation": 0.277, "Rotation Jitter": 0.0436, "Depth Jitter": 0.008,
    "Base Color": (0.188, 0.105, 0.138, 1.0), "Accent Color": (0.178, 0.25, 0.072, 1.0),
    "Accent Chance": 0.1289, "Shade Variation": 0.384,
    "Keep Inside": False, "Cap Angle": 0.7854, "Smooth Angle": 0.5236,
}
TILE_LENGTHS = ["Course Height", "Tile Width", "Tile Length", "Thickness", "Gap", "Surface Offset",
                "Eave Overhang", "Depth Jitter"]

# the mayor house's "Main Roof | Ridge" Ridge Row settings
RIDGE_ROW = {
    "Piece 1": 906.76, "Piece 2": 0.0, "Piece 3": 337.038, "Piece 4": 100.0, "Piece 5": 100.0,
    "Piece 6": 100.0, "Piece 7": 100.0, "Piece 8": 100.0, "Piece 9": 100.0, "Piece 10": 100.0,
    "Piece Length": 0.7531, "Length Variation": 0.1642, "Width": 0.336, "Height": 0.1765, "Gap": 0.0,
    "Offset": 0.0, "Flip Chance": 0.2806, "Rotation Jitter": 0.0422,
    "Base Color": (0.188, 0.105, 0.138, 1.0), "Accent Color": (0.178, 0.25, 0.072, 1.0),
    "Accent Chance": 0.1289, "Shade Variation": 0.384,
}
# on the mayor house the ridge runs 0.059 above the barge board's top, 0.063 past its front end
# and 0.034 past its back end
RIDGE_ABOVE, RIDGE_FRONT, RIDGE_BACK = 0.059, 0.063, 0.034


def modifier_input(md, name):
    item = next((it for it in md.node_group.interface.items_tree
                 if getattr(it, "in_out", None) == "INPUT" and it.name == name), None)
    if item is None:
        raise KeyError(f"{md.node_group.name} has no input {name!r}")
    return getattr(md.properties.inputs, item.identifier)


def set_inputs(md, values):
    for name, value in values.items():
        modifier_input(md, name).value = value


def nodes_modifier(obj, group, before=None):
    """obj's modifier running group (added if missing), placed before the modifier named before."""
    md = next((m for m in obj.modifiers if m.type == "NODES" and m.node_group
               and m.node_group.name == group), None)
    if md is None:
        md = obj.modifiers.new(group, "NODES")
        md.node_group = bpy.data.node_groups[group]
    if before is not None and before in obj.modifiers:
        here = list(obj.modifiers).index(md)
        there = list(obj.modifiers).index(obj.modifiers[before])
        if here > there:
            obj.modifiers.move(here, there)
    return md


def ridge_curve(name, roof, collection):
    """A straight curve along the top of the roof, as on the mayor house, in world space."""
    vs = [v.co for v in roof.data.vertices]
    scale = roof.scale.x
    top = max(v.z for v in vs) + RIDGE_ABOVE / scale
    x = 0.0                                              # the Mirror's plane: the apex
    a = roof.matrix_world @ Vector((x, min(v.y for v in vs) - RIDGE_FRONT / scale, top))
    b = roof.matrix_world @ Vector((x, max(v.y for v in vs) + RIDGE_BACK / scale, top))
    obj = bpy.data.objects.get(name)
    if obj is None:
        cu = bpy.data.curves.new(name, "CURVE")
        cu.dimensions = "3D"
        obj = bpy.data.objects.new(name, cu)
        collection.objects.link(obj)
    cu = obj.data
    cu.splines.clear()
    sp = cu.splines.new("POLY")
    sp.points.add(1)
    sp.points[0].co = (*a, 1.0)
    sp.points[1].co = (*b, 1.0)
    obj.matrix_world = obj.matrix_world.Identity(4)
    return obj


def main():
    tiles = bpy.data.collections["Roof Tiles"]
    pieces = bpy.data.collections["Ridge Pieces"]
    cutters = bpy.data.collections.get("Roof Cutters")
    for roof_name, (ridge_name, tile_seed, ridge_seed) in ROOFS.items():
        roof = bpy.data.objects[roof_name]
        scale = roof.scale.x
        md = nodes_modifier(roof, "Tile Cover", before="Smooth by Angle")
        values = dict(TILE_COVER, Tiles=tiles, Seed=tile_seed)
        values["Exclude Objects"] = cutters
        for k in TILE_LENGTHS:
            values[k] = TILE_COVER[k] / scale
        set_inputs(md, values)
        skip = modifier_input(md, "Skip Faces")          # as on the mayor house: the faces marked
        skip.type = "ATTRIBUTE"                          # no_tiles (all but the top of the slope)
        skip.attribute_name = "no_tiles"

        ridge = ridge_curve(ridge_name, roof, roof.users_collection[0])
        md = nodes_modifier(ridge, "Ridge Row")
        set_inputs(md, dict(RIDGE_ROW, Pieces=pieces, Seed=ridge_seed, **{"Exclude Objects": cutters}))
        print(f"agent roof tiles: {roof_name} tiled (seed {tile_seed}), ridge {ridge_name}")


if __name__ == "__main__":
    main()
