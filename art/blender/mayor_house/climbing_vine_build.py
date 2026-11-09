"""Climbing vines that grow over a wall, live: a "Climbing Vine" Geometry Nodes modifier. Wandering,
kinked stems with side branches and forks, heart-shaped leaves, little five-petal flowers in
clusters, curled tips. The vines climb over what the wall's own modifiers lay on it (Brick Cover's
stones), and cross ledges.

Each vine is an object of its own (named "Vine N", in the "Climbing Vines" collection) that grows
from its origin over the wall named in its modifier. Change its settings in the modifier panel
(Properties > Modifiers) and it regrows as you drag; move the object and it regrows where it is.

Run this from Blender's Text Editor:
* New vine: put the 3D cursor where the vine should start (Shift + right-click on the wall, near the
  ground), select the wall mesh it should climb (e.g. the tower), run. Add more the same way.
* Running it with nothing selected only makes sure the node group, the leaf and flower meshes and
  the material are in the file (and up to date: the vines keep their settings).
* Remove a vine: delete its object.

The modifier's settings:
  Wall, More Walls   what it climbs: a mesh, and a collection of more (e.g. corner stones)
  Height             how high the main stem climbs, metres
  Branches           side branches off the main stem (about half of them fork once)
  Wander             how much the stems meander (0 straight up, 1 very loose)
  Leaves, Leaf Size  leaf density (0 bare stems) and size
  Flowers            flower amount (0 none, 1 many)
  Stem Thickness     stem thickness factor
  Offset             distance of the stems from the wall's surface
  Seed               another number, another vine
  Colors panel       stem colour, six leaf colours, four flower colours
The leaf and the flower are the meshes "Vine leaf" and "Vine flower" (collection "Vine Parts",
hidden): edit them and every vine changes. The vine object must stay unrotated and unscaled (it grows
upwards along its Z).

Everything is one mesh with one material, "Climbing vine", which colours each part from the
vine_color attribute.
"""
import math
import os
import sys

import bpy
from mathutils import Vector
from mathutils.bvhtree import BVHTree

sys.path.append(os.path.expanduser("~/.claude/skills/scenario-blender-geometry-nodes/scripts"))
import bx_gn as G  # noqa: E402

NAME = "Climbing Vine"
VERSION = 2                 # bump when the node group changes: the group is rebuilt, the vines keep their settings
COLL = "Climbing Vines"
PARTS = "Vine Parts"
MATERIAL = "Climbing vine"
STEP = 0.035                # growth step, metres


def lin(c):
    return tuple(x ** 2.2 for x in c) + (1.0,)


# sRGB colours: muted and warm (art direction: cozy village)
STEM = (0.47, 0.52, 0.33)
LEAVES = [(0.86, 0.55, 0.52), (0.80, 0.43, 0.45), (0.92, 0.68, 0.58), (0.69, 0.33, 0.40),
          (0.58, 0.29, 0.38), (0.93, 0.74, 0.55)]
FLOWERS = [(0.97, 0.80, 0.82), (0.98, 0.93, 0.86), (0.82, 0.70, 0.88), (0.95, 0.66, 0.70)]
CENTRE = (0.93, 0.76, 0.40)


# ---------------------------------------------------------------- field shorthands
class Kit:
    def __init__(self, b):
        self.b = b

    def M(self, op, a, c=None, d=None):
        n = self.b.node("Math", operation=op)
        for i, v in enumerate((a, c, d)):
            if v is not None:
                self.b.set(n, i, v)
        return n.outputs[0]

    def VM(self, op, a, c=None, scale=None, out="Vector"):
        n = self.b.node("VectorMath", operation=op)
        if a is not None:
            self.b.set(n, 0, a)
        if c is not None:
            self.b.set(n, 1, c)
        if scale is not None:
            self.b.set(n, "Scale", scale)
        return self.b.out(n, out)

    def dot(self, a, c):
        return self.VM("DOT_PRODUCT", a, c, out="Value")

    def norm(self, a):
        return self.VM("NORMALIZE", a)

    def scale(self, v, s):
        return self.VM("SCALE", v, scale=s)

    def add(self, *vs):
        out = vs[0]
        for v in vs[1:]:
            out = self.VM("ADD", out, v)
        return out

    def sub(self, a, c):
        return self.VM("SUBTRACT", a, c)

    def cmp(self, op, a, c):
        return self.b.out(self.b.node("Compare", data_type="FLOAT", operation=op, ins={"A": a, "B": c}), "Result")

    def bl(self, op, a, c=None):
        n = self.b.node("BooleanMath", operation=op)
        self.b.set(n, 0, a)
        if c is not None:
            self.b.set(n, 1, c)
        return n.outputs[0]

    def all(self, *cs):
        out = cs[0]
        for c in cs[1:]:
            out = self.bl("AND", out, c)
        return out

    def sw(self, kind, cond, f, t):
        return self.b.out(self.b.node("Switch", input_type=kind, ins={"Switch": cond, "False": f, "True": t}),
                          "Output")

    def na(self, name, dtype="FLOAT"):
        return self.b.out(self.b.node("NamedAttribute", data_type=dtype, ins={"Name": name}), "Attribute")

    def store(self, g, name, val, dtype="FLOAT", domain="POINT"):
        return self.b.out(self.b.node("StoreNamedAttribute", data_type=dtype, domain=domain,
                                      ins={"Geometry": g, "Name": name, "Value": val}), "Geometry")

    def xyz(self, v):
        return self.b.node("SeparateXYZ", ins={"Vector": v})

    def z(self, v):
        return self.b.out(self.xyz(v), "Z")

    def vec(self, x=0.0, y=0.0, z=0.0):
        return self.b.out(self.b.node("CombineXYZ", ins={"X": x, "Y": y, "Z": z}), "Vector")

    def rnd(self, lo, hi, seed, id_=None, dtype="FLOAT"):
        n = self.b.node("RandomValue", data_type=dtype, ins={"Min": lo, "Max": hi, "Seed": seed})
        if id_ is not None:
            self.b.set(n, "ID", id_)
        return self.b.out(n, "Value")

    def chance(self, p, seed, id_):
        n = self.b.node("RandomValue", data_type="BOOLEAN", ins={"Probability": p, "Seed": seed})
        if id_ is not None:
            self.b.set(n, "ID", id_)
        return self.b.out(n, "Value")

    def iadd(self, a, c):
        return self.b.out(self.b.node("IntegerMath", operation="ADD", ins={0: a, 1: c}), 0)

    def seed(self, s, k):
        return self.iadd(s, k)

    def frame(self, n):
        """Up the wall and sideways along it, for a wall normal n."""
        up = self.norm(self.sub(self.vec(z=1.0), self.scale(n, self.z(n))))
        return up, self.norm(self.VM("CROSS_PRODUCT", up, n))

    def nearest(self, surf, pos, value):
        n = self.b.node("SampleNearestSurface", data_type="FLOAT_VECTOR",
                        ins={"Mesh": surf, "Value": value, "Sample Position": pos})
        return self.b.out(n, "Value")

    def position(self):
        return self.b.node("Position").outputs[0]

    def normal(self):
        return self.b.out(self.b.node("Normal"), "Normal")


# ---------------------------------------------------------------- growing
def grow_group():
    """Grow every head (a point with its stem's settings as attributes) over Surface, step by step,
    and leave a trail of points: the stems' points, in order (_v_k) per stem (_v_id)."""
    b = G.Builder(NAME + " | Grow")
    k = Kit(b)
    surf = b.input("Surface", "GEOMETRY")
    steps = b.input("Steps", "INT", 100, min=0)
    step = b.input("Step", "FLOAT", STEP)
    wander = b.input("Wander", "FLOAT", 0.55)
    off = b.input("Offset", "FLOAT", 0.022)
    seed = b.input("Seed", "INT", 0)

    zi, zo = b.zone("REPEAT", items=[("GEOMETRY", "Trail")], iterations=steps)
    b.set(zi, "Geometry", b.geo_in)
    it = b.out(zi, "Iteration")
    pos = k.position()

    # 1. snap to the wall, check it is still growing, record it
    q = k.nearest(surf, pos, k.position())
    n = k.norm(k.nearest(surf, pos, k.normal()))
    dist = k.VM("DISTANCE", pos, q, out="Value")
    is_flat = k.cmp("GREATER_THAN", k.M("ABSOLUTE", k.z(n)), 0.7)        # over a stone's top or a ledge
    flat = k.sw("FLOAT", is_flat, 0.0, k.M("ADD", k.na("_v_flat"), 1.0))
    snapped = k.add(q, k.scale(n, off))
    sz = k.z(snapped)
    alive = k.all(k.na("_v_alive", "BOOLEAN"),
                  k.cmp("LESS_THAN", dist, 0.3),                         # still on the wall
                  k.cmp("LESS_EQUAL", flat, 5.5),                        # not on a real top or soffit
                  k.cmp("LESS_EQUAL", sz, k.na("_v_top")),
                  k.cmp("GREATER_EQUAL", sz, k.M("SUBTRACT", k.na("_v_z0"), 0.15)),
                  k.cmp("LESS_THAN", it, k.na("_v_nmax")))
    g = b.out(zi, "Geometry")
    g = k.store(g, "_v_alive", alive, "BOOLEAN")
    g = k.store(g, "_v_flat", flat)
    g = k.store(g, "_v_n", n, "FLOAT_VECTOR")
    g = k.store(g, "_v_k", it)
    g = b.out(b.node("SetPosition", ins={"Geometry": g, "Position": snapped}), "Geometry")
    g = b.out(b.node("SeparateGeometry", domain="POINT",
                     ins={"Geometry": g, "Selection": k.na("_v_alive", "BOOLEAN")}), "Selection")
    trail = b.join([b.out(zi, "Trail"), g])

    # 2. steer: up the wall, wandering, with zig-zag kinks; across a ledge back to the wall
    n = k.na("_v_n", "FLOAT_VECTOR")
    sid = k.na("_v_id", "INT")
    s = k.M("DIVIDE", it, k.M("MAXIMUM", k.na("_v_nmax"), 1.0))
    run = b.out(b.node("FloatToInt", rounding_mode="FLOOR", ins={0: k.M("DIVIDE", it, 5.0)}), 0)
    kid = k.iadd(b.out(b.node("IntegerMath", operation="MULTIPLY", ins={0: sid, 1: 997}), 0), run)
    amp = k.M("MULTIPLY", 0.4, k.M("ADD", wander, 0.4))
    kink = k.rnd(-1.0, 1.0, k.seed(seed, 3), kid)
    w = k.M("ADD", k.M("MULTIPLY", it, k.M("MULTIPLY", step, 1.4)),
            k.M("ADD", k.M("MULTIPLY", sid, 7.31), k.M("MULTIPLY", seed, 3.17)))
    fac = b.out(b.node("NoiseTexture", noise_dimensions="1D", ins={"W": w, "Scale": 1.0, "Detail": 0.0}), "Fac")
    h = k.M("ADD", k.M("MULTIPLY", k.na("_v_h0"), k.M("SUBTRACT", 1.0, k.M("MULTIPLY", 0.45, s))),
            k.M("ADD", k.M("MULTIPLY", k.M("SUBTRACT", fac, 0.5), k.M("MULTIPLY", wander, 2.4)),
                k.M("MULTIPLY", kink, amp)))
    up, side = k.frame(n)
    wall_want = k.add(k.scale(up, k.M("COSINE", h)), k.scale(side, k.M("SINE", h)))
    lastn = k.na("_v_lastn", "FLOAT_VECTOR")
    on_top = k.cmp("GREATER_THAN", k.z(n), 0.0)
    flat_want = k.sw("VECTOR", on_top, lastn, k.scale(lastn, -1.0))
    on_flat = k.cmp("GREATER_THAN", k.na("_v_flat"), 0.5)
    want = k.sw("VECTOR", on_flat, wall_want, flat_want)
    new_lastn = k.sw("VECTOR", on_flat, n, lastn)
    d0 = k.na("_v_dir", "FLOAT_VECTOR")
    f = k.sw("FLOAT", k.cmp("LESS_THAN", it, 0.5), 0.45, 1.0)
    d = k.add(d0, k.scale(k.sub(want, d0), f))
    d = k.norm(k.sub(d, k.scale(n, k.dot(d, n))))
    g2 = k.store(g, "_v_dir", d, "FLOAT_VECTOR")
    g2 = k.store(g2, "_v_lastn", new_lastn, "FLOAT_VECTOR")
    g2 = b.out(b.node("SetPosition", ins={"Geometry": g2, "Offset": k.scale(k.na("_v_dir", "FLOAT_VECTOR"), step)}),
               "Geometry")
    b.link(g2, (zo, "Geometry"))
    b.link(trail, (zo, "Trail"))
    b.output_geometry((zo, "Trail"))
    return b.done()


# ---------------------------------------------------------------- the vine
def vine_group(grow):
    b = G.Builder(NAME)
    k = Kit(b)
    wall = b.input("Wall", "OBJECT", description="The mesh the vine climbs")
    more = b.input("More Walls", "COLLECTION", description="More meshes it climbs (e.g. corner stones)")
    height = b.input("Height", "FLOAT", 2.4, min=0.1, max=20.0, subtype="DISTANCE",
                     description="How high the main stem climbs")
    branches = b.input("Branches", "INT", 6, min=0, max=30, description="Side branches off the main stem")
    wander = b.input("Wander", "FLOAT", 0.55, min=0.0, max=1.5, description="How much the stems meander")
    leaves = b.input("Leaves", "FLOAT", 1.0, min=0.0, max=3.0, description="Leaf density (0: bare stems)")
    leaf_size = b.input("Leaf Size", "FLOAT", 1.0, min=0.1, max=4.0)
    flowers = b.input("Flowers", "FLOAT", 0.5, min=0.0, max=1.0, subtype="FACTOR", description="0 none, 1 many")
    thick = b.input("Stem Thickness", "FLOAT", 1.0, min=0.1, max=4.0)
    off = b.input("Offset", "FLOAT", 0.022, min=0.0, max=0.3, subtype="DISTANCE",
                  description="Distance of the stems from the wall's surface")
    seed = b.input("Seed", "INT", 1)
    stem_col = b.input("Stem Color", "COLOR", lin(STEM), panel="Colors")
    leaf_cols = [b.input(f"Leaf {i + 1}", "COLOR", lin(c), panel="Colors") for i, c in enumerate(LEAVES)]
    flower_cols = [b.input(f"Flower {i + 1}", "COLOR", lin(c), panel="Colors") for i, c in enumerate(FLOWERS)]
    leaf_obj = b.input("Leaf", "OBJECT", panel="Parts", description="The leaf mesh (length 1 along X, face up Z)")
    flower_obj = b.input("Flower", "OBJECT", panel="Parts", description="The flower mesh (radius 1, face up Z)")
    mat = b.input("Material", "MATERIAL", panel="Parts")

    # what it climbs, in this object's space, with what the wall's modifiers lay on it
    oi = b.out(b.node("ObjectInfo", transform_space="RELATIVE", ins={"Object": wall}), "Geometry")
    ci = b.out(b.node("CollectionInfo", transform_space="RELATIVE", ins={"Collection": more}), "Instances")
    surf = b.out(b.node("RealizeInstances", ins={"Geometry": b.out(b.join([oi, ci]), "Geometry")}), "Geometry")

    def heads(g, vals):
        for name, (val, dtype) in vals.items():
            g = k.store(g, name, val, dtype)
        return g

    def grow_stems(g, nsteps, sd):
        gr = b.node("Group", node_tree=grow, ins={"Geometry": g, "Surface": surf, "Steps": nsteps, "Step": STEP,
                                                   "Wander": wander, "Offset": off, "Seed": sd})
        crv = b.out(b.node("PointsToCurves", ins={"Points": b.out(gr, "Geometry"),
                                                   "Curve Group ID": k.na("_v_id", "INT"),
                                                   "Weight": k.na("_v_k")}), "Curves")
        cnt = b.out(b.node("SplineLength"), "Point Count")
        return b.out(b.node("DeleteGeometry", domain="CURVE",
                            ins={"Geometry": crv, "Selection": k.cmp("LESS_THAN", cnt, 4.0)}), "Geometry")

    def to_steps(length):
        return b.out(b.node("FloatToInt", rounding_mode="CEILING", ins={0: k.M("DIVIDE", length, STEP)}), 0)

    idx = b.node("Index").outputs[0]
    common = {"_v_alive": (True, "BOOLEAN"), "_v_flat": (0.0, "FLOAT"), "_v_lastn": ((0.0, 0.0, 0.0), "FLOAT_VECTOR"),
              "_v_dir": ((0.0, 0.0, 1.0), "FLOAT_VECTOR")}

    # main stem, from the origin
    p0 = b.out(b.node("Points", ins={"Count": 1}), "Points")
    main_len = k.M("MULTIPLY", height, 1.6)
    p0 = heads(p0, {**common, "_v_h0": (k.rnd(-0.15, 0.15, seed, 0), "FLOAT"), "_v_id": (0, "INT"),
                    "_v_nmax": (k.M("DIVIDE", main_len, STEP), "FLOAT"), "_v_top": (height, "FLOAT"),
                    "_v_z0": (0.0, "FLOAT"), "_v_depth": (0.0, "FLOAT"), "_v_r0": (0.016, "FLOAT")})
    main = grow_stems(p0, to_steps(main_len), seed)

    # side branches, off the main stem, alternating sides
    pb = b.out(b.node("Points", ins={"Count": branches}), "Points")
    fb = k.rnd(0.12, 0.88, k.seed(seed, 11), idx)
    sc = b.node("SampleCurve", data_type="FLOAT", mode="FACTOR", use_all_curves=True,
                ins={"Curves": main, "Factor": fb})
    pb = b.out(b.node("SetPosition", ins={"Geometry": pb, "Position": b.out(sc, "Position")}), "Geometry")
    parity = k.M("SUBTRACT", k.M("MULTIPLY", b.out(b.node("IntegerMath", operation="MODULO", ins={0: idx, 1: 2}), 0),
                                 2.0), 1.0)
    first = k.sw("FLOAT", k.chance(0.5, k.seed(seed, 15), 0), -1.0, 1.0)
    keep = k.sw("FLOAT", k.chance(0.2, k.seed(seed, 12), idx), 1.0, -1.0)
    side = k.M("MULTIPLY", parity, k.M("MULTIPLY", first, keep))
    blen = k.M("MULTIPLY", height, k.rnd(0.25, 0.55, k.seed(seed, 14), idx))
    pz = k.z(k.position())
    pb = heads(pb, {**common, "_v_h0": (k.M("MULTIPLY", side, k.rnd(0.6, 1.25, k.seed(seed, 13), idx)), "FLOAT"),
                    "_v_id": (k.iadd(idx, 1), "INT"),
                    "_v_nmax": (k.M("DIVIDE", blen, STEP), "FLOAT"), "_v_top": (k.M("ADD", height, 0.2), "FLOAT"),
                    "_v_z0": (pz, "FLOAT"), "_v_depth": (1.0, "FLOAT"),
                    "_v_r0": (k.M("MULTIPLY", 0.012, k.M("SUBTRACT", 1.0, k.M("MULTIPLY", 0.55, fb))), "FLOAT")})
    bra = grow_stems(pb, to_steps(k.M("MULTIPLY", height, 0.55)), k.seed(seed, 100))

    # forks, off about half the branches
    pf = b.out(b.node("Points", ins={"Count": branches}), "Points")
    nb = b.out(b.node("GeometryNodeAttributeDomainSize", component="CURVE", ins={"Geometry": bra}), "Spline Count")
    ff = k.rnd(0.25, 0.75, k.seed(seed, 21), idx)

    def sample_branch(value):
        return b.node("SampleCurve", data_type="FLOAT", mode="FACTOR", use_all_curves=False,
                      ins={"Curves": bra, "Value": value, "Factor": ff, "Curve Index": idx})

    sp = sample_branch(k.na("_v_h0"))
    bh0 = b.out(sp, "Value")
    bnmax = b.out(sample_branch(k.na("_v_nmax")), "Value")
    br0 = b.out(sample_branch(k.na("_v_r0")), "Value")
    bcnt = b.out(sample_branch(b.out(b.node("SplineLength"), "Point Count")), "Value")
    pf = b.out(b.node("SetPosition", ins={"Geometry": pf, "Position": b.out(sp, "Position")}), "Geometry")
    fork = k.all(k.cmp("LESS_THAN", idx, nb), k.chance(0.5, k.seed(seed, 22), idx),
                 k.cmp("GREATER_THAN", bcnt, 10.0))
    pf = b.out(b.node("DeleteGeometry", domain="POINT", ins={"Geometry": pf, "Selection": k.bl("NOT", fork)}),
               "Geometry")
    pf = heads(pf, {**common, "_v_h0": (k.M("MULTIPLY", k.M("MULTIPLY", bh0, -1.0),
                                              k.rnd(0.3, 0.8, k.seed(seed, 23), idx)), "FLOAT"),
                    "_v_id": (k.iadd(idx, 100), "INT"),
                    "_v_nmax": (k.M("MULTIPLY", bnmax, 0.5), "FLOAT"), "_v_top": (k.M("ADD", height, 0.3), "FLOAT"),
                    "_v_z0": (pz, "FLOAT"), "_v_depth": (2.0, "FLOAT"),
                    "_v_r0": (k.M("MULTIPLY", br0, 0.7), "FLOAT")})
    frk = grow_stems(pf, to_steps(k.M("MULTIPLY", height, 0.3)), k.seed(seed, 200))

    # smooth the stems (stones and joints make them bumpy), never into the wall
    stems = b.out(b.join([main, bra, frk]), "Geometry")
    blur = b.out(b.node("BlurAttribute", data_type="FLOAT_VECTOR",
                        ins={"Value": k.position(), "Iterations": 3}), "Value")
    stems = b.out(b.node("SetPosition", ins={"Geometry": stems, "Position": blur}), "Geometry")
    pos = k.position()
    q = k.nearest(surf, pos, k.position())
    qn = k.norm(k.nearest(surf, pos, k.normal()))
    close = k.cmp("LESS_THAN", k.dot(k.sub(pos, q), qn), k.M("MULTIPLY", off, 0.8))
    stems = b.out(b.node("SetPosition", ins={"Geometry": stems, "Selection": close,
                                             "Position": k.add(q, k.scale(qn, k.M("MULTIPLY", off, 0.8)))}),
                  "Geometry")
    stems = k.store(stems, "_v_param", b.out(b.node("SplineParameter"), "Factor"))
    stems = k.store(stems, "_v_total", b.out(b.node("SplineLength"), "Length"))
    stems = k.store(stems, "_v_t", b.out(b.node("CurveTangent"), "Tangent"), "FLOAT_VECTOR")
    par = k.na("_v_param")
    depth = k.na("_v_depth")
    # radius: thinner towards the tips, thicker where the main stem leaves the ground
    base = k.M("ADD", 1.0, k.M("MULTIPLY", 0.25, k.M("MAXIMUM", k.M("SUBTRACT", 4.0, k.na("_v_k")), 0.0)))
    base = k.sw("FLOAT", k.cmp("LESS_THAN", depth, 0.5), 1.0, base)
    r = k.M("MULTIPLY", k.M("ADD", k.M("MULTIPLY", k.na("_v_r0"), k.M("SUBTRACT", 1.0, k.M("MULTIPLY", 0.75, par))),
                            0.002), k.M("MULTIPLY", base, thick))
    stems = b.out(b.node("SetCurveRadius", ins={"Curve": stems, "Radius": r}), "Curve")
    stems = k.store(stems, "vine_color", k.scale(stem_col, k.M("ADD", 0.85, k.M("MULTIPLY", 0.25, par))),
                    "FLOAT_COLOR")

    # curled tips, where a stem grew its full length
    tip = k.all(b.out(b.node("GeometryNodeCurveEndpointSelection", ins={"Start Size": 0, "End Size": 1}), "Selection"),
                k.cmp("GREATER_EQUAL", k.na("_v_k"), k.M("SUBTRACT", k.na("_v_nmax"), 1.5)))
    stems = k.store(stems, "_v_tip", tip, "BOOLEAN")
    cpts = b.out(b.node("CurveToPoints", mode="EVALUATED", ins={"Curve": stems}), "Points")
    cpts = b.out(b.node("SeparateGeometry", domain="POINT",
                        ins={"Geometry": cpts, "Selection": k.na("_v_tip", "BOOLEAN")}), "Selection")
    spiral = b.node("CurveSpiral", ins={"Resolution": 9, "Rotations": 0.85, "Start Radius": 0.033,
                                        "End Radius": 0.008, "Height": 0.0})
    spiral = b.out(b.node("Transform", ins={"Geometry": spiral, "Translation": (-0.033, 0.0, 0.0)}), "Geometry")
    rz = b.out(b.node("AlignRotationToVector", axis="Z", ins={"Vector": k.na("_v_n", "FLOAT_VECTOR")}), "Rotation")
    ry = b.out(b.node("AlignRotationToVector", axis="Y", pivot_axis="Z",
                      ins={"Rotation": rz, "Vector": k.na("_v_t", "FLOAT_VECTOR")}), "Rotation")
    flip = k.sw("FLOAT", k.chance(0.5, k.seed(seed, 31), idx), 1.0, -1.0)
    curls = b.node("InstanceOnPoints", ins={"Points": cpts, "Instance": spiral, "Rotation": ry,
                                            "Scale": k.vec(flip, 1.0, 1.0)})
    curls = b.out(b.node("RealizeInstances", ins={"Geometry": curls}), "Geometry")
    curls = b.out(b.node("SetCurveRadius", ins={"Curve": curls, "Radius": k.M("MULTIPLY", 0.0045, thick)}), "Curve")
    curls = k.store(curls, "vine_color", k.scale(stem_col, 1.08), "FLOAT_COLOR")
    profile = b.node("CurveCircle", ins={"Resolution": 5, "Radius": 1.0})
    stem_mesh = b.out(b.node("CurveToMesh", ins={"Curve": b.out(b.join([stems, curls]), "Geometry"),
                                                 "Profile Curve": profile, "Fill Caps": True,
                                                 "Scale": b.out(b.node("GeometryNodeInputRadius"), 0)}), "Mesh")
    stem_mesh = k.store(stem_mesh, "vine_shade", 1.0)

    # leaves, alternating along the stems, hanging out and down from the wall
    gap = k.M("DIVIDE", 0.075, k.M("MAXIMUM", leaves, 0.05))
    c2p = b.node("CurveToPoints", mode="LENGTH", ins={"Curve": stems, "Length": gap})
    lp = k.store(b.out(c2p, "Points"), "_v_tan", b.out(c2p, "Tangent"), "FLOAT_VECTOR")
    bare = k.bl("OR", k.cmp("LESS_EQUAL", leaves, 0.0),
                k.all(k.cmp("LESS_THAN", depth, 0.5), k.cmp("LESS_THAN", k.M("MULTIPLY", par, k.na("_v_total")), 0.06)))
    lp = b.out(b.node("DeleteGeometry", domain="POINT", ins={"Geometry": lp, "Selection": bare}), "Geometry")
    n = k.na("_v_n", "FLOAT_VECTOR")
    t = k.na("_v_tan", "FLOAT_VECTOR")
    lside = k.M("SUBTRACT", k.M("MULTIPLY", b.out(b.node("IntegerMath", operation="MODULO", ins={0: idx, 1: 2}), 0),
                                2.0), 1.0)
    across = k.scale(k.norm(k.VM("CROSS_PRODUCT", n, t)), lside)
    up, _ = k.frame(n)
    axis = k.norm(k.add(k.scale(across, 0.75), k.scale(up, k.rnd(-0.75, -0.2, k.seed(seed, 41), idx)),
                        k.scale(n, k.rnd(0.25, 0.55, k.seed(seed, 42), idx)),
                        k.scale(t, k.rnd(-0.2, 0.2, k.seed(seed, 43), idx))))
    size = k.M("MULTIPLY", k.M("MULTIPLY", 0.11, leaf_size),
               k.M("MULTIPLY", k.rnd(0.7, 1.25, k.seed(seed, 44), idx),
                   k.M("MULTIPLY", k.M("SUBTRACT", 1.0, k.M("MULTIPLY", 0.45, par)),
                       k.M("SUBTRACT", 1.0, k.M("MULTIPLY", 0.12, depth)))))
    rx = b.out(b.node("AlignRotationToVector", axis="X", ins={"Vector": axis}), "Rotation")
    rot = b.out(b.node("AlignRotationToVector", axis="Z", pivot_axis="X", ins={"Rotation": rx, "Vector": n}),
                "Rotation")
    lp = k.store(lp, "_v_size", size)
    lp = k.store(lp, "_v_rot", rot, "QUATERNION")
    lp = b.out(b.node("SetPosition", ins={"Geometry": lp, "Offset": k.scale(across, 0.01)}), "Geometry")
    leaf_geo = b.out(b.node("ObjectInfo", ins={"Object": leaf_obj}), "Geometry")
    li = b.out(b.node("InstanceOnPoints", ins={"Points": lp, "Instance": leaf_geo,
                                               "Rotation": k.na("_v_rot", "QUATERNION"),
                                               "Scale": k.na("_v_size")}), "Instances")
    pick = b.node("IndexSwitch", data_type="RGBA",
                  ins={"Index": k.rnd(0, len(LEAVES) - 1, k.seed(seed, 45), idx, dtype="INT")})
    while len(pick.index_switch_items) < len(LEAVES):
        pick.index_switch_items.new()
    for i, c in enumerate(leaf_cols):
        b.link(c, pick.inputs[i + 1])
    lcol = k.scale(b.out(pick, "Output"), k.rnd(0.9, 1.08, k.seed(seed, 46), idx))
    li = k.store(li, "vine_color", lcol, "FLOAT_COLOR", "INSTANCE")
    leaf_mesh = b.out(b.node("RealizeInstances", ins={"Geometry": li}), "Geometry")

    # flowers: a little cluster near the tips, and now and then along the stems
    fp = b.out(b.node("CurveToPoints", mode="LENGTH", ins={"Curve": stems, "Length": 6 * STEP}), "Points")
    total = k.na("_v_total")
    from_tip = k.M("MULTIPLY", k.M("SUBTRACT", 1.0, par), total)
    at_tip = k.all(k.cmp("GREATER_EQUAL", from_tip, 6 * STEP), k.cmp("LESS_THAN", from_tip, 12 * STEP),
                   k.cmp("GREATER_THAN", total, 12 * STEP))
    some = k.chance(k.M("MULTIPLY", 0.18, flowers), k.seed(seed, 51), idx)
    bloom = k.all(k.bl("OR", at_tip, some), k.chance(k.M("MULTIPLY", 1.4, flowers), k.seed(seed, 52), idx),
                  k.cmp("GREATER_THAN", k.M("MULTIPLY", par, total), 4 * STEP))
    fp = b.out(b.node("SeparateGeometry", domain="POINT", ins={"Geometry": fp, "Selection": bloom}), "Selection")
    fp = k.store(fp, "_v_fc", k.rnd(0, len(FLOWERS) - 1, k.seed(seed, 53), idx, dtype="INT"), "INT")
    fp = k.store(fp, "_v_cnt", k.rnd(1, 4, k.seed(seed, 54), idx, dtype="INT"), "INT")
    fp = k.store(fp, "_v_ang", k.rnd(0.0, 6.283, k.seed(seed, 55), idx))
    dup = b.node("DuplicateElements", domain="POINT", ins={"Geometry": fp, "Amount": k.na("_v_cnt", "INT")})
    fp = b.out(dup, "Geometry")
    di = b.out(dup, "Duplicate Index")
    # a cluster's flowers sit round a ring just wide enough that they do not overlap, each a little
    # further out from the wall than the one before
    cnt = k.na("_v_cnt")
    ang = k.M("ADD", k.na("_v_ang"), k.M("DIVIDE", k.M("MULTIPLY", di, 6.283), cnt))
    ring = k.M("DIVIDE", k.M("MULTIPLY", 0.036, leaf_size),
               k.M("SINE", k.M("DIVIDE", math.pi, k.M("MAXIMUM", cnt, 2.0))))
    ring = k.sw("FLOAT", k.cmp("GREATER_THAN", cnt, 1.5), 0.0, ring)
    up, side = k.frame(n)
    jit = k.add(k.scale(up, k.M("MULTIPLY", ring, k.M("COSINE", ang))),
                k.scale(side, k.M("MULTIPLY", ring, k.M("SINE", ang))),
                k.scale(n, k.M("ADD", 0.03, k.M("MULTIPLY", di, 0.006))))
    fn = k.norm(k.add(n, k.scale(up, k.rnd(-0.2, 0.25, k.seed(seed, 57), idx)),
                      k.scale(side, k.rnd(-0.2, 0.2, k.seed(seed, 58), idx))))
    frot = b.out(b.node("AlignRotationToVector", axis="Z", ins={"Vector": fn}), "Rotation")
    spin = b.out(b.node("AxisAngleToRotation", ins={"Axis": (0.0, 0.0, 1.0),
                                                    "Angle": k.rnd(0.0, 6.283, k.seed(seed, 59), idx)}), "Rotation")
    frot = b.out(b.node("RotateRotation", rotation_space="LOCAL", ins={"Rotation": frot, "Rotate By": spin}),
                 "Rotation")
    fp = k.store(fp, "_v_rot", frot, "QUATERNION")
    fp = b.out(b.node("SetPosition", ins={"Geometry": fp, "Offset": jit}), "Geometry")
    fsize = k.M("MULTIPLY", k.M("MULTIPLY", 0.03, leaf_size), k.rnd(0.8, 1.2, k.seed(seed, 60), idx))
    flower_geo = b.out(b.node("ObjectInfo", ins={"Object": flower_obj}), "Geometry")
    fi = b.out(b.node("InstanceOnPoints", ins={"Points": fp, "Instance": flower_geo,
                                               "Rotation": k.na("_v_rot", "QUATERNION"), "Scale": fsize}),
               "Instances")
    fpick = b.node("IndexSwitch", data_type="RGBA", ins={"Index": k.na("_v_fc", "INT")})
    while len(fpick.index_switch_items) < len(FLOWERS):
        fpick.index_switch_items.new()
    for i, c in enumerate(flower_cols):
        b.link(c, fpick.inputs[i + 1])
    fcol = k.scale(b.out(fpick, "Output"), k.rnd(0.93, 1.05, k.seed(seed, 61), idx))
    fi = k.store(fi, "vine_color", fcol, "FLOAT_COLOR", "INSTANCE")
    flower_mesh = b.out(b.node("RealizeInstances", ins={"Geometry": fi}), "Geometry")

    out = b.out(b.join([stem_mesh, leaf_mesh, flower_mesh]), "Geometry")
    out = b.out(b.node("SetMaterial", ins={"Geometry": out, "Material": mat}), "Geometry")
    out = b.out(b.node("SetShadeSmooth", ins={"Geometry": out}), "Geometry")
    out = b.out(b.node("GeometryNodeRemoveAttribute",
                       ins={"Geometry": out, "Pattern Mode": "Wildcard", "Name": "_v_*"}), "Geometry")
    b.output_geometry(out)
    tree = b.done()
    tree.description = ("A climbing vine growing from the object's origin over Wall: stems, leaves, flowers "
                        "and curled tips.")
    return tree


# ---------------------------------------------------------------- parts, material
HEART = [(0.0, 0.0), (-0.09, 0.14), (-0.05, 0.30), (0.08, 0.40), (0.26, 0.43), (0.46, 0.37),
         (0.66, 0.24), (0.84, 0.11), (1.0, 0.0)]


def parts_collection():
    c = bpy.data.collections.get(PARTS)
    if c is None:
        c = bpy.data.collections.new(PARTS)
        bpy.context.scene.collection.children.link(c)
    c.hide_render = True
    c.hide_viewport = True
    return c


def part(name, verts, faces, shade, centre=None):
    me = bpy.data.meshes.get(name) or bpy.data.meshes.new(name)
    me.clear_geometry()
    me.from_pydata(verts, [], faces)
    for nm, vals in (("vine_shade", shade), ("vine_centre", centre)):
        if vals is None:
            continue
        a = me.attributes.get(nm) or me.attributes.new(nm, "FLOAT", "POINT")
        a.data.foreach_set("value", vals)
    me.polygons.foreach_set("use_smooth", [True] * len(me.polygons))
    me.update()
    obj = bpy.data.objects.get(name)
    if obj is None:
        obj = bpy.data.objects.new(name, me)
        parts_collection().objects.link(obj)
    obj.data = me
    return obj


def leaf_part():
    """A heart leaf, notch at the origin, tip at x = 1, folded along its midrib, its tip drooping."""
    fold, droop = 0.22, 0.15
    outline = HEART + [(x, -y) for x, y in reversed(HEART[1:-1])]

    def at(x, y, lift=0.0):
        return (x, y, -fold * abs(y) - droop * x * x + lift)

    verts = [at(0.38, 0.0, 0.02)] + [at(x, y) for x, y in outline]
    m = len(outline)
    faces = [(0, 1 + i, 1 + (i + 1) % m) for i in range(m)]
    shade = [1.0] + [0.82 if x < 0.15 else 1.08 if x > 0.7 else 1.0 for x, _ in outline]
    return part("Vine leaf", verts, faces, shade)


def flower_part():
    """Five petals round a centre, radius 1, face up Z, petals cupped a little."""
    verts, faces, shade, centre = [(0.0, 0.0, 0.15)], [], [1.0], [1.0]
    for k in range(5):
        a = 2 * math.pi * k / 5
        d = Vector((math.cos(a), math.sin(a), 0.0))
        e = Vector((-math.sin(a), math.cos(a), 0.0))
        i0 = len(verts)
        for x, y in ((0.18, -0.16), (0.55, -0.3), (0.95, -0.12), (1.0, 0.12), (0.55, 0.3), (0.18, 0.16)):
            verts.append(tuple(d * x + e * y * 0.9 + Vector((0, 0, 0.18 * x * x))))
            shade.append(0.9 + 0.12 * x)
            centre.append(0.0)
        faces.append([0] + list(range(i0, i0 + 6)))
    return part("Vine flower", verts, faces, shade, centre)


def material():
    m = bpy.data.materials.get(MATERIAL) or bpy.data.materials.new(MATERIAL)
    nt = m.node_tree
    nt.nodes.clear()
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    bsdf = nt.nodes.new("ShaderNodeBsdfPrincipled")
    col, sh, ce = (nt.nodes.new("ShaderNodeAttribute") for _ in range(3))
    col.attribute_name, sh.attribute_name, ce.attribute_name = "vine_color", "vine_shade", "vine_centre"
    mul = nt.nodes.new("ShaderNodeVectorMath")
    mul.operation = "SCALE"
    mix = nt.nodes.new("ShaderNodeMix")
    mix.data_type = "RGBA"
    nt.links.new(col.outputs["Color"], mul.inputs[0])
    nt.links.new(sh.outputs["Fac"], mul.inputs["Scale"])
    nt.links.new(ce.outputs["Fac"], mix.inputs["Factor"])
    a_in = next(s for s in mix.inputs if s.name == "A" and s.type == "RGBA")
    b_in = next(s for s in mix.inputs if s.name == "B" and s.type == "RGBA")
    res = next(s for s in mix.outputs if s.name == "Result" and s.type == "RGBA")
    nt.links.new(mul.outputs["Vector"], a_in)
    b_in.default_value = lin(CENTRE)
    nt.links.new(res, bsdf.inputs["Base Color"])
    bsdf.inputs["Roughness"].default_value = 0.7
    nt.links.new(bsdf.outputs["BSDF"], out.inputs["Surface"])
    for i, n in enumerate((col, sh, ce)):
        n.location = (-700, 200 - 180 * i)
    mul.location, mix.location, bsdf.location, out.location = (-450, 150), (-250, 0), (0, 0), (350, 0)
    m.diffuse_color = (0.8, 0.45, 0.45, 1.0)
    return m


# ---------------------------------------------------------------- set-up
def vine_modifiers():
    return [md for o in bpy.data.objects for md in getattr(o, "modifiers", [])
            if md.type == "NODES" and md.node_group is not None and md.node_group.name == NAME]


def ensure():
    """The node group, parts and material; the group is rebuilt when VERSION changed, and the vines
    keep their settings."""
    leaf, flower, mat = leaf_part(), flower_part(), material()
    tree = bpy.data.node_groups.get(NAME)
    if tree is None or tree.get("vine_version") != VERSION:
        saved = [(md, G.get_inputs(md)) for md in vine_modifiers()]
        tree = vine_group(grow_group())
        tree["vine_version"] = VERSION
        for md, vals in saved:
            restore(md, vals)
    return tree, leaf, flower, mat


def restore(md, vals):
    """Put back the settings get_inputs read (it gives data-blocks as their names)."""
    kinds = {"Wall": bpy.data.objects, "More Walls": bpy.data.collections, "Leaf": bpy.data.objects,
             "Flower": bpy.data.objects, "Material": bpy.data.materials}
    out = {}
    for key, v in vals.items():
        if key in kinds:
            if isinstance(v, str) and v in kinds[key]:
                out[key] = kinds[key][v]
        elif v is not None:
            out[key] = v
    G.set_inputs(md, out)


def collection():
    c = bpy.data.collections.get(COLL)
    if c is None:
        c = bpy.data.collections.new(COLL)
        bpy.context.scene.collection.children.link(c)
    return c


def wall_bvh(walls, with_instances):
    """The walls in world space; with_instances: with what their modifiers lay on them (stones)."""
    dg = bpy.context.evaluated_depsgraph_get()
    verts, polys = [], []
    originals = set(walls)
    for inst in dg.object_instances:
        ob = inst.object
        if inst.is_instance:
            if not with_instances or inst.parent is None or inst.parent.original not in originals:
                continue
        elif ob.original not in originals:
            continue
        if ob.type != "MESH":
            continue
        me = ob.to_mesh()
        mw = inst.matrix_world.copy()
        base = len(verts)
        verts += [mw @ v.co for v in me.vertices]
        polys += [[base + i for i in f.vertices] for f in me.polygons]
        ob.to_mesh_clear()
    return BVHTree.FromPolygons(verts, polys)


def start_on(walls, p):
    """Where a vine wanting to start at p starts: on the wall, or on the face of a stone in front of it."""
    hit = wall_bvh(walls, False).find_nearest(p, 5.0)
    if hit[0] is None:
        return p
    probe = hit[0] + hit[1] * 0.3                       # from in front of the wall, onto what is there
    hit2 = wall_bvh(walls, True).find_nearest(probe, 1.0)
    return hit2[0] if hit2[0] is not None else hit[0]


def new_vine(walls, at, tree, leaf, flower, mat):
    coll = collection()
    n = 1 + sum(1 for o in coll.objects if o.name.startswith("Vine "))
    while f"Vine {n}" in bpy.data.objects:
        n += 1
    name = f"Vine {n}"
    obj = bpy.data.objects.new(name, bpy.data.meshes.new(name))
    obj.location = start_on(walls, at)
    coll.objects.link(obj)
    md = obj.modifiers.new(NAME, "NODES")
    md.node_group = tree
    G.set_inputs(md, {"Wall": walls[0], "Seed": n, "Leaf": leaf, "Flower": flower, "Material": mat})
    print(f"vine: {name!r} climbing {walls[0].name!r}")
    return obj


def migrate(tree, leaf, flower, mat):
    """Vines made by the first version of this script (an Empty root with a mesh) become live ones."""
    c = bpy.data.collections.get(COLL)
    for root in [o for o in (c.objects if c else []) if o.type == "EMPTY" and o.get("vine_root")]:
        walls = [bpy.data.objects.get(s.strip()) for s in str(root.get("climbs", "")).split(",")]
        walls = [o for o in walls if o is not None and o.type == "MESH"]
        if walls:
            obj = new_vine(walls, root.matrix_world.translation.copy(), tree, leaf, flower, mat)
            G.set_inputs(obj.modifiers[NAME], {
                "Seed": int(root.get("seed", 1)), "Height": float(root.get("height", 2.4)),
                "Branches": int(root.get("branches", 6)), "Wander": float(root.get("wander", 0.55)),
                "Leaves": float(root.get("leaves", 1.0)), "Leaf Size": float(root.get("leaf_size", 1.0)),
                "Flowers": float(root.get("flowers", 0.5))})
        old = bpy.data.objects.get(f"{root.name} | vine")
        if old is not None:
            bpy.data.objects.remove(old)
        bpy.data.objects.remove(root)


def main():
    tree, leaf, flower, mat = ensure()
    migrate(tree, leaf, flower, mat)
    walls = [o for o in bpy.context.selected_objects
             if o.type == "MESH" and not any(md.type == "NODES" and md.node_group == tree for md in o.modifiers)]
    active = bpy.context.view_layer.objects.active
    if active in walls:                                  # the active one is the Wall
        walls.remove(active)
        walls.insert(0, active)
    if walls:
        obj = new_vine(walls, bpy.context.scene.cursor.location.copy(), tree, leaf, flower, mat)
        for o in bpy.context.selected_objects:
            o.select_set(False)
        obj.select_set(True)
        bpy.context.view_layer.objects.active = obj


if __name__ == "__main__":
    main()
