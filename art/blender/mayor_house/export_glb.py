"""Export the whole cottage (the "Tower Cottage" collection) to glb_assets/mayor_house_full.glb.

glTF knows nothing of geometry nodes nor of procedural shaders, so before exporting this script:

* evaluates every visible object of the cottage with its modifiers and realises the instances the
  generators lay (Brick Cover, Tile Cover, Ridge Row, Cone Tiles, Paving, Climbing Vine), so every
  stone, tile and leaf becomes real geometry in place;
* bakes the colour of the procedural materials (the per-tile and per-stone colours, the noise, the
  vine colours) into a colour attribute with Cycles, and gives those faces a plain material reading
  it, which glTF carries as vertex colours (COLOR_0);
* gives each tile and stone, a piece of mesh of its own, two random numbers, drawn by the piece's
  number with the seeds the generator draws its accent and its shade with, in a second UV map
  (TEXCOORD_1), so the engine's shader can colour the tiles itself; the faces of the part's own
  mesh read (-1, -1). The generators keep no id of a tile, so the numbers are not the ones Blender
  drew;
* exports only those baked copies, then deletes them: the scene is left as it was.

Run it from Blender's Text Editor (Run Script), or headless:
    blender -b cottage_tower_roof_tiles_v4.blend --python export_glb.py -- /abs/out.glb
To export a part only, name it after the path: objects whose name starts with one of the words, or
the objects of a collection of that name, e.g. the main roof (its tiles, barge boards and ridge):
    blender -b cottage_tower_roof_tiles_v4.blend --python export_glb.py -- /abs/roof.glb "Main Roof"
or set ONLY below and Run Script.
Add --split to get one object, with one material, per part (SPLIT below) instead of one per object:
    ... -- /abs/roof.glb "Main Roof" --split    ->  "Main Roof | Wood" and "Main Roof | Tiles"
"""

import os
import sys

import bpy

ROOT = "Tower Cottage"
OUT = "glb_assets/mayor_house_full.glb"      # next to the .blend
ONLY = []          # e.g. ["Main Roof"] (and OUT = "glb_assets/main_roof.glb"): only these objects
BAKE_LAYER = "Baked color"
TAG = "export_glb_temp"
COLOR_ATTRS = ["tile_color", "brick_color"]  # what the generators colour each part with
TILE_RANDOM = "Tile random"                  # the UV map of each tile's random numbers
ACCENT_SEED, SHADE_SEED = 4, 5               # what the generators add to their seed for each
SPLIT_ON = False   # or --split: one object and one material per part below, named "<ONLY> | <part>"
SPLIT = {          # part: the materials it is made of; any other material makes a part of its own
    "Wood": ["Timber", "Dark timber"],
    "Tiles": ["Roof tile (procedural)", "Ridge piece (procedural)", "Tower tile (procedural)"],
}


def cli_args():
    args = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv[1:] else []
    return [a for a in args if a != "--split"]


def split_on():
    return SPLIT_ON or "--split" in sys.argv


def out_path():
    args = cli_args()
    return args[0] if args else bpy.path.abspath("//" + OUT)


def wanted(o):
    only = cli_args()[1:] or ONLY
    if not only:
        return True
    return any(o.name.startswith(w) for w in only) or any(
        c.name in only or (bpy.data.collections.get(w) and o.name in bpy.data.collections[w].all_objects)
        for c in o.users_collection for w in only)


def is_procedural(mat):
    """A material whose base colour comes from nodes glTF cannot carry (attributes, noise)."""
    if mat is None or not mat.use_nodes:
        return False
    bsdf = next((n for n in mat.node_tree.nodes if n.type == "BSDF_PRINCIPLED"), None)
    if bsdf is None or not bsdf.inputs["Base Color"].is_linked:
        return False
    src = bsdf.inputs["Base Color"].links[0].from_node
    return src.type != "VERTEX_COLOR"            # a plain colour attribute travels as it is


def generator_seed(src):
    """The Seed of src's first geometry nodes modifier that has one, else 0."""
    for m in src.modifiers:
        if m.type != "NODES" or not m.node_group:
            continue
        ids = {i.name: i.identifier for i in m.node_group.interface.items_tree
               if i.item_type == "SOCKET" and i.in_out == "INPUT"}
        if "Seed" not in ids:
            continue
        props = getattr(m, "properties", None)       # Blender 5
        if props is not None:
            value = getattr(props.inputs, ids["Seed"])
            return int(getattr(value, "value", value))
        return int(m[ids["Seed"]])                   # Blender 4
    return 0


def tile_random(N, L, geo, seed):
    """geo, realised, with two random numbers for each piece of mesh a generator coloured, by the
    piece's number, in TILE_RANDOM, with the seeds its generator draws the accent and the shade
    with; (-1, -1) at every other corner."""
    coloured = None
    for name in COLOR_ATTRS:
        colour = N("GeometryNodeInputNamedAttribute")
        colour.data_type = "FLOAT_COLOR"
        colour.inputs["Name"].default_value = name
        length = N("ShaderNodeVectorMath")
        length.operation = "LENGTH"
        L(next(o for o in colour.outputs if o.name == "Attribute" and o.enabled), length.inputs[0])
        has = N("ShaderNodeMath")
        has.operation = "GREATER_THAN"
        has.inputs[1].default_value = 0.0001
        L(length.outputs["Value"], has.inputs[0])
        if coloured is None:
            coloured = has.outputs[0]
        else:
            either = N("ShaderNodeMath")
            either.operation = "MAXIMUM"
            L(coloured, either.inputs[0])
            L(has.outputs[0], either.inputs[1])
            coloured = either.outputs[0]
    island = N("GeometryNodeInputMeshIsland")
    combine = N("ShaderNodeCombineXYZ")
    for axis, offset in (("X", ACCENT_SEED), ("Y", SHADE_SEED)):
        rand = N("FunctionNodeRandomValue")
        rand.data_type = "FLOAT"
        L(island.outputs["Island Index"], next(i for i in rand.inputs if i.name == "ID"))
        next(i for i in rand.inputs if i.name == "Seed").default_value = seed + offset
        L(next(o for o in rand.outputs if o.enabled), combine.inputs[axis])
    pick = N("GeometryNodeSwitch")
    pick.input_type = "VECTOR"
    L(coloured, pick.inputs["Switch"])
    pick.inputs["False"].default_value = (-1.0, -1.0, 0.0)
    L(combine.outputs[0], pick.inputs["True"])
    put = N("GeometryNodeStoreNamedAttribute")
    put.data_type, put.domain = "FLOAT2", "CORNER"
    put.inputs["Name"].default_value = TILE_RANDOM
    L(geo, put.inputs["Geometry"])
    L(pick.outputs[0], next(i for i in put.inputs if i.name == "Value" and i.enabled))
    return put.outputs["Geometry"]


def keep_tile_random_second(me):
    """Puts TILE_RANDOM after the mesh's own UV map, so it travels as TEXCOORD_1, or drops it
    when no tile reads it."""
    layer = me.uv_layers.get(TILE_RANDOM)
    if layer is None:
        return
    data = [0.0] * (2 * len(me.loops))
    layer.data.foreach_get("uv", data)
    me.uv_layers.remove(layer)
    if max(data, default=-1.0) < 0.0:            # no tiles on it
        return
    if not me.uv_layers:
        me.uv_layers.new(name="UVMap")
    me.uv_layers.new(name=TILE_RANDOM).data.foreach_set("uv", data)
    me.uv_layers.active_index = 0
    for uv in me.uv_layers:
        uv.active_render = uv.name != TILE_RANDOM


def realize_group(src):
    """Nodes reading src's finished geometry (Object Info) and realising its instances. They sit on
    a stand-in object, so src itself is not touched: a modifier added to a wall would change what
    the vines climbing it grow on."""
    ng = bpy.data.node_groups.new("Realize for export", "GeometryNodeTree")
    ng[TAG] = True
    ng.interface.new_socket("Geometry", in_out="INPUT", socket_type="NodeSocketGeometry")
    ng.interface.new_socket("Geometry", in_out="OUTPUT", socket_type="NodeSocketGeometry")
    N, L = ng.nodes.new, ng.links.new
    info = N("GeometryNodeObjectInfo")
    info.transform_space = "ORIGINAL"            # in src's own space
    info.inputs["Object"].default_value = src
    geo = info.outputs["Geometry"]
    # the colour a generator gives each instance (Attribute node, Instancer) must win over the one
    # the part's own mesh carries (Attribute node, Geometry), as in the shaders; realising keeps
    # the mesh's, so keep the instance's under another name first
    for name in COLOR_ATTRS:
        get = N("GeometryNodeInputNamedAttribute")
        get.data_type = "FLOAT_COLOR"
        get.inputs["Name"].default_value = name
        put = N("GeometryNodeStoreNamedAttribute")
        put.data_type, put.domain = "FLOAT_COLOR", "INSTANCE"
        put.inputs["Name"].default_value = name + " (instance)"
        L(geo, put.inputs["Geometry"])
        L(get.outputs["Attribute"], put.inputs["Value"])
        geo = put.outputs["Geometry"]
    ri = N("GeometryNodeRealizeInstances")
    L(geo, ri.inputs[0])
    geo = ri.outputs[0]
    def first_set(names):
        """The first of these attributes that is set (longer than 0.0001, the shaders' test)."""
        out = None
        for name in reversed(names):
            get = N("GeometryNodeInputNamedAttribute")
            get.data_type = "FLOAT_COLOR"
            get.inputs["Name"].default_value = name
            if out is None:
                out = get.outputs["Attribute"]
                continue
            length = N("ShaderNodeVectorMath")
            length.operation = "LENGTH"
            L(get.outputs["Attribute"], length.inputs[0])
            has = N("ShaderNodeMath")
            has.operation = "GREATER_THAN"
            has.inputs[1].default_value = 0.0001
            L(length.outputs["Value"], has.inputs[0])
            pick = N("GeometryNodeSwitch")
            pick.input_type = "RGBA"
            L(has.outputs[0], pick.inputs["Switch"])
            L(out, pick.inputs["False"])
            L(get.outputs["Attribute"], pick.inputs["True"])
            out = pick.outputs[0]
        return out

    # what the shaders read once the instances are gone: the roof, ridge and tower tiles take the
    # instance's tile_color, else the part's own, else the instance's brick_color (the paving lays
    # roof tiles coloured with brick_color); the stones take the instance's brick_color, else the
    # part's own
    for name, order in (("tile_color", ["tile_color (instance)", "tile_color", "brick_color (instance)"]),
                        ("brick_color", ["brick_color (instance)", "brick_color"])):
        put = N("GeometryNodeStoreNamedAttribute")
        put.data_type, put.domain = "FLOAT_COLOR", "CORNER"
        put.inputs["Name"].default_value = name
        L(geo, put.inputs["Geometry"])
        L(first_set(order), put.inputs["Value"])
        geo = put.outputs["Geometry"]
    geo = tile_random(N, L, geo, generator_seed(src))
    L(geo, N("NodeGroupOutput").inputs[0])
    return ng


def baked_material(mat, cache):
    """A copy of mat whose base colour is the baked colour attribute (roughness etc. kept)."""
    if mat.name in cache:
        return cache[mat.name]
    new = mat.copy()
    new.name = mat.name.replace(" (procedural)", "") + " (baked)"
    new[TAG] = True
    nt = new.node_tree
    bsdf = next(n for n in nt.nodes if n.type == "BSDF_PRINCIPLED")
    for n in list(nt.nodes):                     # keep only the BSDF and the output
        if n.type not in ("BSDF_PRINCIPLED", "OUTPUT_MATERIAL"):
            nt.nodes.remove(n)
    col = nt.nodes.new("ShaderNodeVertexColor")
    col.layer_name = BAKE_LAYER
    nt.links.new(col.outputs["Color"], bsdf.inputs["Base Color"])
    cache[mat.name] = new
    return new


def keep_used_colors(c):
    """Only the colour attributes a material reads travel (glTF would export them all)."""
    used = {n.layer_name for m in c.data.materials if m and m.use_nodes
            for n in m.node_tree.nodes if n.type == "VERTEX_COLOR"}
    for a in list(c.data.color_attributes):
        if a.name not in used:
            c.data.color_attributes.remove(a)
    if c.data.color_attributes:
        first = c.data.color_attributes[0].name
        c.data.color_attributes.active_color_name = first
        c.data.color_attributes.render_color_index = 0
        for m in c.data.materials:               # an unnamed layer reads the render colour
            if m and m.use_nodes:
                for n in m.node_tree.nodes:
                    if n.type == "VERTEX_COLOR" and not n.layer_name:
                        n.layer_name = first


def one_slot_each(me):
    """Slots holding the same material made one, and empty slots dropped."""
    used = sorted({p.material_index for p in me.polygons})
    uniq, where = [], {}
    for i in used:
        m = me.materials[i]
        if m not in uniq:
            uniq.append(m)
        where[i] = uniq.index(m)
    idx = [0] * len(me.polygons)
    me.polygons.foreach_get("material_index", idx)
    idx = [where[i] for i in idx]
    me.materials.clear()
    for m in uniq:
        me.materials.append(m)
    me.polygons.foreach_set("material_index", idx)


def split_parts(copies, cache, tmp):
    """All copies joined, then cut into one object per part of SPLIT, each with one material
    ("Main Roof | Wood", "Main Roof | Tiles")."""
    prefix = (cli_args()[1:] or ONLY or ["Export"])[0]
    original = {b.name: a for a, b in cache.items()}         # "Roof tile (baked)" -> its source

    def part_of(m):
        name = original.get(m.name, m.name) if m else "No material"
        return next((p for p, names in SPLIT.items() if name in names), name)

    mats = {}
    for c in copies:
        for i, m in enumerate(c.data.materials):
            p = part_of(m)
            if p not in mats:
                new = m.copy() if m else bpy.data.materials.new(p)
                new.name = f"{prefix} | {p}"
                new[TAG] = True
                mats[p] = new
            c.data.materials[i] = mats[p]

    view_layer = bpy.context.view_layer
    for o in view_layer.objects:
        o.select_set(False)
    for c in copies:
        c.select_set(True)
    view_layer.objects.active = copies[0]
    bpy.ops.object.join()
    joined = view_layer.objects.active
    one_slot_each(joined.data)
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_all(action="SELECT")
    bpy.ops.mesh.separate(type="MATERIAL")
    bpy.ops.object.mode_set(mode="OBJECT")
    parts = [o for o in tmp.objects if o.type == "MESH" and o.data.polygons]
    for o in parts:
        one_slot_each(o.data)
        o.name = o.data.name = o.data.materials[0].name
        o[TAG] = o.data[TAG] = True
    return parts


def main():
    scene = bpy.context.scene
    view_layer = bpy.context.view_layer
    root = bpy.data.collections[ROOT]
    sources = [o for o in root.all_objects
               if o.visible_get() and o.type in ("MESH", "CURVE", "FONT", "SURFACE", "META") and wanted(o)]
    if not sources:
        raise RuntimeError(f"export_glb: nothing visible to export matches {cli_args()[1:] or ONLY}")

    # a collection of our own for the baked copies
    tmp = bpy.data.collections.new("Export (temporary)")
    tmp[TAG] = True
    scene.collection.children.link(tmp)

    stand_ins, groups = {}, []
    for o in sources:                            # realise the generators' instances
        if o.type != "MESH" and not any(m.type == "NODES" for m in o.modifiers):
            continue                             # text, plain curves: their own mesh will do
        ng = realize_group(o)
        groups.append(ng)
        s = bpy.data.objects.new("Export stand-in", bpy.data.meshes.new("Export stand-in"))
        s[TAG] = True
        s.modifiers.new("Realize for export", "NODES").node_group = ng
        tmp.objects.link(s)
        stand_ins[o] = s

    depsgraph = bpy.context.evaluated_depsgraph_get()
    copies = []
    for o in sources:
        ev = stand_ins.get(o, o).evaluated_get(depsgraph)
        me = bpy.data.meshes.new_from_object(ev, preserve_all_data_layers=True, depsgraph=depsgraph)
        if not me.polygons:
            bpy.data.meshes.remove(me)
            continue
        me.name = o.name
        me[TAG] = True
        keep_tile_random_second(me)
        c = bpy.data.objects.new(o.name, me)
        c[TAG] = True
        c.matrix_world = o.matrix_world.copy()
        tmp.objects.link(c)
        copies.append(c)

    for s in stand_ins.values():
        me = s.data
        bpy.data.objects.remove(s)
        bpy.data.meshes.remove(me)
    for ng in groups:
        bpy.data.node_groups.remove(ng)

    # bake the procedural colours
    to_bake = [c for c in copies if any(is_procedural(m) for m in c.data.materials)]
    engine = scene.render.engine
    scene.render.engine = "CYCLES"
    samples = scene.cycles.samples
    scene.cycles.samples = 1
    cache = {}
    for c in to_bake:
        attr = c.data.color_attributes.new(BAKE_LAYER, "BYTE_COLOR", "CORNER")
        c.data.color_attributes.active_color = attr
        for o in view_layer.objects:
            o.select_set(False)
        c.select_set(True)
        view_layer.objects.active = c
        bpy.ops.object.bake(type="DIFFUSE", pass_filter={"COLOR"}, target="VERTEX_COLORS")
        for i, m in enumerate(c.data.materials):
            if is_procedural(m):
                c.data.materials[i] = baked_material(m, cache)
    scene.render.engine = engine
    scene.cycles.samples = samples
    if split_on():
        copies = split_parts(copies, cache, tmp)
    for c in copies:
        keep_used_colors(c)

    path = out_path()
    os.makedirs(os.path.dirname(path), exist_ok=True)
    for o in view_layer.objects:
        o.select_set(False)
    for c in copies:
        c.select_set(True)
    bpy.ops.export_scene.gltf(
        filepath=path, export_format="GLB", use_selection=True, export_apply=False,
        export_yup=True, export_materials="EXPORT", export_vertex_color="MATERIAL",
        export_normals=True, export_texcoords=True, export_attributes=False,
        export_extras=False, export_cameras=False, export_lights=False, export_animations=False)

    tris = sum(sum(len(p.vertices) - 2 for p in c.data.polygons) for c in copies)
    print(f"export_glb: {len(copies)} objects, {tris} triangles, "
          f"{len(to_bake)} baked ({', '.join(sorted(cache))}) -> {path}")

    # leave the scene as it was
    for kind in ("objects", "meshes", "materials"):
        coll = getattr(bpy.data, kind)
        for x in [x for x in coll if x.get(TAG)]:
            coll.remove(x)
    bpy.data.collections.remove(tmp)


if __name__ == "__main__":
    main()
