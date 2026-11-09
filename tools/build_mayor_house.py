import math
import os
import sys

import bpy
from mathutils import Vector


TAG = "codex_mayor_house"


def args_after_dash():
    return sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []


OUT_DIR = os.path.abspath(args_after_dash()[0] if args_after_dash() else "artifacts/mayor_house")
os.makedirs(OUT_DIR, exist_ok=True)


def material(name, color, roughness=0.65, metallic=0.0, emission=None):
    mat = bpy.data.materials.get(name) or bpy.data.materials.new(name)
    bsdf = next(n for n in mat.node_tree.nodes if n.bl_idname == "ShaderNodeBsdfPrincipled")
    bsdf.inputs["Base Color"].default_value = (*color, 1.0)
    bsdf.inputs["Roughness"].default_value = roughness
    bsdf.inputs["Metallic"].default_value = metallic
    if emission:
        bsdf.inputs["Emission Color"].default_value = (*emission[0], 1.0)
        bsdf.inputs["Emission Strength"].default_value = emission[1]
    return mat


MATS = {
    "plaster": material("MH_Plaster", (0.68, 0.57, 0.40), 0.86),
    "stone": material("MH_Limestone", (0.52, 0.47, 0.37), 0.9),
    "wood": material("MH_WarmWood", (0.15, 0.055, 0.018), 0.72),
    "wood_light": material("MH_DoorWood", (0.28, 0.105, 0.030), 0.72),
    "roof_green": material("MH_RoofSage", (0.16, 0.25, 0.105), 0.82),
    "roof_red": material("MH_RoofTerracotta", (0.46, 0.14, 0.050), 0.82),
    "glass": material("MH_WarmGlass", (0.8, 0.34, 0.055), 0.32, emission=((1.0, 0.31, 0.035), 2.5)),
    "metal": material("MH_BlackIron", (0.035, 0.04, 0.033), 0.35, 0.55),
    "banner": material("MH_BannerGreen", (0.12, 0.27, 0.10), 0.75),
    "gold": material("MH_Gold", (0.88, 0.55, 0.10), 0.32, 0.42),
    "leaf": material("MH_Leaf", (0.20, 0.42, 0.10), 0.78),
    "leaf2": material("MH_LeafLight", (0.38, 0.58, 0.17), 0.8),
    "flower": material("MH_Flower", (0.95, 0.82, 0.62), 0.7),
    "pot": material("MH_TerracottaPot", (0.62, 0.24, 0.10), 0.8),
}


def setup_collection(name):
    old = bpy.data.collections.get(name)
    if old:
        for obj in list(old.all_objects):
            bpy.data.objects.remove(obj, do_unlink=True)
        bpy.data.collections.remove(old)
    coll = bpy.data.collections.new(name)
    bpy.context.scene.collection.children.link(coll)
    return coll


ROOT_COLL = setup_collection("MayorHouse_Model")
SUB = {}
for name in ("Structure", "Roof", "Timber", "Openings", "Details", "Vegetation", "Presentation"):
    c = bpy.data.collections.new(f"MH_{name}")
    ROOT_COLL.children.link(c)
    SUB[name] = c


def link_object(obj, collection):
    for c in list(obj.users_collection):
        c.objects.unlink(obj)
    collection.objects.link(obj)
    obj[TAG] = True
    return obj


def mesh_object(name, verts, faces, collection, mat=None):
    me = bpy.data.meshes.new(name + "_Mesh")
    me.from_pydata(verts, [], faces)
    me.validate()
    me.update()
    obj = bpy.data.objects.new(name, me)
    collection.objects.link(obj)
    obj[TAG] = True
    if mat:
        me.materials.append(mat)
    return obj


def box(name, loc, dims, mat, collection, rot=(0, 0, 0), bevel=0.06):
    x, y, z = (d / 2 for d in dims)
    verts = [(-x,-y,-z),(x,-y,-z),(x,y,-z),(-x,y,-z),(-x,-y,z),(x,-y,z),(x,y,z),(-x,y,z)]
    faces = [(0,3,2,1),(4,5,6,7),(0,1,5,4),(1,2,6,5),(2,3,7,6),(3,0,4,7)]
    obj = mesh_object(name, verts, faces, collection, mat)
    obj.location = loc
    obj.rotation_euler = rot
    if bevel:
        mod = obj.modifiers.new("Soft bevel", "BEVEL")
        mod.width = min(bevel, min(dims) * 0.24)
        mod.segments = 2
    return obj


def cylinder(name, loc, radius, depth, mat, collection, vertices=24, rot=(0,0,0), bevel=0.04):
    bpy.ops.mesh.primitive_cylinder_add(vertices=vertices, radius=radius, depth=depth, location=loc, rotation=rot)
    obj = bpy.context.object
    obj.name = name
    link_object(obj, collection)
    obj.data.materials.append(mat)
    if bevel:
        mod = obj.modifiers.new("Soft bevel", "BEVEL")
        mod.width = bevel
        mod.segments = 2
    return obj


def cone(name, loc, r1, r2, depth, mat, collection, vertices=24):
    bpy.ops.mesh.primitive_cone_add(vertices=vertices, radius1=r1, radius2=r2, depth=depth, location=loc)
    obj = bpy.context.object
    obj.name = name
    link_object(obj, collection)
    obj.data.materials.append(mat)
    mod = obj.modifiers.new("Soft bevel", "BEVEL")
    mod.width = 0.045
    mod.segments = 2
    return obj


def radial_roof_tile(name, center, angle, span, r_outer, r_inner, z_outer, z_inner, mat):
    """One thick, overlapping trapezoid tile following a conical roof slope."""
    cx, cy = center
    a0, a1 = angle-span/2, angle+span/2
    top = [
        (cx+math.cos(a0)*r_outer, cy+math.sin(a0)*r_outer, z_outer),
        (cx+math.cos(a1)*r_outer, cy+math.sin(a1)*r_outer, z_outer),
        (cx+math.cos(a1)*r_inner, cy+math.sin(a1)*r_inner, z_inner),
        (cx+math.cos(a0)*r_inner, cy+math.sin(a0)*r_inner, z_inner),
    ]
    verts = top + [(x, y, z-0.10) for x, y, z in top]
    faces = [(0,1,2,3),(7,6,5,4),(0,4,5,1),(1,5,6,2),(2,6,7,3),(3,7,4,0)]
    obj = mesh_object(name, verts, faces, SUB["Roof"], mat)
    mod = obj.modifiers.new("Soft tile edges", "BEVEL")
    mod.width = 0.035
    mod.segments = 2
    return obj


def uv_sphere(name, loc, scale, mat, collection, segments=12, rings=8):
    bpy.ops.mesh.primitive_uv_sphere_add(segments=segments, ring_count=rings, radius=1.0, location=loc)
    obj = bpy.context.object
    obj.name = name
    obj.scale = scale
    link_object(obj, collection)
    obj.data.materials.append(mat)
    return obj


def gable_prism(name, loc, width, depth, wall_h, peak_h, mat, collection):
    x = width / 2
    y = depth / 2
    z0 = 0
    z1 = wall_h
    z2 = wall_h + peak_h
    verts = [(-x,-y,z0),(x,-y,z0),(x,-y,z1),(0,-y,z2),(-x,-y,z1),
             (-x,y,z0),(x,y,z0),(x,y,z1),(0,y,z2),(-x,y,z1)]
    faces = [(0,1,2,3,4),(5,9,8,7,6),(0,5,6,1),(1,6,7,2),(2,7,8,3),(3,8,9,4),(4,9,5,0)]
    obj = mesh_object(name, verts, faces, collection, mat)
    obj.location = loc
    mod = obj.modifiers.new("Soft bevel", "BEVEL")
    mod.width = 0.045
    mod.segments = 2
    return obj


def beam_between(name, a, b, thickness, mat, collection, depth=None):
    a, b = Vector(a), Vector(b)
    vec = b - a
    mid = (a + b) * 0.5
    d = depth if depth is not None else thickness
    obj = box(name, mid, (thickness, d, vec.length), mat, collection, bevel=0.035)
    obj.rotation_mode = "QUATERNION"
    obj.rotation_quaternion = Vector((0,0,1)).rotation_difference(vec.normalized())
    return obj


def arch_mesh(name, loc, width, height, depth, mat, collection, facing="FRONT", segments=10, bevel=0.035):
    r = width / 2
    spring = height - r
    profile = [(-r, 0), (r, 0)]
    for i in range(segments + 1):
        t = i / segments
        ang = t * math.pi
        profile.append((math.cos(ang) * r, spring + math.sin(ang) * r))
    n = len(profile)
    verts = []
    for side in (-depth/2, depth/2):
        for u, v in profile:
            if facing == "FRONT":
                verts.append((u, side, v))
            else:
                verts.append((side, u, v))
    if facing == "FRONT":
        faces = [tuple(range(n)), tuple(range(n, 2*n))[::-1]]
        for i in range(n):
            j = (i + 1) % n
            faces.append((i, n+i, n+j, j))
    else:
        faces = [tuple(range(n))[::-1], tuple(range(n, 2*n))]
        for i in range(n):
            j = (i + 1) % n
            faces.append((i, j, n+j, n+i))
    obj = mesh_object(name, verts, faces, collection, mat)
    obj.location = loc
    if bevel:
        mod = obj.modifiers.new("Soft bevel", "BEVEL")
        mod.width = bevel
        mod.segments = 2
    return obj


def arch_surround(prefix, loc, width, height, facing="FRONT", scale=1.0):
    x, y, z = loc
    r = width / 2 + 0.16 * scale
    spring = height - width/2
    if facing == "FRONT":
        box(prefix+"_L", (x-r, y, z+spring/2), (0.28*scale,0.28*scale,spring), MATS["stone"], SUB["Openings"], bevel=0.045)
        box(prefix+"_R", (x+r, y, z+spring/2), (0.28*scale,0.28*scale,spring), MATS["stone"], SUB["Openings"], bevel=0.045)
    else:
        box(prefix+"_L", (x, y-r, z+spring/2), (0.28*scale,0.28*scale,spring), MATS["stone"], SUB["Openings"], bevel=0.045)
        box(prefix+"_R", (x, y+r, z+spring/2), (0.28*scale,0.28*scale,spring), MATS["stone"], SUB["Openings"], bevel=0.045)
    for i in range(7):
        ang = math.pi * (i / 6)
        if facing == "FRONT":
            p = (x + math.cos(ang)*r, y, z + spring + math.sin(ang)*r)
            rot = (0, ang-math.pi/2, 0)
            box(prefix+f"_Arch_{i:02d}", p, (0.34*scale,0.30*scale,0.42*scale), MATS["stone"], SUB["Openings"], rot=rot, bevel=0.045)
        else:
            p = (x, y + math.cos(ang)*r, z + spring + math.sin(ang)*r)
            rot = (ang-math.pi/2, 0, 0)
            box(prefix+f"_Arch_{i:02d}", p, (0.30*scale,0.34*scale,0.42*scale), MATS["stone"], SUB["Openings"], rot=rot, bevel=0.045)


def add_window(prefix, loc, width=0.95, height=1.75, facing="FRONT", shutters=False, flowerbox=False):
    x,y,z = loc
    arch_mesh(prefix+"_Glass", loc, width, height, 0.09, MATS["glass"], SUB["Openings"], facing=facing)
    front_offset = -0.07 if facing == "FRONT" else 0.07
    if facing == "FRONT":
        yy=y+front_offset-0.012
        beam_between(prefix+"_DiamondA",(x-width*0.38,yy-0.012,z+0.30),(x+width*0.38,yy-0.012,z+height*0.78),0.055,MATS["wood"],SUB["Openings"],0.045)
        beam_between(prefix+"_DiamondB",(x+width*0.38,yy+0.012,z+0.30),(x-width*0.38,yy+0.012,z+height*0.78),0.055,MATS["wood"],SUB["Openings"],0.045)
        box(prefix+"_V", (x,yy,z+height*0.47), (0.055,0.05,height*0.78), MATS["wood"], SUB["Openings"], bevel=0.015)
        arch_surround(prefix, (x,y+0.01,z), width, height, facing)
        if shutters:
            box(prefix+"_ShutterL", (x-width*0.72,y-0.02,z+height*0.43), (width*0.35,0.12,height*0.72), MATS["roof_green"], SUB["Openings"], rot=(0,0,-0.08), bevel=0.04)
            box(prefix+"_ShutterR", (x+width*0.72,y-0.02,z+height*0.43), (width*0.35,0.12,height*0.72), MATS["roof_green"], SUB["Openings"], rot=(0,0,0.08), bevel=0.04)
        if flowerbox:
            flower_box(prefix+"_Flowers", (x,y-0.22,z-0.05), width*1.15, facing)
    else:
        xx=x+front_offset+0.012
        beam_between(prefix+"_DiamondA",(xx-0.012,y-width*0.38,z+0.30),(xx-0.012,y+width*0.38,z+height*0.78),0.055,MATS["wood"],SUB["Openings"],0.045)
        beam_between(prefix+"_DiamondB",(xx+0.012,y+width*0.38,z+0.30),(xx+0.012,y-width*0.38,z+height*0.78),0.055,MATS["wood"],SUB["Openings"],0.045)
        box(prefix+"_V", (xx,y,z+height*0.47), (0.05,0.055,height*0.78), MATS["wood"], SUB["Openings"], bevel=0.015)
        arch_surround(prefix, (x-0.01,y,z), width, height, facing)


def flower_box(name, loc, width, facing="FRONT"):
    x,y,z = loc
    dims = (width,0.32,0.30) if facing == "FRONT" else (0.32,width,0.30)
    box(name+"_Planter", (x,y,z), dims, MATS["wood_light"], SUB["Details"], bevel=0.05)
    for i in range(7):
        off = (i-3)*width/7
        if facing == "FRONT": p=(x+off,y-0.04,z+0.26)
        else: p=(x+0.04,y+off,z+0.26)
        uv_sphere(name+f"_Leaf_{i}", p, (0.12,0.09,0.09), MATS["leaf2"], SUB["Vegetation"])
        if i%2==0:
            uv_sphere(name+f"_Flower_{i}", (p[0],p[1]-0.02,p[2]+0.08), (0.065,0.045,0.035), MATS["flower"], SUB["Vegetation"], 10,6)


def add_door(prefix, loc, width=1.35, height=2.45, facing="FRONT"):
    x,y,z=loc
    arch_mesh(prefix+"_Leaf", loc, width, height, 0.16, MATS["wood_light"], SUB["Openings"], facing=facing)
    arch_surround(prefix, (x,y-0.03 if facing=="FRONT" else y,z), width, height, facing, 1.15)
    if facing=="FRONT":
        for dz in (0.55,1.35): box(prefix+f"_Band_{dz}",(x,y-0.13,z+dz),(width*0.92,0.07,0.10),MATS["metal"],SUB["Openings"],bevel=0.02)
        cylinder(prefix+"_Knocker",(x+0.38,y-0.18,z+1.1),0.12,0.055,MATS["metal"],SUB["Openings"],16,rot=(math.pi/2,0,0),bevel=0.015)
    else:
        for dz in (0.55,1.35): box(prefix+f"_Band_{dz}",(x+0.13,y,z+dz),(0.07,width*0.92,0.10),MATS["metal"],SUB["Openings"],bevel=0.02)


def add_pot(prefix, loc, scale=1.0):
    x,y,z=loc
    cone(prefix+"_Pot",(x,y,z+0.28*scale),0.30*scale,0.23*scale,0.55*scale,MATS["pot"],SUB["Details"],16)
    for i in range(7):
        ang=i*2*math.pi/7
        uv_sphere(prefix+f"_Leaf_{i}",(x+math.cos(ang)*0.20*scale,y+math.sin(ang)*0.20*scale,z+0.66*scale),(0.14*scale,0.09*scale,0.08*scale),MATS["leaf" if i%2 else "leaf2"],SUB["Vegetation"])
    for i in range(3):
        ang=i*2*math.pi/3
        uv_sphere(prefix+f"_Flower_{i}",(x+math.cos(ang)*0.18*scale,y+math.sin(ang)*0.18*scale,z+0.78*scale),(0.07*scale,0.05*scale,0.04*scale),MATS["flower"],SUB["Vegetation"],10,6)


def save_copy(name):
    bpy.ops.wm.save_as_mainfile(filepath=os.path.join(OUT_DIR,name), copy=True)


# Clean factory scene data.
for obj in list(bpy.data.objects):
    if obj.name not in ROOT_COLL.all_objects:
        bpy.data.objects.remove(obj, do_unlink=True)

# Root and ground.
root = bpy.data.objects.new("MayorHouse_ROOT", None)
ROOT_COLL.objects.link(root)
root[TAG] = True
box("Ground",(0,0,-0.22),(11,9,0.4),material("MH_Ground",(0.70,0.67,0.62),0.95),SUB["Presentation"],bevel=0.12)

# Main mass and round tower.
main = gable_prism("MainHouse",(0,0,0.25),6.4,5.1,5.35,3.65,MATS["plaster"],SUB["Structure"])
tower = cylinder("RoundTower",(3.38,0.72,3.72),1.32,7.45,MATS["plaster"],SUB["Structure"],32,bevel=0.10)

# Foundation blocks and corner quoins.
for i,x in enumerate([-2.85,-2.15,-1.45,-0.75,-0.05,0.65,1.35,2.05,2.75]):
    box(f"FoundationFront_{i:02d}",(x,-2.68,0.35),(0.64,0.38,0.65),MATS["stone"],SUB["Structure"],rot=(0,0,0.018*((i%3)-1)),bevel=0.07)
for side in (-1,1):
    for level in range(5):
        z=0.75+level*1.05
        x=side*3.08
        box(f"Quoin_{side}_{level}",(x,-2.66,z),(0.52,0.38,0.50),MATS["stone"],SUB["Structure"],rot=(0,0,0.04*((level%2)-0.5)),bevel=0.065)

# Main roof slabs and ridge.
roof_ang=math.atan2(3.65,3.2)
sl=4.95
for side in (-1,1):
    x=side*1.60
    rot_y=side*roof_ang
    box(f"MainRoof_{side}",(x,0,7.20),(sl,5.85,0.34),MATS["roof_green"],SUB["Roof"],rot=(0,rot_y,0),bevel=0.11)
box("RoofRidge",(0,0,9.27),(0.42,5.95,0.44),MATS["roof_red"],SUB["Roof"],bevel=0.13)

# Hand-laid modular shingles. The slab remains as weather backing, while these pieces carry
# the readable low-poly tile rhythm from the concept art.
tile_index=0
for side in (-1,1):
    for row in range(9):
        ax=0.38+row*0.36
        x=side*ax
        z=9.20-ax*math.tan(roof_ang)+0.24
        stagger=0.27 if row%2 else 0.0
        for col in range(9):
            ay=-2.55+col*0.60+stagger
            if ay>2.62:
                continue
            is_red=(row*11+col*3+side)%13 in (0,1)
            mat=MATS["roof_red"] if is_red else MATS["roof_green"]
            jitter=(((row*17+col*7+(side+1)*13)%5)-2)*0.01
            box(f"MainShingle_{tile_index:03d}",(x,ay+jitter*2,z+jitter),(0.48,0.66,0.12),mat,SUB["Roof"],rot=(0,side*roof_ang,jitter*1.8),bevel=0.045)
            tile_index+=1

# Tower roof with broad eave and finial.
cylinder("TowerEave",(3.38,0.72,7.48),1.52,0.24,MATS["wood"],SUB["Roof"],32,bevel=0.06)
for layer in range(6):
    r_bottom=1.62-layer*0.225
    r_top=max(0.18,r_bottom-0.31)
    z=7.74+layer*0.37
    mat=MATS["roof_red"] if layer==0 else MATS["roof_green"]
    cone(f"TowerRoofLayer_{layer}",(3.38,0.72,z),r_bottom,r_top,0.46,mat,SUB["Roof"],32)
    count=max(8,16-layer)
    for j in range(count):
        a=2*math.pi*j/count
        tile_mat=MATS["roof_red"] if (j+layer*3)%17==0 else MATS["roof_green"]
        radial_roof_tile(f"TowerTile_{layer}_{j}",(3.38,0.72),a,2*math.pi/count*0.94,r_bottom*1.02,r_top*0.98,z-0.23,z+0.23,tile_mat)
cone("TowerRoofTip",(3.38,0.72,10.00),0.49,0.10,0.84,MATS["roof_green"],SUB["Roof"],32)
cylinder("FlagPole",(3.38,0.72,10.30),0.055,1.9,MATS["gold"],SUB["Details"],16,bevel=0.01)
uv_sphere("FlagFinial",(3.38,0.72,11.28),(0.13,0.13,0.18),MATS["gold"],SUB["Details"],16,10)
flag_verts=[(3.41,0.73,11.01),(4.88,0.73,10.86),(4.58,0.73,10.51),(3.41,0.73,10.58)]
flag=mesh_object("MayorFlag",flag_verts,[(0,1,2,3)],SUB["Details"],MATS["banner"])
solid=flag.modifiers.new("Flag thickness","SOLIDIFY"); solid.thickness=0.035

# Timber structure on facade and gable.
for z in (3.0,5.35):
    box(f"FacadeBeam_{z}",(0,-2.73,z),(6.5,0.26,0.26),MATS["wood"],SUB["Timber"],bevel=0.065)
beam_between("GableTrim_L",(-3.2,-2.74,5.35),(0,-2.74,9.05),0.30,MATS["wood"],SUB["Timber"],0.27)
beam_between("GableTrim_R",(3.2,-2.74,5.35),(0,-2.74,9.05),0.30,MATS["wood"],SUB["Timber"],0.27)
box("GableCenterPost",(0,-2.75,7.05),(0.26,0.26,3.35),MATS["wood"],SUB["Timber"],bevel=0.06)
beam_between("GableBrace_L",(-2.35,-2.76,5.58),(0,-2.76,7.55),0.22,MATS["wood"],SUB["Timber"],0.22)
beam_between("GableBrace_R",(2.35,-2.76,5.58),(0,-2.76,7.55),0.22,MATS["wood"],SUB["Timber"],0.22)
for x in (-2.7,-1.8,-0.9,0,0.9,1.8,2.7):
    box(f"EaveBracket_{x}",(x,-2.86,5.08),(0.20,0.48,0.46),MATS["wood"],SUB["Timber"],rot=(0.2,0,0),bevel=0.05)

# Tower roof brackets.
for i in range(12):
    if i == 0:
        continue
    a=i*2*math.pi/12
    box(f"TowerBracket_{i}",(3.38+math.cos(a)*1.26,0.72+math.sin(a)*1.26,7.28),(0.18,0.18,0.45),MATS["wood"],SUB["Timber"],rot=(0,0,a),bevel=0.035)

save_copy("mayor_house_01_blockout.blend")

# Openings: hand-tower inspired door and selective windows.
add_door("FrontDoor",(-1.35,-2.78,0.48),1.38,2.45,"FRONT")
for i in range(5):
    box(f"FrontDoorPlank_{i}",(-1.35+(i-2)*0.245,-2.875,1.38),(0.19,0.035,1.62),MATS["wood_light"],SUB["Openings"],bevel=0.018)
add_window("FrontGroundWindow",(1.25,-2.78,0.92),0.92,1.72,"FRONT",False,True)
add_window("FrontUpperWindow",(-0.85,-2.78,3.72),1.02,1.86,"FRONT",True,True)

# Tower windows, deliberately sparse.
add_window("TowerFrontWindow",(3.38,-0.61,3.20),0.72,1.62,"FRONT",False,False)
add_window("TowerSideWindow",(4.71,0.72,5.35),0.72,1.62,"SIDE",False,False)

# Small hand-tower gable module on right roof slope.
gable_prism("DormerWall",(1.05,-0.78,6.42),1.28,0.78,1.05,0.72,MATS["plaster"],SUB["Structure"])
for sx in (-1,1):
    box(f"DormerRoof_{sx}",(1.05+sx*0.34,-0.78,7.68),(1.02,1.08,0.20),MATS["roof_red"],SUB["Roof"],rot=(0,sx*math.radians(46),0),bevel=0.07)
add_window("DormerWindow",(1.05,-1.19,6.60),0.62,1.18,"FRONT",False,False)

# Chimney, built from separate pale blocks.
for i in range(5):
    box(f"ChimneyBlock_{i}",(1.62,1.10,7.20+i*0.36),(0.72,0.68,0.40),MATS["stone"],SUB["Details"],rot=(0,0,0.03*((i%2)-0.5)),bevel=0.05)
box("ChimneyCap",(1.62,1.10,8.78),(0.96,0.88,0.25),MATS["stone"],SUB["Details"],bevel=0.06)
cylinder("ChimneyPot",(1.62,1.10,9.08),0.23,0.42,MATS["roof_red"],SUB["Details"],20,bevel=0.04)

# Entry steps.
box("Step_Lower",(-1.35,-3.42,0.12),(2.55,0.95,0.24),MATS["stone"],SUB["Structure"],bevel=0.08)
box("Step_Upper",(-1.35,-3.14,0.33),(2.10,0.70,0.22),MATS["stone"],SUB["Structure"],bevel=0.08)

# Mayor banner, rod and simple gold crest.
cylinder("BannerRod",(2.22,-2.98,2.60),0.055,1.32,MATS["metal"],SUB["Details"],16,rot=(0,math.pi/2,0),bevel=0.012)
box("BannerPanel",(2.34,-2.94,1.98),(0.88,0.06,1.62),MATS["banner"],SUB["Details"],bevel=0.04)
box("BannerCrestV",(2.34,-3.005,2.08),(0.10,0.025,0.55),MATS["gold"],SUB["Details"],rot=(0,0,math.radians(35)),bevel=0.012)
box("BannerCrestW",(2.34,-3.035,2.08),(0.10,0.025,0.55),MATS["gold"],SUB["Details"],rot=(0,0,math.radians(-35)),bevel=0.012)

# Warm iron wall lantern beside the main entry.
box("LanternArm",(-0.30,-2.98,2.52),(0.52,0.08,0.08),MATS["metal"],SUB["Details"],bevel=0.025)
box("LanternTop",(-0.06,-3.02,2.42),(0.32,0.24,0.10),MATS["metal"],SUB["Details"],bevel=0.025)
box("LanternGlow",(-0.06,-3.03,2.18),(0.24,0.18,0.38),MATS["glass"],SUB["Details"],bevel=0.045)
box("LanternBase",(-0.06,-3.02,1.96),(0.28,0.22,0.10),MATS["metal"],SUB["Details"],bevel=0.025)

# Side timber canopy and side door on the visible right wall.
add_door("SideDoor",(3.42,-1.72,0.45),1.02,2.05,"SIDE")
box("CanopyRoof",(4.28,-1.72,2.92),(1.70,2.15,0.20),MATS["wood_light"],SUB["Roof"],rot=(0,math.radians(15),0),bevel=0.08)
for i in range(8):
    y=-2.60+i*0.25
    box(f"CanopyPlank_{i}",(4.29,y,2.98),(1.74,0.22,0.09),MATS["wood_light"],SUB["Roof"],rot=(0,math.radians(15),0.012*((i%3)-1)),bevel=0.035)
for y in (-2.56,-0.88):
    box(f"CanopyPost_{y}",(4.92,y,1.42),(0.22,0.22,2.62),MATS["wood"],SUB["Timber"],bevel=0.05)
    beam_between(f"CanopyBrace_{y}",(4.90,y,2.35),(4.18,y,2.85),0.17,MATS["wood"],SUB["Timber"],0.17)
box("CanopyLedger",(3.55,-1.72,3.02),(0.18,2.18,0.18),MATS["wood"],SUB["Timber"],bevel=0.04)

# Pots and restrained vegetation.
add_pot("Pot_Left",(-2.72,-3.25,0.0),0.92)
add_pot("Pot_Right",(0.25,-3.30,0.0),0.70)

# Simple climbing vine on left facade using curves and flattened leaves.
curve_data=bpy.data.curves.new("MH_VineCurve","CURVE")
curve_data.dimensions="3D"; curve_data.bevel_depth=0.035; curve_data.bevel_resolution=2
spline=curve_data.splines.new("POLY"); pts=[(-2.88,-2.94,0.2),(-2.98,-2.96,1.6),(-2.72,-2.96,3.0),(-2.98,-2.96,4.5),(-2.62,-2.96,5.9)]
spline.points.add(len(pts)-1)
for p,co in zip(spline.points,pts): p.co=(*co,1)
vine=bpy.data.objects.new("FacadeVine",curve_data); SUB["Vegetation"].objects.link(vine); vine[TAG]=True; curve_data.materials.append(MATS["leaf"])
for i,p in enumerate(pts[1:]):
    for side in (-1,1):
        uv_sphere(f"VineLeaf_{i}_{side}",(p[0]+side*0.18,p[1]-0.03,p[2]+0.18*side),(0.17,0.055,0.10),MATS["leaf" if i%2 else "leaf2"],SUB["Vegetation"],10,6)
    if i%2==0: uv_sphere(f"VineFlower_{i}",(p[0],p[1]-0.08,p[2]+0.12),(0.075,0.035,0.06),MATS["flower"],SUB["Vegetation"],10,6)

# Dense storybook foliage clusters: broad flattened leaves, white blossoms and a tower vine.
foliage_paths = {
    "FrontCreeper": [(-2.65,-2.98,0.55),(-2.45,-2.99,1.25),(-2.70,-2.99,2.05),(-2.35,-2.99,2.85),(-2.55,-2.99,3.65),(-2.25,-2.99,4.45),(-2.38,-2.99,5.15)],
    "TowerCreeper": [(4.54,-0.02,0.30),(4.62,-0.10,1.15),(4.47,-0.18,2.00),(4.61,-0.22,2.85),(4.43,-0.28,3.75),(4.55,-0.30,4.60),(4.35,-0.33,5.55),(4.45,-0.34,6.45)],
}
for prefix, path in foliage_paths.items():
    cd=bpy.data.curves.new(prefix+"Curve","CURVE"); cd.dimensions="3D"; cd.bevel_depth=0.028; cd.bevel_resolution=2; cd.materials.append(MATS["leaf"])
    sp=cd.splines.new("POLY"); sp.points.add(len(path)-1)
    for point,co in zip(sp.points,path): point.co=(*co,1)
    ob=bpy.data.objects.new(prefix,cd); SUB["Vegetation"].objects.link(ob); ob[TAG]=True
    for i,p in enumerate(path):
        for side in (-1,1):
            dx=side*(0.16+0.05*(i%2)); dz=side*0.10
            uv_sphere(f"{prefix}_Leaf_{i}_{side}",(p[0]+dx,p[1]-0.04,p[2]+dz),(0.20,0.055,0.12),MATS["leaf2" if (i+side)%3==0 else "leaf"],SUB["Vegetation"],12,7)
        if i%2==0:
            for petal in range(5):
                a=petal*2*math.pi/5
                uv_sphere(f"{prefix}_Flower_{i}_{petal}",(p[0]+math.cos(a)*0.07,p[1]-0.11,p[2]+math.sin(a)*0.07),(0.055,0.025,0.045),MATS["flower"],SUB["Vegetation"],10,6)
            uv_sphere(f"{prefix}_FlowerCore_{i}",(p[0],p[1]-0.13,p[2]),(0.035,0.018,0.035),MATS["gold"],SUB["Vegetation"],10,6)

save_copy("mayor_house_02_details.blend")

# Parent all authored objects to root, except presentation ground.
for obj in list(ROOT_COLL.all_objects):
    if obj != root and obj.name != "Ground" and obj.parent is None:
        obj.parent = root

# Presentation camera and lights.
def point_camera(cam, target):
    direction=Vector(target)-cam.location
    cam.rotation_euler=direction.to_track_quat('-Z','Y').to_euler()

cam_data=bpy.data.cameras.new("MayorHouse_Camera")
cam=bpy.data.objects.new("MayorHouse_Camera",cam_data); SUB["Presentation"].objects.link(cam)
cam.location=(18.0,-20.4,6.75); cam_data.lens=70; point_camera(cam,(0.15,-0.05,5.25)); bpy.context.scene.camera=cam

def area_light(name,loc,energy,size,color):
    data=bpy.data.lights.new(name,"AREA"); data.energy=energy; data.shape="DISK"; data.size=size; data.color=color
    obj=bpy.data.objects.new(name,data); SUB["Presentation"].objects.link(obj); obj.location=loc; obj.rotation_euler=(0,0,0); point_camera(obj,(0,0,4)); return obj

area_light("KeyLight",(-7,-10,14),1750,6.0,(1.0,0.83,0.67))
area_light("FillLight",(8,-5,8),720,6.0,(0.72,0.80,1.0))
area_light("RimLight",(2,7,12),1050,5.0,(1.0,0.72,0.50))

world=bpy.context.scene.world or bpy.data.worlds.new("MayorHouseWorld")
bpy.context.scene.world=world
bg=next(n for n in world.node_tree.nodes if n.type=="BACKGROUND")
bg.inputs["Color"].default_value=(0.43,0.41,0.40,1)
bg.inputs["Strength"].default_value=0.42

scene=bpy.context.scene
scene.render.engine="BLENDER_EEVEE"
scene.render.resolution_x=900; scene.render.resolution_y=900; scene.render.resolution_percentage=100
scene.render.image_settings.file_format="PNG"
scene.render.film_transparent=False
scene.view_settings.look="AgX - Very High Contrast" if "AgX - Very High Contrast" in [i.name for i in bpy.types.ColorManagedViewSettings.bl_rna.properties['look'].enum_items] else scene.view_settings.look
scene.render.filepath=os.path.join(OUT_DIR,"mayor_house_preview.png")
scene.render.image_settings.color_mode="RGBA"

# Select authored root for easy handoff.
bpy.ops.object.select_all(action="DESELECT")
root.select_set(True); bpy.context.view_layer.objects.active=root
bpy.context.view_layer.update()

final_path=os.path.join(OUT_DIR,"mayor_house_final.blend")
bpy.ops.wm.save_as_mainfile(filepath=final_path)
bpy.ops.render.render(write_still=True)

mesh_objs=[o for o in ROOT_COLL.all_objects if o.type=="MESH" and o.name!="Ground"]
triangles=sum(len(p.vertices)-2 for o in mesh_objs for p in o.data.polygons)
print("RESULT",{"file":final_path,"preview":scene.render.filepath,"mesh_objects":len(mesh_objs),"triangles_base":triangles,"materials":len(MATS)})
