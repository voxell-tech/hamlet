"""Tidy the cottage file: its collections, the objects' names and the materials.

Run from Blender's Text Editor (nothing needs to be selected), look at the Outliner, then save (or
File > Save As to keep the untidy file too). Running it again changes nothing more. It:

* gives back to each generator its own parts collection: the collection Ridge Row lays is called
  "Ridge Pieces" and holds the ridge pieces only, Tile Cover's "Roof Tiles", and so on. Whatever was
  built from copies of those parts (the work table) is taken out of it, or the generator would lay it
  on the roof too;
* deletes what shows neither in the viewport nor in renders: the objects of switched-off collections
  (Lamp and Sign, Flower Pots, Ivy, ...) and objects hidden both ways (the hand-placed tower tiles,
  the chimney's old blocks once chimney_bricks_build.py has hidden them), but not the parts the
  generators read, nor anything still used by something that shows;
* gives what was built from copies of parts (a table from plinth stones and ridge pieces, still in
  their parts collections) copies of its own, under an Empty ("Bakery | Table"), so that moving it
  changes nothing else. The parts stay where they were, hidden in the viewport: the walls keep their
  stones and the vines their shape;
* sorts every object into one tree, the parts the generators lay kept apart and hidden in renders:

    Tower Cottage
      House            Walls and Timber, Roof and Chimney, Front Door, Windows
      Tower
      Climbing Vines
      Ground
      Bakery Corner    Oven, Work Table, Bread Sacks
    Parts              what Brick Cover, Tile Cover, Paving and the vines lay (hidden in renders)
      Plinth Stones, Chimney Bricks, Paving Slabs, Roof Tiles, Ridge Pieces, Tower Tiles, Vine Parts,
      Cutters (Roof Cutters, House plinth cutouts, Tower plinth cutouts), Labels

  and deletes the collections left empty, and the empties with nothing under them and nothing
  pointing at them;
* names the objects after what they are ("Empty" -> "Bakery | Table", "Brick04.003" under it ->
  "Bakery | Table | Leg 3", "Circle" -> "Tower | Wall", "frame.017" -> "Window | Front gable |
  Frame 15", ...);
* merges the materials that are copies of each other (same nodes and values), names every material
  plainly ("r" -> "Plaster") and deletes the materials nothing uses.

The collections, objects and materials the build scripts look up keep their names, or the scripts
were changed to the new ones ("Circle" -> "Tower | Wall" in cottage_plinth_build.py, ...).
"""
import re

import bpy
from mathutils import Matrix, Vector

# ---------------------------------------------------------------- collections

# (name, older names it may have, parent, hidden in renders); in Outliner order
COLLECTIONS = [
    ("Tower Cottage", [], None, False),
    ("House", [], "Tower Cottage", False),
    ("Walls and Timber", ["01 Walls and Timber"], "House", False),
    ("Roof and Chimney", ["03 Main Roof"], "House", False),
    ("Front Door", ["Door"], "House", False),
    ("Windows", [], "House", False),
    ("Lamp and Sign", ["06 Details"], "House", False),
    ("Tower", ["04 Tower"], "Tower Cottage", False),
    ("Plants", [], "Tower Cottage", False),
    ("Flower Pots", ["07 Plants"], "Plants", False),
    ("Ivy", [], "Plants", False),
    ("Climbing Vines", [], "Tower Cottage", False),
    ("Ground", [], "Tower Cottage", False),
    ("Bakery Corner", [], "Tower Cottage", False),
    ("Oven", [], "Bakery Corner", False),
    ("Work Table", [], "Bakery Corner", False),
    ("Bread Sacks", [], "Bakery Corner", False),
    ("Built from Parts", [], "Tower Cottage", False),
    ("Parts", [], None, True),
    ("Plinth Stones", [], "Parts", True),
    ("Chimney Bricks", [], "Parts", True),
    ("Paving Slabs", [], "Parts", True),
    ("Roof Tiles", [], "Parts", True),
    ("Ridge Pieces", [], "Parts", True),
    ("Tower Tiles", [], "Parts", True),
    ("Vine Parts", [], "Parts", True),
    ("Cutters", [], "Parts", True),
    ("Roof Cutters", [], "Cutters", True),
    ("House plinth cutouts", [], "Cutters", True),
    ("Tower plinth cutouts", [], "Cutters", True),
    ("Labels", [], "Parts", True),
    ("Old", [], "Parts", True),
    ("Tower tiles (hand placed)", [], "Old", True),
]
# kept even when empty: the build scripts look these up by name
NEEDED = {"Climbing Vines", "Plinth Stones", "Chimney Bricks", "Paving Slabs", "Roof Tiles", "Ridge Pieces",
          "Tower Tiles", "Vine Parts", "Roof Cutters", "House plinth cutouts", "Tower plinth cutouts",
          "Labels"}
# the cutters stay in their collections even where they hang under a door
CUTTERS = {"Roof Cutters", "House plinth cutouts", "Tower plinth cutouts"}
# the parts collections whose parts stand on a shelf away from the cottage, as models to edit
SHELVED = ["Plinth Stones", "Chimney Bricks", "Paving Slabs", "Roof Tiles", "Ridge Pieces", "Tower Tiles"]
# copies of parts used to build something, recognised by what they touch:
# (object they touch, their collection, the group's Empty, the name of a single copy)
BUILT_AT = [
    ("Bakery | Table board", "Work Table", "Bakery | Table", "Bakery | Table | Part"),
    ("Oven | Brick base", "Oven", "Oven | Hearth", "Oven | Hearth slab"),
    ("Oven base | Bricks", "Oven", "Oven | Hearth", "Oven | Hearth slab"),
]
TOUCH = 0.15                # metres: copies this close make one thing
# (node group, input): the collection of parts it lays
GENERATOR_PARTS = {("Ridge Row", "Pieces"): "Ridge Pieces", ("Tile Cover", "Tiles"): "Roof Tiles",
                   ("Cone Tiles", "Tiles"): "Tower Tiles", ("Paving", "Slabs"): "Paving Slabs"}


def collection(name, olds, hidden):
    c = bpy.data.collections.get(name)
    if c is None:
        old = next((bpy.data.collections[o] for o in olds
                    if o in bpy.data.collections and not bpy.data.collections[o].library), None)
        if old is not None:
            old.name = name
            c = old
        else:
            c = bpy.data.collections.new(name)
    if hidden:
        c.hide_render = True
    return c


def generator_inputs():
    """(modifier, input name, collection) for every collection a generator modifier lays."""
    for o in bpy.data.objects:
        for md in o.modifiers:
            if md.type != "NODES" or md.node_group is None:
                continue
            for it in md.node_group.interface.items_tree:
                if getattr(it, "in_out", None) == "INPUT" and it.socket_type == "NodeSocketCollection":
                    c = getattr(md.properties.inputs, it.identifier).value
                    if c is not None:
                        yield md, it.name, c


def give_back_parts():
    """Each generator's parts collection called by the name the build scripts know it by."""
    fixed = []
    for md, name, c in generator_inputs():
        want = GENERATOR_PARTS.get((md.node_group.name, name))
        if want is None or c.name == want:
            continue
        other = bpy.data.collections.get(want)
        if other is not None:
            other.name = want + " (old)"
        fixed.append(f"{c.name!r} -> {want!r}")
        c.name = want
    return fixed


def parents_of(c):
    scene = bpy.context.scene.collection
    out = [scene] if c.name in scene.children else []
    return out + [p for p in bpy.data.collections if c.name in p.children]


def build_tree():
    scene = bpy.context.scene.collection
    made = {}
    for name, olds, parent, hidden in COLLECTIONS:
        made[name] = collection(name, olds, hidden)
    order = {}
    for name, _, parent, _ in COLLECTIONS:
        order.setdefault(parent, []).append(made[name])
    # relink each parent's children in the listed order (the Outliner shows link order)
    for parent, kids in order.items():
        p = scene if parent is None else made[parent]
        for c in kids:
            for q in parents_of(c):
                q.children.unlink(c)
        others = [c for c in p.children if c not in kids]
        for c in others:
            p.children.unlink(c)
        for c in kids + others:
            p.children.link(c)
    return made


def move(obj, coll, with_children=True, force=False):
    """obj (and its children, but not the cutters among them) into coll only. A part a generator lays
    stays in its collection; something built from parts (it has children) does not."""
    if not force and not obj.children and any(c.name in NEEDED and c != coll for c in obj.users_collection):
        return
    todo =[obj] + (list(obj.children_recursive) if with_children else [])
    for o in todo:
        if o is not obj and any(c.name in CUTTERS for c in o.users_collection):
            continue
        if coll not in o.users_collection:
            coll.objects.link(o)
        for c in list(o.users_collection):
            if c != coll:
                c.objects.unlink(o)


def sort_objects(C):
    def top(prefix):
        return [o for o in bpy.data.objects if o.name.startswith(prefix) and not o.library and o.parent is None]

    for o in top("Timber | ") + top("Walls | "):
        move(o, C["Walls and Timber"])
    for o in top("Main Roof | ") + top("Chimney"):
        if not o.name.startswith("Chimney brick"):
            move(o, C["Roof and Chimney"])
    for o in top("Door | "):
        move(o, C["Front Door"])
    for o in top("Window | "):
        move(o, C["Windows"])
    for o in top("Lamp | ") + top("Sign | "):
        move(o, C["Lamp and Sign"])
    for o in top("Tower | "):
        if not any(c.name in NEEDED or c.name == "Tower tiles (hand placed)" for c in o.users_collection):
            move(o, C["Tower"])
    for o in top("Plant | "):
        move(o, C["Flower Pots"])
    for o in top("Ivy | "):
        move(o, C["Ivy"])
    for o in bpy.data.objects:
        if any(md.type == "NODES" and md.node_group and md.node_group.name == "Climbing Vine"
               for md in o.modifiers):
            move(o, C["Climbing Vines"], False)
    for o in top("Ground | "):
        move(o, C["Ground"])
    for o in top("Oven"):
        move(o, C["Oven"])
    for o in top("Bakery | Sack"):
        move(o, C["Bread Sacks"])
    for o in top("Bakery | "):
        if not o.name.startswith("Bakery | Sack"):
            move(o, C["Work Table"])
    for o in top("Chimney brick "):
        move(o, C["Chimney Bricks"])


def world_box(objs):
    lo, hi = Vector((1e9,) * 3), Vector((-1e9,) * 3)
    for o in objs:
        for c in o.bound_box:
            w = o.matrix_world @ Vector(c)
            lo, hi = Vector(map(min, lo, w)), Vector(map(max, hi, w))
    return lo, hi


def touch(a, b, gap):
    return all(a[0][i] - gap <= b[1][i] and b[0][i] - gap <= a[1][i] for i in range(3))


BUILT = "organize_built_into"   # set on a part whose copy was taken to build something


def own_copy(o):
    c = o.copy()
    if o.data is not None:
        c.data = o.data.copy()                           # its own mesh: editing it leaves the part alone
    return c


def separate_copies(C):
    """Copies of parts (a plinth stone, a ridge piece) used to build something at the cottage stay in
    the parts collection they were copied in (Shift+D keeps the collection): the generator lays them
    too, and moving them changes the walls, the roof and the vines climbing them.

    The thing gets copies of its own, under an Empty, out of the parts collection. The parts themselves
    stay in their collection exactly as they are (hidden in the viewport, where they overlap the copy):
    the walls keep their stones and the vines their shape, and moving the copy changes nothing else."""
    cottage = [o for o in C["Tower Cottage"].all_objects if o.type in ("MESH", "CURVE")]
    if not cottage:
        return []
    lo, hi = world_box(cottage)
    lo, hi = lo - Vector((0.5,) * 3), hi + Vector((0.5,) * 3)
    copies = []
    for name in SHELVED:
        c = bpy.data.collections.get(name)
        for o in (c.objects if c else []):
            centre = sum(world_box([o]), Vector()) / 2
            if (o.parent is None and not o.children and BUILT not in o
                    and all(lo[i] <= centre[i] <= hi[i] for i in range(3))):
                copies.append(o)
    groups = []                                          # touching copies make one thing
    for o in copies:
        box = world_box([o])
        near = [g for g in groups if any(touch(box, world_box([p]), TOUCH) for p in g)]
        groups = [g for g in groups if g not in near] + [[o] + [p for g in near for p in g]]
    done = []
    for k, group in enumerate(sorted(groups, key=lambda g: min(o.name for o in g))):
        box = world_box(group)
        place = next((b for b in BUILT_AT if bpy.data.objects.get(b[0]) is not None
                      and touch(box, world_box([bpy.data.objects[b[0]]]), TOUCH)), None)
        if place is not None:
            coll, empty_name, single = C[place[1]], place[2], place[3]
        else:
            coll, empty_name, single = C["Built from Parts"], f"Built from parts | {k + 1}", None
        empty = None
        if len(group) > 1 or single is None:
            empty = bpy.data.objects.get(empty_name)
            if empty is None:
                empty = bpy.data.objects.new(empty_name, None)
                empty.empty_display_size = 0.3
                empty.location = ((box[0].x + box[1].x) / 2, (box[0].y + box[1].y) / 2, box[0].z)
                coll.objects.link(empty)
                bpy.context.view_layer.update()
            if empty_name not in FAMILIES:
                FAMILIES.append(empty_name)
        for o in group:
            c = own_copy(o)
            coll.objects.link(c)
            if empty is not None:
                c.parent = empty
                c.matrix_parent_inverse = empty.matrix_world.inverted()
            else:
                c.name = single
            o[BUILT] = c.name
            o.hide_set(True)
        done.append(f"{empty_name} ({len(group)} parts)" if empty is not None else single)
    return done


def rendered_collections():
    """The collections whose objects render: switched on, not hidden in renders, and their parents too."""
    out = {bpy.context.scene.collection}

    def walk(lc):
        for ch in lc.children:
            if not ch.exclude and not ch.collection.hide_render:
                out.add(ch.collection)
                walk(ch)
    for vl in bpy.context.scene.view_layers:
        walk(vl.layer_collection)
    return out


def remove_unseen():
    """Delete what shows neither in the viewport nor in renders (the objects of a switched-off
    collection, objects hidden both ways), unless a generator reads it (the parts collections), it
    is used by something (a modifier, a constraint, a visible child) or it comes from a library."""
    rendered = rendered_collections()

    def shows(o):
        if o.visible_get():
            return True
        return not o.hide_render and any(c in rendered for c in o.users_collection)

    unseen = {o for o in bpy.data.objects if not o.library and not shows(o)
              and not any(c.name in NEEDED for c in o.users_collection)}
    users = bpy.data.user_map(subset=list(unseen))
    while True:                                          # keep what something kept still needs
        keep = {o for o in unseen
                if any(k not in unseen for k in o.children)
                or any(not isinstance(u, (bpy.types.Collection, bpy.types.Scene)) and u not in unseen
                       for u in users[o])}
        if not keep:
            break
        unseen -= keep
    names = sorted(o.name for o in unseen)
    for o in list(unseen):
        bpy.data.objects.remove(o)
    return names


def purge_orphans():
    """Meshes, curves, materials, images, ... nothing uses any more (not those kept on purpose)."""
    kinds = ["meshes", "curves", "materials", "images", "textures", "node_groups", "fonts", "lights",
             "cameras", "actions", "lattices", "metaballs"]
    gone = 0
    while True:
        dead = [(k, x) for k in kinds for x in getattr(bpy.data, k)
                if x.users == 0 and not x.use_fake_user and not x.library]
        if not dead:
            return gone
        for k, x in dead:
            getattr(bpy.data, k).remove(x)
        gone += len(dead)


def layer_collections(lc):
    yield lc
    for c in lc.children:
        yield from layer_collections(c)


def remember_switches():
    """Per view layer: the collections switched off (checkbox) or hidden (eye) in the Outliner, and the
    objects that are off because every collection they are in is. Relinking a collection resets its
    switches, so they are set back afterwards."""
    state, off = {}, {}
    for vl in bpy.context.scene.view_layers:
        lcs = list(layer_collections(vl.layer_collection))[1:]
        state[vl.name] = {lc.collection.as_pointer(): (lc.exclude, lc.hide_viewport) for lc in lcs}
        excluded = set()
        for lc in lcs:
            if lc.exclude:
                excluded |= {lc.collection, *lc.collection.children_recursive}
        off[vl.name] = {o for o in bpy.data.objects
                        if o.users_collection and all(c in excluded for c in o.users_collection)}
    return state, off


def restore_switches(state, off):
    for vl in bpy.context.scene.view_layers:
        for lc in list(layer_collections(vl.layer_collection))[1:]:
            c = lc.collection
            if c.as_pointer() in state[vl.name]:           # by address: some were deleted since
                lc.exclude, lc.hide_viewport = state[vl.name][c.as_pointer()]
            elif c.all_objects and all(o in off[vl.name] for o in c.all_objects):
                lc.exclude = True                        # new, and all that is in it was off


def drop_empty_collections():
    gone = []
    while True:                                          # a parent empties once its children go
        empty = [c for c in bpy.data.collections
                 if not c.library and c.name not in NEEDED and not c.objects and not c.children]
        if not empty:
            return gone
        for c in empty:
            gone.append(c.name)
            bpy.data.collections.remove(c)


def drop_bare_empties():
    """Empties with nothing under them, nothing instanced and nothing pointing at them."""
    bare = [o for o in bpy.data.objects if o.type == "EMPTY" and not o.library and not o.children
            and o.instance_type != "COLLECTION"]
    users = bpy.data.user_map(subset=bare)
    gone = []
    for o in bare:
        if all(isinstance(u, (bpy.types.Collection, bpy.types.Scene)) for u in users[o]):
            gone.append(o.name)
            bpy.data.objects.remove(o)
    return gone


# ---------------------------------------------------------------- object names

def kids(*bases):
    """Test: the object has children named like one of bases."""
    return lambda o: any(base(k.name) in bases for k in o.children)


def under(parent):
    """Test: the object hangs under parent."""
    return lambda o: o.parent is not None and o.parent.name == parent


def taller_than(h):
    return lambda o: o.dimensions.z > h


# (old name, new name, test the object must pass, if any); in order: parents before their parts
NAMES = [
    ("Circle", "Tower | Wall", None),
    ("Circle.001", "Tower | Roof base", None),
    ("01 Tower Structure | Plane Mesh | 001.001", "Tower | Eaves ring", None),
    ("Tower | Cone roof | Circle Mesh", "Tower | Cone roof | Rim", None),
    ("Tower | Cone roof | Sphere Mesh", "Tower | Cone roof | Ball", None),
    ("Tower | Cone roof | Flowing orange pennant", "Tower | Cone roof | Pennant", None),
    ("01 Tower Structure | Plane Mesh | 001", "Timber | Floor band | front", None),
    ("01 Tower Structure | Plane Mesh | 001.002", "Timber | Floor band | back", None),
    ("Timber | Gable post", "Timber | Gable post | left", None),
    ("Timber | Gable post.001", "Timber | Gable post | right", None),
    ("Timber | Jetty beam | front.001", "Timber | Jetty beam | back", None),
    ("Chimney | Clay pot", "Chimney | Clay pot 1", None),
    ("Chimney | Clay pot.001", "Chimney | Clay pot 2", None),
    ("Empty", "Door | Front door", kids("door")),
    ("Empty.003", "Door | Threshold", kids("Door 01 | Threshold Stone | 01")),
    ("Door 01 | Threshold Stone | 01", "Door | Threshold | Stone 1", None),
    ("Door 01 | Threshold Stone | 02", "Door | Threshold | Stone 2", None),
    ("Stair", "Plinth cutout | steps", None),
    ("Empty.001", "Window | Front gable", kids("frame")),
    ("Empty.002", "Window | Right side", kids("frame")),
    ("03 Chimney | Chimney ashlar | 001", "Chimney brick 1", None),
    ("03 Chimney | Chimney ashlar | 002", "Chimney brick 2", None),
    ("03 Chimney | Chimney ashlar | 003", "Chimney brick 3", None),
    ("03 Chimney | Chimney ashlar | 004", "Chimney brick 4", None),
    # the oven: the dome on its brick base
    ("Plane.001", "Oven | Brick base", None),
    ("Mball.001", "Oven | Dome", None),
    ("Mball.003", "Oven | Mouth arch", None),
    ("Cylinder", "Oven | Flue", None),
    ("Cylinder.001", "Oven | Flue cap", None),
    ("Ridge piece 3.002", "Oven | Log 1", None),
    ("Ridge piece 3.003", "Oven | Log 2", None),
    ("Ridge piece 3.004", "Oven | Log 3", None),
    ("Baguette.001", "Oven | Baguette 1", None),
    ("Baguette.002", "Oven | Baguette 2", None),
    ("Bakery | Baguette 1", "Oven | Baguette 1", under("Oven")),
    ("Bakery | Baguette 2", "Oven | Baguette 2", under("Oven")),
    ("Oven | Brick base", "Oven base | Bricks", under("Oven base")),
    ("Oven | Log 1", "Oven base | Log 1", under("Oven base")),
    ("Oven | Log 2", "Oven base | Log 2", under("Oven base")),
    ("Oven | Log 3", "Oven base | Log 3", under("Oven base")),
    ("Paving slab 1.001", "Oven base | Hearth slab", under("Oven base")),
    # the work table: brick legs and braces, ridge-piece planks
    ("Empty", "Bakery | Table", kids("Brick04", "Ridge piece 1")),
    ("Plane", "Bakery | Table board", None),
    ("Bakery | Table top", "Bakery | Table board", None),
    ("Sphere", "Bakery | Dough", None),
    ("Rolling Pin", "Bakery | Rolling pin", None),
    ("Flou_factory.001", "Bakery | Flour factory", None),
    ("Collection.015", "Bakery | Bun on the table", None),
    ("Empty.004", "Bakery | Sack of baguettes", None),
    ("Empty.005", "Bakery | Sack of buns", None),
]
# the parts of a door, window, sack or table, named after their parent: "frame.017" under
# "Window | Front gable" becomes "Window | Front gable | Frame 15"
PART_NAMES = {
    "door": "Leaf", "Door_brass_ring": "Brass ring", "Door_strap": "Strap",
    "Iron_lantern_frame": "Lantern frame", "Lantern_cap": "Lantern cap", "Scroll_hook": "Lantern hook",
    "Warm_lantern": "Lantern glass", "stones": "Stone", "frame": "Frame", "panes": "Panes",
    "flower_box": "Flower box", "spice_sack": "Sack", "Collection": "Bun",
    "Ridge piece 1": "Plank", "Ridge piece 2": "Plank", "Ridge piece 3": "Plank",
}
FAMILIES = ["Door | Front door", "Window | Front gable", "Window | Right side",
            "Bakery | Sack of baguettes", "Bakery | Sack of buns", "Bakery | Table"]


def rename(o, new):
    if o.name != new:
        o.name = new
        return 1
    return 0


def base(name):
    return re.sub(r"(_\d+)?(\.\d+)?$", "", name)


def table_part(o):
    """The work table's bricks: tall ones are legs, the thinner ones between them braces."""
    return "Leg" if o.dimensions.z > 0.6 else "Brace"


def name_vines():
    """"Vine | House 1", "Vine | Tower 2", ...: after the wall they climb, numbered left to right."""
    where = {}
    for o in bpy.data.objects:
        md = next((m for m in o.modifiers if m.type == "NODES" and m.node_group
                   and m.node_group.name == "Climbing Vine"), None)
        if md is None:
            continue
        wall = next((getattr(md.properties.inputs, it.identifier).value
                     for it in md.node_group.interface.items_tree
                     if getattr(it, "in_out", None) == "INPUT" and it.name == "Wall"), None)
        wname = wall.name if wall is not None else ""
        side = "Tower" if wname.startswith(("Tower", "Circle")) else "House" if wname.startswith("Walls") else "Wall"
        where.setdefault(side, []).append(o)
    todo = {}
    for side, vines in where.items():
        vines.sort(key=lambda o: (round(o.matrix_world.translation.x, 1), o.name))
        todo.update({o: f"Vine | {side} {i + 1}" for i, o in enumerate(vines) if o.name != f"Vine | {side} {i + 1}"})
    for o in todo:                                       # free the names first: two vines may swap
        o.name = "_vine_ " + o.name
    for o, new in todo.items():
        o.name = new
    return len(todo)


def name_objects():
    n = 0
    for old, new, test in NAMES:
        o = bpy.data.objects.get(old)
        if o is not None and not o.library and (test is None or test(o)):
            n += rename(o, new)
    for fam in FAMILIES:
        parent = bpy.data.objects.get(fam)
        if parent is None:
            continue
        groups = {}
        for o in sorted(parent.children, key=lambda o: o.name):
            b = base(o.name)
            if o.instance_type == "COLLECTION" and o.instance_collection is not None:
                b = "Collection"
            if fam == "Bakery | Table" and b.startswith("Brick"):
                label = table_part(o)
            elif b in PART_NAMES:
                label = PART_NAMES[b]
            elif fam.startswith("Built from parts"):
                label = b
            else:
                continue
            if fam == "Bakery | Sack of baguettes" and label == "Bun":
                label = "Baguette"
            groups.setdefault(label, []).append(o)
        for label, objs in groups.items():
            if len(objs) > 1:                            # numbered left to right, front to back
                objs.sort(key=lambda o: (round(o.matrix_world.translation.x, 2),
                                         round(o.matrix_world.translation.y, 2), o.name))
            width = 2 if len(objs) > 9 else 1
            for i, o in enumerate(objs):
                n += rename(o, f"{fam} | {label}" + (f" {i + 1:0{width}d}" if len(objs) > 1 else ""))
    return n


# ---------------------------------------------------------------- materials

# (new name, the old names it takes over); copies of each other are merged into one
MATERIALS = [
    ("Plaster", ["r"]),
    ("Timber", ["Brown"]),
    ("Dark timber", ["Brown.001", "Material.001", "Material.002"]),
    ("Door wood", ["Door_Wood.001"]),
    ("Wrought iron", ["Charcoal Metal", "Forged_Iron.001", "Forged_iron.001", "Forged_iron.002"]),
    ("Aged brass", ["Aged_Brass.001"]),
    ("Gold", ["Gold"]),
    ("Amber glass", ["Amber_Glass", "Amber_Glass.002"]),
    ("Lantern glass", ["Warm Amber Glass"]),
    ("Sandstone", ["Sandstone_Brick", "Sandstone_Brick.003"]),
    ("Threshold stone", ["Stair Brick"]),
    ("Beige stone", ["Warm Beige"]),
    ("Chimney brick", ["Warm Beige.001"]),
    ("Terracotta", ["Aged_terracotta.001", "Brown.002"]),
    ("Tower cap", ["Olive_green"]),
    ("Tower tile (hand placed)", ["Olive_green.001"]),
    ("Sign green", ["Hip roof moss green 00.001"]),
    ("Window paint (vertex colours)", ["Vertex_Color_Surface"]),
    ("Plant stem", ["Ivy_0.001"]),
    ("Plant leaf", ["Ivy_2.001"]),
    ("Petal cream", ["Petal_cream.001"]),
    ("Petal lavender", ["Petal_lavender.001"]),
    ("Petal pink", ["Petal_pink.001"]),
    ("Petal white", ["Petal_white.001"]),
    ("Table wood", ["Material"]),
    ("Dough", ["yellow"]),
]


def signature(m):
    """What the material looks like: its nodes, their values and links."""
    if m.node_tree is None:
        return ("flat", tuple(round(x, 3) for x in m.diffuse_color))
    nodes = []
    for nd in sorted(m.node_tree.nodes, key=lambda nd: (nd.bl_idname, nd.name)):
        vals = []
        for s in nd.inputs:
            if hasattr(s, "default_value") and not s.is_linked:
                v = s.default_value
                try:
                    v = tuple(round(x, 3) for x in v)
                except TypeError:
                    v = round(v, 3) if isinstance(v, float) else v
                vals.append((s.identifier, v))
        img = getattr(nd, "image", None)
        nodes.append((nd.bl_idname, tuple(vals), img.name if img else None))
    links = sorted((lk.from_node.bl_idname, lk.from_socket.identifier, lk.to_node.bl_idname,
                    lk.to_socket.identifier) for lk in m.node_tree.links)
    return tuple(nodes), tuple(links)


def tidy_materials():
    merged, renamed = [], 0
    for new, olds in MATERIALS:
        mats = [bpy.data.materials.get(n) for n in [new] + olds]
        mats = [m for i, m in enumerate(mats) if m is not None and not m.library and m not in mats[:i]]
        if not mats:
            continue
        keep = max(mats, key=lambda m: (m.name == new, m.users))      # the most used one stays
        for m in mats:
            if m is keep:
                continue
            if signature(m) != signature(keep):              # changed since: keep both
                print(f"materials: {m.name!r} differs from {keep.name!r} now, kept apart")
                continue
            m.user_remap(keep)
            merged.append(m.name)
            bpy.data.materials.remove(m)
        if keep.name != new:
            keep.name = new
            renamed += 1
    unused = [m.name for m in bpy.data.materials if m.users == 0 and not m.use_fake_user and not m.library]
    for n in unused:
        bpy.data.materials.remove(bpy.data.materials[n])
    return merged, renamed, unused


def summary(names):
    """"Plant | Pot by door (141), ...": names grouped by what comes before their last " | "."""
    groups = {}
    for n in names:
        key = n.rsplit(" | ", 1)[0] if " | " in n else base(n)
        groups[key] = groups.get(key, 0) + 1
    return ", ".join(f"{k} ({v})" if v > 1 else k for k, v in sorted(groups.items()))


def main():
    state, off = remember_switches()
    removed = remove_unseen()
    had = {c.name for c in bpy.data.collections}
    fixed = give_back_parts()
    n = name_objects()
    C = build_tree()
    sort_objects(C)
    built = separate_copies(C)
    n += name_objects() + name_vines()
    bare = drop_bare_empties()
    gone = [g for g in drop_empty_collections() if g in had]     # not the ones made just now
    restore_switches(state, off)
    merged, renamed, unused = tidy_materials()
    orphans = purge_orphans()
    loose = [o.name for o in bpy.context.scene.collection.objects]
    if removed:
        print(f"organize: removed {len(removed)} objects that showed nowhere: {summary(removed)}")
    if fixed:
        print(f"organize: parts collections named back: {fixed}")
    if built:
        print(f"organize: built from copies of parts (the parts left as they were, hidden): {built}")
    print(f"organize: {n} objects named, bare empties removed: {bare}, empty collections removed: {gone}")
    print(f"organize: {renamed} materials named, merged into others: {merged}, unused removed: {unused}")
    print(f"organize: {orphans} other unused data-blocks removed (meshes, images, ...)")
    for md, name, c in generator_inputs():                # a generator laying something odd
        if c.name in CUTTERS:
            continue
        odd = [o.name for o in c.objects if o.parent is not None]
        if odd:
            print(f"organize: {md.id_data.name!r} lays {c.name!r}, which holds parts of an assembly: {odd}")
    if loose:
        print(f"organize: still loose in the Scene Collection: {loose}")


if __name__ == "__main__":
    main()
