"""Export the cottage's modular parts, each to a .glb of its own, and the house without them.

The parts (window, door, lamp, oven, bread sacks, table, chimney) are collections linked from
../modular-assets and shown in the cottage by instancing empties. The objects of each, those of
the collections it instances in turn too, are baked as export_glb.py bakes the house, and
exported in the part's own space: its origin the empty's, turned back as it was modelled, at the
scale the cottage shows it at. The house is
every visible object of the cottage of its own, which leaves the instanced parts out.

Run it headless, from this folder:
    blender -b cottage_tower_roof_tiles_v5.blend --python export_parts.py
or with only some of them, by name:
    blender -b cottage_tower_roof_tiles_v5.blend --python export_parts.py -- door lamp
The files go to ../glb-assets. Nothing is saved to the .blend.
"""

import os
import sys

import bpy
from mathutils import Matrix

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import export_glb  # noqa: E402

OUT = "//../glb-assets"
HOUSE = "mayor_house"
# the file of each part, and the empties showing it in the cottage, its origin the first one's
PARTS = {
    "window": ["Window Front", "Flower box Front"],
    "door": ["Door"],
    "lamp": ["Side_Lamp"],
    "oven": ["Oven"],
    "stack_of_bread": ["SacksWithBread"],
    "table": ["Table"],
    "chimney": ["03 Chimney"],
}


def asked():
    args = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv[1:] else []
    return args or [HOUSE, *PARTS]


def shown(empty, above=None):
    """Each object the collection instance empty shows, with the matrix that puts it in the
    scene, through the collections it instances in turn. Making the instances real instead would
    make an object of every instance a generator lays too, each laying them all again."""
    placed = (above or empty.matrix_world) @ Matrix.Translation(-empty.instance_collection.instance_offset)

    def walk(collection):
        for o in collection.objects:
            if o.hide_render:
                continue
            if o.instance_type == "COLLECTION" and o.instance_collection:
                yield from shown(o, placed @ o.matrix_world)
            elif o.type in ("MESH", "CURVE", "FONT", "SURFACE", "META"):
                yield o, placed @ o.matrix_world
        for child in collection.children:
            if not child.hide_render:
                yield from walk(child)

    yield from walk(empty.instance_collection)


def own_space(empty):
    """What moves the scene into the part's own space: the empty's place and turn undone, its
    scale kept."""
    at, turn, _ = empty.matrix_world.decompose()
    return (Matrix.Translation(at) @ turn.to_matrix().to_4x4()).inverted()


def main():
    out = bpy.path.abspath(OUT)
    names = asked()
    if HOUSE in names:
        root = bpy.data.collections[export_glb.ROOT]
        sources = [o for o in root.all_objects if export_glb.exportable(o)]
        export_glb.export(sources, os.path.join(out, HOUSE + ".glb"))
    for name in names:
        if name == HOUSE:
            continue
        empties = [bpy.data.objects[e] for e in PARTS[name]]
        sources = [s for e in empties for s in shown(e)]
        print(f"export_parts: {name}: {len(sources)} objects, scale {empties[0].scale[:]}")
        export_glb.export(sources, os.path.join(out, name + ".glb"), own_space(empties[0]))


if __name__ == "__main__":
    main()
