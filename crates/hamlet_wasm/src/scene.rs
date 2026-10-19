//! What a plugin builds in the world in 3D: the `render` functions,
//! which check its scene, meshes and shaders against its grants and
//! the caps, and the systems that keep them on the plugin for the
//! realm's view to build, and take them away when it may no longer.

use bevy::math::{Affine3A, Quat, Vec3, Vec4};
use bevy::prelude::*;
use hamlet_realm_api::scene::{
    Anchor, Finish, Lamp, Material, PARAMS, PluginScene, SceneMesh,
    SceneNode, SceneShader, Shape,
};
use hamlet_sdk::capability::{Capability, Granted};
use hamlet_sdk::plugin::{PluginInfo, Status, valid};

use crate::host::State;
use crate::instance::Instance;
use crate::interface::hamlet::plugin::render::{self as wire, Host};
use crate::limits::Limits;
use crate::rate::Rate;
use crate::{PluginSet, shader};

/// How far a place or a size may be, in metres.
const FAR: f32 = 100_000.0;

/// The smallest scale of a node along an axis.
const THIN: f32 = 0.001;

/// The narrowest and the widest cone of a spot light, in radians.
const NARROW: f32 = 0.02;
const WIDE: f32 = 1.55;

/// Keeps what the plugins build in the world, at most
/// `scene_updates_per_second` times a second each, and takes it away
/// from a plugin that may no longer build.
pub fn plugin(app: &mut App) {
    app.init_resource::<Rebuilt>().add_systems(
        Update,
        (
            take.in_set(PluginSet::Apply),
            prune
                .in_set(PluginSet::Collect)
                .run_if(any_with_component::<PluginScene>),
        ),
    );
}

/// In a plugin's store: what it sent with the `render` functions, as
/// checked.
pub struct Kept {
    limits: Limits,
    site: Anchor,
    nodes: Vec<SceneNode>,
    meshes: Vec<SceneMesh>,
    shaders: Vec<SceneShader>,
    /// Something was sent since the plugin's scene was last kept.
    changed: bool,
    /// Whether the scene kept on the plugin was let in, and with its
    /// shaders.
    shown: bool,
    shaded: bool,
}

impl Kept {
    pub fn new(limits: &Limits) -> Self {
        Self {
            limits: limits.clone(),
            site: Anchor::Point(Vec3::ZERO),
            nodes: Vec::new(),
            meshes: Vec::new(),
            shaders: Vec::new(),
            changed: false,
            shown: false,
            shaded: false,
        }
    }
}

/// The scenes each plugin had kept lately.
#[derive(Resource, Default)]
pub struct Rebuilt(Rate);

impl Host for State {
    fn build(&mut self, scene: wire::Scene) -> Result<(), String> {
        self.require(Capability::RenderScene)?;
        let site = site(scene.site)?;
        let nodes = nodes(scene.nodes, &self.render)?;
        fits(&nodes, &self.render.meshes, &self.render.limits)?;
        let kept = &mut self.render;
        if (kept.site != site) || kept.nodes != nodes {
            kept.site = site;
            kept.nodes = nodes;
            kept.changed = true;
        }
        Ok(())
    }

    fn mesh(
        &mut self,
        id: String,
        positions: Vec<wire::Vec3>,
        normals: Vec<wire::Vec3>,
        indices: Vec<u32>,
    ) -> Result<(), String> {
        self.require(Capability::RenderScene)?;
        let kept = &mut self.render;
        let mesh = meshed(id, positions, normals, indices, kept)?;
        let mut meshes = kept.meshes.clone();
        match meshes.iter_mut().find(|held| held.id == mesh.id) {
            Some(held) => *held = mesh,
            None => meshes.push(mesh),
        }
        fits(&kept.nodes, &meshes, &kept.limits)?;
        if meshes != kept.meshes {
            kept.meshes = meshes;
            kept.changed = true;
        }
        Ok(())
    }

    fn shader(
        &mut self,
        id: String,
        source: String,
    ) -> Result<(), String> {
        self.require(Capability::RenderShader)?;
        let kept = &mut self.render;
        name(&id)?;
        let at = kept.shaders.iter().position(|held| held.id == id);
        if at.is_none() && kept.shaders.len() >= kept.limits.shaders {
            return Err(format!(
                "{} shaders, the most is {}",
                kept.shaders.len() + 1,
                kept.limits.shaders
            ));
        }
        let checked = shader::checked(&id, &source, &kept.limits)?;
        match at {
            Some(at) if kept.shaders[at] == checked => return Ok(()),
            Some(at) => kept.shaders[at] = checked,
            None => kept.shaders.push(checked),
        }
        kept.changed = true;
        Ok(())
    }
}

/// Keeps on each plugin what it built, as its grants let it, once a
/// change is due.
pub fn take(
    mut plugins: Query<(
        Entity,
        &PluginInfo,
        &Granted,
        &mut Instance,
        Option<&PluginScene>,
    )>,
    limits: Res<Limits>,
    time: Option<Res<Time>>,
    mut rebuilt: ResMut<Rebuilt>,
    mut commands: Commands,
) {
    let now = time.map_or(0.0, |time| time.elapsed_secs());
    for (plugin, info, granted, mut instance, held) in &mut plugins {
        let shown = granted.holds(Capability::RenderScene);
        let shaded = granted.holds(Capability::RenderShader);
        let kept = &instance.store.data().render;
        if !kept.changed
            && kept.shown == shown
            && kept.shaded == shaded
        {
            continue;
        }
        // A change waits until the plugin may make one again; only
        // its last scene is kept then.
        let most = limits.scene_updates_per_second;
        if kept.changed && !rebuilt.0.within(plugin, now, 1.0, most) {
            continue;
        }
        let kept = &mut instance.store.data_mut().render;
        (kept.changed, kept.shown, kept.shaded) =
            (false, shown, shaded);
        let scene =
            (shown && !kept.nodes.is_empty()).then(|| PluginScene {
                by: info.name.clone(),
                site: kept.site.clone(),
                nodes: kept.nodes.clone(),
                meshes: kept.meshes.clone(),
                shaders: match shaded {
                    true => kept.shaders.clone(),
                    false => Vec::new(),
                },
            });
        match (scene, held) {
            (Some(scene), Some(held)) if scene == *held => {}
            (Some(scene), _) => {
                commands.entity(plugin).insert(scene);
            }
            (None, Some(_)) => {
                commands.entity(plugin).remove::<PluginScene>();
            }
            (None, None) => {}
        }
    }
}

/// Takes away the scene of a plugin that is not running or may no
/// longer build one.
pub fn prune(
    plugins: Query<(Entity, &Status, &Granted), With<PluginScene>>,
    mut commands: Commands,
) {
    for (plugin, status, granted) in &plugins {
        if *status != Status::Running
            || !granted.holds(Capability::RenderScene)
        {
            commands.entity(plugin).remove::<PluginScene>();
        }
    }
}

/// Why a mesh or a shader cannot have the id `id`, if it cannot.
fn name(id: &str) -> Result<(), String> {
    match valid(id) {
        true => Ok(()),
        false => Err(format!(
            "`{id}` is not an id: 1 to 64 of a-z, 0-9, `.`, `_` and \
             `-`"
        )),
    }
}

fn site(wire: wire::Site) -> Result<Anchor, String> {
    Ok(match wire {
        wire::Site::Agent(name) => Anchor::Agent(short(name)?),
        wire::Site::Hamlet(id) => Anchor::Hamlet(short(id)?),
        wire::Site::Point(at) => Anchor::Point(place(at)?),
    })
}

/// A name of an agent or a hamlet, if it is not too long to be one.
fn short(name: String) -> Result<String, String> {
    match name.len() <= 64 {
        true => Ok(name),
        false => Err("a site's name has over 64 bytes".into()),
    }
}

/// The nodes of a scene, each placed after its parent, or what is
/// wrong with them.
fn nodes(
    wire: Vec<wire::Node>,
    kept: &Kept,
) -> Result<Vec<SceneNode>, String> {
    let limits = &kept.limits;
    if wire.len() > limits.scene_nodes {
        return Err(format!(
            "{} nodes, the most is {}",
            wire.len(),
            limits.scene_nodes
        ));
    }
    let lamps =
        wire.iter().filter(|node| node.lamp.is_some()).count();
    if lamps > limits.scene_lights {
        return Err(format!(
            "{lamps} lights, the most is {}",
            limits.scene_lights
        ));
    }
    let mut depths = Vec::<(u32, usize)>::new();
    let mut nodes = Vec::new();
    for node in wire {
        let id = node.id;
        if depths.iter().any(|(seen, _)| *seen == id) {
            return Err(format!("two nodes have the id {id}"));
        }
        let depth = match node.parent {
            Some(parent) => {
                let (_, above) = depths
                    .iter()
                    .find(|(seen, _)| *seen == parent)
                    .ok_or(format!(
                        "node {id}: its parent {parent} is not a \
                         node before it"
                    ))?;
                above + 1
            }
            None => 1,
        };
        if depth > limits.scene_depth {
            return Err(format!(
                "node {id} is {depth} deep, the most is {}",
                limits.scene_depth
            ));
        }
        depths.push((id, depth));
        let turn = Quat::from_xyzw(
            node.rotation.x,
            node.rotation.y,
            node.rotation.z,
            node.rotation.w,
        );
        if !turn.is_finite() || turn.length() < 1e-6 {
            return Err(format!(
                "node {id}: its rotation is not a turn"
            ));
        }
        let scale = place(node.scale)?;
        nodes.push(SceneNode {
            id,
            parent: node.parent,
            translation: place(node.translation)?,
            rotation: turn.normalize(),
            scale: scale.signum()
                * scale.abs().max(Vec3::splat(THIN)),
            shape: node.shape.map(shape).transpose()?,
            material: material(node.material, kept)?,
            lamp: node
                .lamp
                .map(|lamp| lit(lamp, limits))
                .transpose()?,
        });
    }
    Ok(nodes)
}

fn shape(wire: wire::Shape) -> Result<Shape, String> {
    Ok(match wire {
        wire::Shape::Cube => Shape::Cube,
        wire::Shape::Sphere => Shape::Sphere,
        wire::Shape::Cylinder => Shape::Cylinder,
        wire::Shape::Cone => Shape::Cone,
        wire::Shape::Capsule => Shape::Capsule,
        wire::Shape::Plane => Shape::Plane,
        wire::Shape::Torus => Shape::Torus,
        wire::Shape::Custom(id) => {
            name(&id)?;
            Shape::Custom(id)
        }
    })
}

fn material(
    wire: wire::Material,
    kept: &Kept,
) -> Result<Material, String> {
    Ok(match wire {
        wire::Material::Surface(surface) => Material::Surface {
            finish: match surface.finish {
                wire::Finish::Solid => Finish::Solid,
                wire::Finish::Clear => Finish::Clear,
                wire::Finish::Unlit => Finish::Unlit,
            },
            colour: surface.colour.to_be_bytes(),
            glow: number(surface.glow)?
                .clamp(0.0, kept.limits.scene_glow),
            roughness: number(surface.roughness)?.clamp(0.0, 1.0),
            metallic: number(surface.metallic)?.clamp(0.0, 1.0),
        },
        wire::Material::Shaded(shaded) => {
            if !kept
                .shaders
                .iter()
                .any(|held| held.id == shaded.shader)
            {
                return Err(format!(
                    "no shader `{}`: send it with `shader` first",
                    shaded.shader
                ));
            }
            if shaded.params.len() > PARAMS {
                return Err(format!(
                    "{} values for a shader, the most is {PARAMS}",
                    shaded.params.len()
                ));
            }
            let mut params = [Vec4::ZERO; PARAMS];
            for (param, value) in params.iter_mut().zip(shaded.params)
            {
                *param = Vec4::new(
                    number(value.x)?,
                    number(value.y)?,
                    number(value.z)?,
                    number(value.w)?,
                );
            }
            Material::Shader {
                id: shaded.shader,
                params,
            }
        }
    })
}

fn lit(wire: wire::Lamp, limits: &Limits) -> Result<Lamp, String> {
    let [_, r, g, b] = wire.colour.to_be_bytes();
    Ok(Lamp {
        colour: [r, g, b],
        lumens: number(wire.lumens)?.clamp(0.0, limits.scene_lumens),
        range: number(wire.range)?
            .clamp(0.0, limits.scene_light_range),
        cone: wire
            .cone
            .map(|cone| {
                number(cone).map(|cone| cone.clamp(NARROW, WIDE))
            })
            .transpose()?,
    })
}

/// Whether every shape and light of `nodes` stays within the radius
/// about the site, its mesh as big as `meshes` has it.
fn fits(
    nodes: &[SceneNode],
    meshes: &[SceneMesh],
    limits: &Limits,
) -> Result<(), String> {
    let mut placed = Vec::<(u32, Affine3A, f32)>::new();
    for node in nodes {
        let (above, grown) = node
            .parent
            .and_then(|parent| {
                placed.iter().find(|(id, ..)| *id == parent)
            })
            .map_or((Affine3A::IDENTITY, 1.0), |(_, at, grown)| {
                (*at, *grown)
            });
        let at = above
            * Affine3A::from_scale_rotation_translation(
                node.scale,
                node.rotation,
                node.translation,
            );
        let grown = grown * node.scale.abs().max_element();
        let reach = match &node.shape {
            Some(Shape::Custom(id)) => meshes
                .iter()
                .find(|mesh| mesh.id == *id)
                .map(SceneMesh::reach)
                .ok_or(format!(
                    "node {}: no mesh `{id}`: send it with `mesh` \
                     first",
                    node.id
                ))?,
            Some(shape) => shape.reach().unwrap_or(0.0),
            None => 0.0,
        };
        let far = Vec3::from(at.translation).length() + reach * grown;
        if far > limits.scene_radius {
            return Err(format!(
                "node {} reaches {far:.1} m from the site, the most \
                 is {}",
                node.id, limits.scene_radius
            ));
        }
        placed.push((node.id, at, grown));
    }
    Ok(())
}

fn meshed(
    id: String,
    positions: Vec<wire::Vec3>,
    normals: Vec<wire::Vec3>,
    indices: Vec<u32>,
    kept: &Kept,
) -> Result<SceneMesh, String> {
    let limits = &kept.limits;
    name(&id)?;
    let new = !kept.meshes.iter().any(|held| held.id == id);
    if new && kept.meshes.len() >= limits.scene_meshes {
        return Err(format!(
            "{} meshes, the most is {}",
            kept.meshes.len() + 1,
            limits.scene_meshes
        ));
    }
    if positions.len() > limits.scene_mesh_points {
        return Err(format!(
            "{} points, the most is {}",
            positions.len(),
            limits.scene_mesh_points
        ));
    }
    let triangles = indices.len() / 3;
    if indices.is_empty() || !indices.len().is_multiple_of(3) {
        return Err(
            "the indices are not three for each triangle".into()
        );
    }
    if triangles > limits.scene_mesh_triangles {
        return Err(format!(
            "{triangles} triangles, the most is {}",
            limits.scene_mesh_triangles
        ));
    }
    if indices.iter().any(|at| *at as usize >= positions.len()) {
        return Err("an index names no point".into());
    }
    if !normals.is_empty() && normals.len() != positions.len() {
        return Err("the normals are not one for each point".into());
    }
    let points = |list: Vec<wire::Vec3>| {
        list.into_iter()
            .map(|at| place(at).map(|at| at.to_array()))
            .collect::<Result<Vec<_>, _>>()
    };
    Ok(SceneMesh {
        id,
        positions: points(positions)?,
        normals: points(normals)?,
        indices,
    })
}

/// A place or a size, if it is one.
fn place(at: wire::Vec3) -> Result<Vec3, String> {
    Ok(Vec3::new(number(at.x)?, number(at.y)?, number(at.z)?))
}

fn number(value: f32) -> Result<f32, String> {
    match value.is_finite() && value.abs() <= FAR {
        true => Ok(value),
        false => Err(format!("{value} is not a number of the scene")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> State {
        let mut state = State::new(&Limits::default());
        state.granted = Granted::allowing(&[Capability::RenderScene]);
        state
    }

    fn v(x: f32, y: f32, z: f32) -> wire::Vec3 {
        wire::Vec3 { x, y, z }
    }

    fn node(
        id: u32,
        parent: Option<u32>,
        at: wire::Vec3,
    ) -> wire::Node {
        wire::Node {
            id,
            parent,
            translation: at,
            rotation: wire::Vec4 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                w: 1.0,
            },
            scale: v(1.0, 1.0, 1.0),
            shape: Some(wire::Shape::Cube),
            material: wire::Material::Surface(wire::Surface {
                finish: wire::Finish::Solid,
                colour: 0xffffffff,
                glow: 0.0,
                roughness: 0.5,
                metallic: 0.0,
            }),
            lamp: None,
        }
    }

    fn build(
        state: &mut State,
        nodes: Vec<wire::Node>,
    ) -> Result<(), String> {
        state.build(wire::Scene {
            site: wire::Site::Hamlet("home".into()),
            nodes,
        })
    }

    #[test]
    fn a_scene_stays_within_its_space_and_its_tree() {
        let mut state = state();
        let radius = Limits::default().scene_radius;
        // A child is placed from its parent, so two steps that each
        // fit can still reach too far.
        let half = radius / 2.0;
        assert!(
            build(
                &mut state,
                vec![
                    node(1, None, v(half, 0.0, 0.0)),
                    node(2, Some(1), v(1.0, 0.0, 0.0))
                ]
            )
            .is_ok()
        );
        assert!(
            build(
                &mut state,
                vec![
                    node(1, None, v(half, 0.0, 0.0)),
                    node(2, Some(1), v(half, 0.0, 0.0))
                ]
            )
            .is_err()
        );
        assert!(
            build(
                &mut state,
                vec![node(2, Some(1), v(0.0, 0.0, 0.0))]
            )
            .is_err()
        );
        let deep = Limits::default().scene_depth as u32 + 1;
        let chain = (1..=deep)
            .map(|id| {
                node(id, (id > 1).then(|| id - 1), v(0.0, 0.0, 0.0))
            })
            .collect();
        assert!(build(&mut state, chain).is_err());
        assert_eq!(state.render.nodes.len(), 2);
    }
}
