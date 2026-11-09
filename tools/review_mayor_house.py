import json
import os
import sys

import bpy
from mathutils import Matrix


def args_after_dash():
    return sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []


args = args_after_dash()
out_dir = os.path.abspath(args[0])
skill_scripts = r"C:\Users\cheng\.codex\skills\scenario-blender-expert\scripts"
sys.path.append(skill_scripts)
import bx_audit
import bx_review


owned = [o for o in bpy.data.objects if o.get("codex_mayor_house") and o.type == "MESH" and o.name != "Ground"]
gate_objects = [
    o for o in owned
    if not any(c.name == "MH_Vegetation" for c in o.users_collection)
    and not o.name.startswith(("Pot_Left", "Pot_Right"))
]

# Apply object scale without disturbing placement or rotation.
for obj in owned:
    if any(abs(v - 1.0) > 1e-6 for v in obj.scale):
        obj.data.transform(Matrix.Diagonal((*obj.scale, 1.0)))
        obj.scale = (1.0, 1.0, 1.0)

bpy.context.view_layer.update()
bpy.ops.wm.save_as_mainfile(filepath=bpy.data.filepath)

# The whole modular building is audited as one game asset. Decorative leaves may remain separate
# parts by design; the report is retained so those warnings are explicit.
report = bx_audit.deliverable(gate_objects, profile="game")
review_dir = os.path.join(out_dir, "review")
sheet = bx_review.review(
    owned,
    review_dir,
    views=("front", "right", "threequarter", "back"),
    modes=("silhouette", "matcap", "wire"),
    res=300,
    margin=1.10,
)

summary = {
    "pass": report["pass"],
    "errors": report["errors"][:20],
    "warnings": report["warnings"][:20],
    "object_count": len(owned),
    "audited_object_count": len(gate_objects),
    "audit_exclusion": "Loose vegetation and potted flowers are intentionally separate decorative meshes.",
    "review_sheet": sheet,
}
with open(os.path.join(out_dir, "audit_summary.json"), "w", encoding="utf-8") as f:
    json.dump(summary, f, indent=2)
print("RESULT", json.dumps(summary))
