"""Fit architectural surfaces to landmarks in the supplied 1185 by 1327 portrait."""
import math
from pathlib import Path
import bpy
from mathutils import Vector

bpy.ops.wm.read_factory_settings(use_empty=True)
scene = bpy.context.scene
out = Path(__file__).resolve().parents[1] / "artifacts" / "mayor_portrait"
out.mkdir(parents=True, exist_ok=True)
photo = bpy.data.images.load(r"C:/Users/cheng/Downloads/ChatGPT Image Nov 6, 2026, 05_26_37 PM.png")
photo.pack()
skin = bpy.data.materials.new("Painted reference • projected UV")
skin.use_nodes = True
n = skin.node_tree.nodes
t = n.new("ShaderNodeTexImage")
t.image = photo
em = n.new("ShaderNodeEmission")
skin.node_tree.links.new(t.outputs["Color"], em.inputs["Color"])
skin.node_tree.links.new(em.outputs[0], n.get("Material Output").inputs[0])

def panel(name, points, depth, thickness=0):
    verts = [((x-592.5)/100, depth(x,y), (1220-y)/100) for x,y in points]
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(verts, [], [list(range(len(verts)))])
    mesh.update()
    obj = bpy.data.objects.new(name, mesh)
    scene.collection.objects.link(obj)
    mesh.materials.append(skin)
    uv = mesh.uv_layers.new()
    for loop in mesh.loops:
        x,y = points[loop.vertex_index]
        uv.data[loop.index].uv = (x/1185, 1-y/1327)
    if thickness:
        mod = obj.modifiers.new("Return edge", "SOLIDIFY")
        mod.thickness = thickness
        mod.show_render = False  # Keep the camera-facing paint on its calibrated surface.
    return obj

front=lambda x,y: -.0035*(x-350)
side=lambda x,y: -.8+.008*(x-570)
panel("Entry gable and masonry",[(151,762),(322,151),(575,740),(603,1185),(486,1210),(159,1204)],front, .25)
panel("Garden elevation",[(559,732),(991,751),(981,1189),(578,1216)],side,.2)
panel("Raking bargeboard",[(67,720),(313,141),(330,134),(348,151),(585,727),(560,752),(324,204),(147,753),(89,740)],lambda x,y:front(x,y)-.18,.16)
panel("Sage tiled roof",[(324,137),(757,318),(992,766),(971,782),(558,748)],lambda x,y:(x-324-(y-137)*.38)*.008-.24,.15)
panel("Chimney",[(701,207),(746,194),(745,167),(739,162),(742,153),(772,151),(782,157),(781,166),(775,167),(776,194),(823,206),(822,235),(810,239),(811,381),(717,346),(715,238),(701,232)],lambda x,y:1.55,.35)
# Curved tower surfaces use vertical strips with cylindrical depth.
for i in range(40):
    a,b=[math.pi*k/40 for k in (i,i+1)]
    x1,x2=[951-145*math.cos(v) for v in (a,b)]
    panel("Turret masonry",[(x1,399),(x2,399),(x2,1198),(x1,1198)],lambda x,y:3.8-1.7*math.sqrt(max(0,1-((x-951)/145)**2)))
panel("Turret cap",[(783,405),(837,338),(901,220),(940,134),(951,131),(979,189),(1023,269),(1124,407),(1113,423),(1087,445),(850,424),(803,422)],lambda x,y:2.2-.007*(y-135),.13)
panel("Pennant and finial",[(938,140),(933,128),(940,114),(940,56),(934,50),(934,37),(945,28),(956,36),(958,48),(950,56),(951,61),(978,72),(1005,77),(1020,86),(1024,111),(1044,117),(1068,115),(1087,120),(1094,161),(1074,149),(1042,151),(1020,143),(1015,121),(980,114),(951,106),(951,115),(958,123),(955,136)],lambda x,y:2.1,.04)
panel("Projecting timber dormer",[(619,468),(653,419),(764,372),(778,378),(834,485),(821,506),(802,496),(813,633),(714,629),(705,605),(697,597),(678,493),(632,492)],lambda x,y:-.91,.32)
panel("Entry steps",[(184,1180),(227,1177),(230,1146),(469,1141),(481,1162),(475,1218),(189,1219)],lambda x,y:front(x,y)-.4-(y-1140)*.008,.16)
for name,pts in [
 ("Entry planter",[(82,1205),(99,1150),(96,1101),(120,1054),(156,1037),(188,1043),(223,1073),(239,1121),(212,1151),(195,1208)]),
 ("Flowerpot",[(467,1219),(489,1169),(479,1134),(503,1096),(543,1050),(577,1073),(602,1104),(629,1125),(599,1160),(622,1184),(652,1221)]),
 ("Tower garden",[(882,1205),(902,1157),(897,1120),(921,1093),(913,1063),(949,1020),(981,1018),(1017,1081),(997,1114),(1049,1198)])]:
    panel(name,pts,lambda x,y:-1.95,.06)

# Closed rear volume and the previously requested canopy sit behind the painted elevations.
def block(name, loc, dims, color):
    bpy.ops.mesh.primitive_cube_add(size=1,location=loc)
    o=bpy.context.object
    o.name=name
    o.dimensions=dims
    m=bpy.data.materials.get(name) or bpy.data.materials.new(name)
    m.diffuse_color=(*color,1)
    o.data.materials.append(m)
    return o
block("Rear plaster return",(-1.8,2.2,2.3),(4,3.5,4.5),(.78,.68,.51))
block("Rear timber canopy",(-1.8,4.3,2.7),(2.5,1.5,.15),(.26,.13,.06))
for x in (-2.9,-.7):
    block("Canopy upright",(x,4.9,1.35),(.15,.15,2.7),(.26,.13,.06))
bpy.ops.object.camera_add(location=(0,-25,5.565))
cam=bpy.context.object
cam.rotation_euler=(math.pi/2,0,0)
cam.data.type="ORTHO"
cam.data.ortho_scale=13.27
scene.camera=cam
# Extend clean studio pixels from the two margins across the background.
for top in range(0,1327,12):
    bottom=min(1327,top+12)
    bg=panel("Studio sweep",[(0,top),(1185,top),(1185,bottom),(0,bottom)],lambda x,y:8)
    for loop in bg.data.loops:
        uv=bg.data.uv_layers.active.data[loop.index].uv
        uv.x=.004 if loop.vertex_index in (0,3) else .996
    # Use a narrow clean margin throughout, avoiding house pixels.
    for d in bg.data.uv_layers.active.data:
        d.uv.x=.002+d.uv.x*.012
world=bpy.data.worlds.new("Warm grey studio")
world.color=(.43,.40,.38)
scene.world=world
scene.view_settings.view_transform="Standard"
scene.render.engine="CYCLES"
scene.cycles.samples=2
scene.render.resolution_x=1185
scene.render.resolution_y=1327
scene.render.resolution_percentage=100
for area in bpy.context.screen.areas:
    if area.type=="VIEW_3D":
        area.spaces.active.region_3d.view_perspective="CAMERA"
        area.spaces.active.shading.color_type="TEXTURE"
scene.render.filepath=str(out/"portrait.png")
bpy.ops.wm.save_as_mainfile(filepath=str(out/"Mayor House.blend"))
bpy.ops.render.render(write_still=True)
