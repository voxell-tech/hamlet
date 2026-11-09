# Prefabs in place of blueprints

A whole model made in Blender, dressed in Hamlet, is what the realm puts
down. What exists now: the blueprint editor, whose graphs put library
assets down (`hamlet_app/src/graphs/blueprint.rs`); meshes added from
`.glb` files that follow their file when it is saved again
(`hamlet_app/src/graphs/assets.rs`, `sources.ron` in the library); shader
graphs compiled to a shader drawn on one preview mesh at a time
(`hamlet_app/src/graphs/shader/preview.rs`); and a realm whose houses are
built in code (`hamlet_realm/src/generator/house.rs`), which reads
nothing from the library.

## Outcome

The PREFAB panel takes the dock place of the BLUEPRINT panel. In it the
chief adds a `.glb`, sees the model as Blender made it, clicks one of its
objects, and gives it a shader graph and a transform. The prefab is saved
in the library, shown in ASSETS, and the realm draws prefabs where it now
builds houses in code.

The blueprint graphs stay in the code, tests and examples included; only
their panel leaves the dock. Nothing that reads or writes a blueprint
file is removed, so the panel can come back.

## What a prefab is

A prefab names one mesh of the library and changes some of its objects.

```ron
(
    model: "meshes/hand_tower.glb",
    parts: {
        "Tower/Wall": (material: Shader("plaster")),
        "Tower/Roof": (material: Shader("slate"), turn: (0.0, 0.0, 0.0, 1.0)),
        "Door": (offset: (0.0, 0.0, 0.05)),
    },
)
```

- An object is a node of the `.glb`'s scene, named by its path of node
  names from the scene's root, as Blender's outliner shows it. A name
  used twice under one parent takes its index, `Window#2`.
- Each object keeps what the file gives it unless the prefab says
  otherwise. A part may set its material, either a shader graph of the
  library by name or the file's own, and its transform, as an offset, a
  turn and a scale on top of the file's.
- An object the file no longer has, after the `.glb` is saved again, is
  kept in the prefab and shown as missing, so a renamed object can be
  given its settings back by hand. Nothing is matched by guessing.
- Prefabs are `.ron` files in the library's `prefabs/`, one to a file, a
  new `Kind::Prefab` beside meshes, shaders and textures. Renaming or
  removing the mesh it names is refused while a prefab names it.

## The editor

Three areas, as Unity lays out a prefab:

- **Hierarchy**: the model's objects as a tree, by their Blender names.
  Clicking one selects it, as clicking it in the view does.
- **View**: the model in the realm's light, the camera turned by
  dragging, moved by dragging with the middle button and brought nearer
  by scrolling. A cube in its corner turns it to look from the front,
  back, left, right, top or bottom, and switches between perspective and
  orthographic, as Unity's scene gizmo does. The keys the blueprint
  preview has (F, Shift+R, B, L, T, Shift+T) do the same. The selected
  object is outlined and carries the move, turn and scale handles of
  `gizmo.rs`.
- **Inspector**: the selected object's material, picked from the
  library's shader graphs or "the file's own", and its offset, turn and
  scale as numbers.

Adding a model is `add_mesh`, so the prefab follows the `.glb`: saving it
again in Blender changes the prefab's model, and the parts keep their
settings. Changes are undone and redone, and saved to the prefab's file
as graphs are, with the same dirty mark.

## Drawing several shaders at once

The shader preview draws one graph, because a Bevy material type has one
fragment shader. A prefab needs as many as its parts use. Each part's
material carries the compiled shader it uses as bind group data, and the
material's `specialize` sets the pipeline's fragment shader from it, so
one material type draws every graph and each graph is compiled once.
This is tried first, on two parts with two graphs, before the editor is
built on it; if it does not hold, the fallback compiles the graphs a
prefab uses into one shader that picks by a number in the material.

## The realm

The realm draws prefabs where it now builds a house in code. Each house
is drawn from its prefab as one mesh per material it uses, so a house
costs as many draws as its distinct shaders, and it moves, rises and
sinks as a house does now: by its root's transform. The Hand's house and
an agent's house each name the prefab they are drawn from; how a house
picks its prefab is the open question below.

## Order of work

1. Draw two parts of one mesh with two shader graphs at once.
2. Read a `.glb` as a tree of named objects, each with its own meshes
   and transform, beside the flattened read the catalogue uses now.
3. The prefab file, `Kind::Prefab` in the library, and its picture in
   ASSETS.
4. The PREFAB panel: view, hierarchy, inspector, picking, handles, the
   view cube, undo and save. The BLUEPRINT panel leaves the dock.
5. The realm draws houses from prefabs.

Each step is usable on its own: after step 4 a prefab can be built and
looked at, and step 5 puts it in the world.

## Open questions

- How the realm picks a house's prefab: one for the Hand and a set the
  agents' houses are rolled from, a prefab chosen per agent by the chief,
  or a prefab per role. Until then the realm keeps building houses in
  code.
- Whether a part may also take a palette colour or a tint over its
  shader, as mesh slots now take palette colours.

## Left out

Prefabs inside prefabs, variants of a prefab, and parts other than
meshes (lights, sounds, sockets as objects) are not part of this. A
prefab's parts are what its `.glb` has; objects are not added or deleted
in Hamlet.
