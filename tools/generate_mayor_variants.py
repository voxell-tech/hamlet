import bpy, math
from pathlib import Path
from mathutils import Vector

bpy.ops.wm.read_factory_settings(use_empty=True)
OUT = Path(__file__).resolve().parents[1] / "artifacts" / "mayor_house_variants"
OUT.mkdir(parents=True, exist_ok=True)

def mat(name, color):
    m=bpy.data.materials.new(name); m.use_nodes=True; m.diffuse_color=(*color,1)
    bsdf=m.node_tree.nodes.get("Principled BSDF"); bsdf.inputs["Base Color"].default_value=(*color,1); bsdf.inputs["Roughness"].default_value=.72
    return m
PLASTER=mat("Cream plaster",(.78,.66,.48)); STONE=mat("Pale stone",(.55,.50,.41)); WOOD=mat("Dark timber",(.20,.075,.025)); GREEN=mat("Sage shingles",(.22,.32,.14)); RED=mat("Terracotta shingles",(.58,.19,.07)); GLASS=mat("Warm windows",(.95,.43,.07)); LEAF=mat("Leaves",(.20,.42,.09)); GOLD=mat("Gold",(.82,.48,.06)); BANNER=mat("Mayor green",(.10,.27,.08))

def cube(c,n,l,d,m,rot=(0,0,0)):
    bpy.ops.mesh.primitive_cube_add(size=1,location=l,rotation=rot); o=bpy.context.object; o.name=n; o.dimensions=d; o.data.materials.append(m); c.objects.link(o); bpy.context.collection.objects.unlink(o); bevel=o.modifiers.new("Soft edges","BEVEL"); bevel.width=min(d)*.06; bevel.segments=2; return o
def cyl(c,n,l,r,h,m):
    bpy.ops.mesh.primitive_cylinder_add(vertices=20,radius=r,depth=h,location=l); o=bpy.context.object; o.name=n; o.data.materials.append(m); c.objects.link(o); bpy.context.collection.objects.unlink(o); return o
def cone(c,n,l,a,b,h,m):
    bpy.ops.mesh.primitive_cone_add(vertices=24,radius1=a,radius2=b,depth=h,location=l); o=bpy.context.object; o.name=n; o.data.materials.append(m); c.objects.link(o); bpy.context.collection.objects.unlink(o); return o
def beam(c,n,a,b,w=.16):
    a,b=Vector(a),Vector(b); v=b-a; o=cube(c,n,(a+b)/2,(w,w,v.length),WOOD); o.rotation_mode='QUATERNION'; o.rotation_quaternion=Vector((0,0,1)).rotation_difference(v.normalized()); return o
def arch(c,n,x,y,z,w,h):
    # warm arched hand-house window, structured from a rectangle and a half-round head
    cube(c,n+" glass",(x,y,z+h*.43),(w*.62,.08,h*.68),GLASS)
    cyl(c,n+" head",(x,y,z+h*.78),w*.31,.09,GLASS).rotation_euler=(math.pi/2,0,0)
    cube(c,n+" mullion",(x,y-.05,z+h*.45),(.06,.06,h*.75),WOOD)
    beam(c,n+" diamond1",(x-w*.25,y-.06,z+h*.2),(x+w*.25,y-.06,z+h*.65),.045)
    beam(c,n+" diamond2",(x+w*.25,y-.065,z+h*.2),(x-w*.25,y-.065,z+h*.65),.045)
    for sx in (-1,1): cube(c,n+str(sx),(x+sx*w*.38,y,z+h*.38),(.12,.16,h*.72),STONE)
def roof(c,n,x,y,z,w,d,h):
    # two steep roof planes and hand-laid tile rows
    ang=math.atan2(h,w/2)
    for s in (-1,1):
        cube(c,n+str(s),(x+s*w/4,y,z+h/2),(math.hypot(w/2,h),d,.18),GREEN,(0,s*ang,0))
    cube(c,n+" ridge",(x,y,z+h),(w*.08,d+.12,.20),RED)
    for s in (-1,1):
        for r in range(6):
            xx=x+s*(.28+r*w*.075); zz=z+h-.25-r*h*.14
            for k in range(5): cube(c,n+f" tile{s}{r}{k}",(xx,y-d*.38+k*d*.19,zz),(.43,.55,.07),RED if (r+k)%7==0 else GREEN,(0,s*ang,0))
def tower(c,x,y):
    # locked family tower used by every option
    cyl(c,"Shared tower plaster",(x,y,3.6),1.25,7.2,PLASTER)
    cyl(c,"Shared tower wood eave",(x,y,7.25),1.42,.23,WOOD)
    for i in range(5): cone(c,"Shared tower shingles",(x,y,7.55+i*.43),1.55-i*.25,1.25-i*.25,.5,GREEN)
    cone(c,"Shared tower cap",(x,y,9.95),.52,.08,.9,GREEN); cyl(c,"Shared flag pole",(x,y,10.8),.045,1.4,GOLD); bpy.ops.mesh.primitive_uv_sphere_add(segments=12,ring_count=8,radius=.13,location=(x,y,11.5)); o=bpy.context.object; o.data.materials.append(GOLD); c.objects.link(o); bpy.context.collection.objects.unlink(o)
    for z in (2.5,4.8): arch(c,"Tower window",x,y-1.27,z,.62,1.35)
def common(c,ox,title,body=(5.5,4.6,4.5)):
    w,d,h=body; cube(c,title+" foundation",(ox,-.15,.2),(w+.3,d+.3,.4),STONE); cube(c,title+" plaster",(ox,-.15,h/2),(w,d,h),PLASTER)
    roof(c,title+" roof",ox,-.15,h,w,d,3.1); tower(c,ox+w*.47,.7)
    cube(c,title+" door",(ox-w*.18,-d/2-.05,1.15),(1.1,.16,2.2),WOOD); arch(c,title+" upper window",ox-w*.18,-d/2-.08,2.65,.82,1.45); arch(c,title+" lower window",ox+w*.22,-d/2-.08,.95,.76,1.3)
    beam(c,title+" eave",(ox-w/2,-d/2-.12,h-.1),(ox+w/2,-d/2-.12,h-.1),.18)
    return w,d,h
def plants(c,x,y):
    for i in range(7):
        bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=2,radius=.15,location=(x+math.sin(i)*.3,y-.08,.32+i*.38)); o=bpy.context.object; o.data.materials.append(LEAF); c.objects.link(o); bpy.context.collection.objects.unlink(o)

ground=bpy.data.collections.new("Options"); bpy.context.scene.collection.children.link(ground)
for ox,kind in [(-8,"A_Frame"),(0,"Cross_Gable"),(8,"Bay_Wing")]:
    c=bpy.data.collections.new(kind); ground.children.link(c); w,d,h=common(c,ox,kind)
    if kind=="A_Frame":
        # tall narrow body and one small front dormer
        cube(c,"A porch",(ox-.4,-3.0,1.0),(2.4,1.1,.18),WOOD); cube(c,"A porch posts",(ox-1.4,-3.35,1.2),(.14,.14,2.2),WOOD); cube(c,"A porch posts",(ox+.6,-3.35,1.2),(.14,.14,2.2),WOOD)
    elif kind=="Cross_Gable":
        cube(c,"Cross wing",(ox-2.8,-1.35,2.1),(2.3,2.1,3.8),PLASTER); roof(c,"Cross wing roof",ox-2.8,-1.35,3.8,2.6,2.3,2.0); arch(c,"Cross wing window",ox-2.8,-2.46,1.0,.72,1.25)
    else:
        cube(c,"Bay wing",(ox-2.35,-1.75,1.55),(1.8,1.6,2.7),PLASTER); cone(c,"Bay wing roof",(ox-2.35,-1.75,3.25),1.35,0,1.4,GREEN); cube(c,"Canopy",(ox+1.4,-2.8,2.5),(2.0,1.2,.16),WOOD); cube(c,"Canopy post",(ox+2.2,-3.25,1.25),(.16,.16,2.5),WOOD)
    plants(c,ox-w*.42,-d/2-.13); plants(c,ox+w*.44,-d/2-.14)

# neutral ground and camera
cube(ground,"Ground",(0,0,-.15),(26,10,.25),mat("Ground",(.35,.32,.27)))
bpy.ops.object.camera_add(location=(15,-30,15)); cam=bpy.context.object; bpy.context.scene.camera=cam; cam.data.lens=52; cam.rotation_euler=(math.radians(66),0,math.radians(27))
def look(o,p): o.rotation_euler=(Vector(p)-o.location).to_track_quat('-Z','Y').to_euler()
look(cam,(0,0,3.6))
bpy.ops.object.light_add(type='AREA',location=(-8,-10,17)); bpy.context.object.data.energy=2200; bpy.context.object.data.shape='DISK'; bpy.context.object.data.size=10; look(bpy.context.object,(0,0,3))
bpy.ops.object.light_add(type='AREA',location=(8,-4,10)); bpy.context.object.data.energy=1000; bpy.context.object.data.size=8; look(bpy.context.object,(0,0,3))
scene=bpy.context.scene; scene.render.engine='BLENDER_EEVEE'; scene.render.resolution_x=1600; scene.render.resolution_y=900; scene.render.resolution_percentage=100; scene.render.image_settings.file_format='PNG'; scene.render.filepath=str(OUT/'mayor_house_variants.png'); scene.world=bpy.data.worlds.new('World'); scene.world.color=(.17,.15,.13)
bpy.ops.wm.save_as_mainfile(filepath=str(OUT/'mayor_house_variants.blend')); bpy.ops.render.render(write_still=True)
