//! What plugins build in the world: each `PluginScene` made entities
//! under a root kept at its site, linked to the plugin, changed only
//! where a node, mesh or shader changed, and named over it.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::ecs::system::EntityCommands;
use bevy::light::NotShadowCaster;
use bevy::math::Affine3A;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::text::FontSize;
use bevy::transform::TransformSystems;
use bevy::ui::UiSystems;
use hamlet_realm::label::Lens;
use hamlet_realm::needs::NeedTag;
use hamlet_realm::survey::Whereabouts;
use hamlet_realm_api::scene::{
    Anchor, Finish, Lamp, Material as Paint, PluginScene, SceneMesh,
    SceneNode, Shape, Staged, StagedBy,
};
use hamlet_sdk::plugin::PluginInfo;
use hamlet_sdk::theme::Themed;

use super::overlay;
use super::shader::{self, PluginMaterial, ShadersOff};

/// The gap between a scene and its name over it, in pixels.
const GAP: f32 = 6.0;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Shapes>()
        .init_resource::<PluginShaders>()
        .add_observer(clear)
        .add_systems(
            Update,
            build.run_if(any_match_filter::<Changed<PluginScene>>),
        )
        .add_systems(
            PostUpdate,
            (
                place.before(TransformSystems::Propagate),
                name.after(place).before(UiSystems::Layout),
            )
                .run_if(any_with_component::<Stage>),
        );
}

/// The root of a plugin's scene in the world: what it was built from,
/// and the entities and assets made of it.
#[derive(Component, Default)]
pub struct Stage {
    site: Option<Anchor>,
    /// The height over the site the scene reaches up to.
    top: f32,
    nodes: HashMap<u32, (Entity, SceneNode)>,
    meshes: HashMap<String, (SceneMesh, Handle<Mesh>)>,
    shaders: HashMap<String, (String, Handle<Shader>)>,
}

/// The name of the plugin over its scene, which hangs off the world's
/// panel.
#[derive(Component)]
pub struct Named(Entity);

/// Hamlet's own shapes, one metre across.
#[derive(Resource)]
struct Shapes(HashMap<&'static str, Handle<Mesh>>);

/// The shader asset of each shader id of each plugin, kept for the
/// run: a pipeline whose shader is dropped would be tried again every
/// frame.
#[derive(Resource, Default)]
struct PluginShaders(HashMap<(String, String), Handle<Shader>>);

impl FromWorld for Shapes {
    fn from_world(world: &mut World) -> Self {
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        let mut add = |name, mesh: Mesh| (name, meshes.add(mesh));
        Self(HashMap::from([
            add("cube", Cuboid::from_length(1.0).into()),
            add("sphere", Sphere::new(0.5).mesh().uv(32, 18)),
            add("cylinder", Cylinder::new(0.5, 1.0).into()),
            add(
                "cone",
                Cone {
                    radius: 0.5,
                    height: 1.0,
                }
                .into(),
            ),
            add("capsule", Capsule3d::new(0.25, 0.5).into()),
            add(
                "plane",
                Plane3d::default().mesh().size(1.0, 1.0).into(),
            ),
            add("torus", Torus::new(0.25, 0.5).into()),
        ]))
    }
}

impl Shapes {
    fn of(
        &self,
        shape: &Shape,
        stage: &Stage,
    ) -> Option<Handle<Mesh>> {
        let name = match shape {
            Shape::Cube => "cube",
            Shape::Sphere => "sphere",
            Shape::Cylinder => "cylinder",
            Shape::Cone => "cone",
            Shape::Capsule => "capsule",
            Shape::Plane => "plane",
            Shape::Torus => "torus",
            Shape::Custom(id) => {
                return stage
                    .meshes
                    .get(id)
                    .map(|(_, mesh)| mesh.clone());
            }
        };
        self.0.get(name).cloned()
    }
}

/// The assets the nodes of a scene are made with.
struct Making<'a> {
    shapes: &'a Shapes,
    standard: &'a mut Assets<StandardMaterial>,
    shaded: &'a mut Assets<PluginMaterial>,
    /// No plugin's shader is drawn.
    off: bool,
}

/// Builds each scene that changed: its meshes and shaders that are
/// new or other, then its nodes that are.
fn build(
    mut commands: Commands,
    scenes: Query<
        (Entity, &PluginScene, Option<&Staged>, Option<&PluginInfo>),
        Changed<PluginScene>,
    >,
    mut stages: Query<&mut Stage>,
    mut names: Query<(&Named, &Children)>,
    mut texts: Query<&mut Text>,
    shapes: Res<Shapes>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut shaders: ResMut<Assets<Shader>>,
    mut kept: ResMut<PluginShaders>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut shaded: ResMut<Assets<PluginMaterial>>,
    off: Option<Res<ShadersOff>>,
    theme: Themed,
) {
    for (plugin, scene, staged, info) in &scenes {
        let held = staged
            .into_iter()
            .flat_map(|staged| staged.iter())
            .find(|root| stages.contains(*root));
        let (root, mut stage) = match held {
            Some(root) => {
                let mut stage = stages.get_mut(root).unwrap();
                (root, std::mem::take(&mut *stage))
            }
            None => {
                let root = commands
                    .spawn((
                        StagedBy(plugin),
                        Name::new(format!("scene of {}", scene.by)),
                        Transform::default(),
                        Visibility::Hidden,
                    ))
                    .id();
                tag(&mut commands, plugin, root, &scene.by, &theme);
                (root, Stage::default())
            }
        };
        for (named, children) in &mut names {
            if named.0 == root {
                for child in children {
                    if let Ok(mut text) = texts.get_mut(*child) {
                        text.0.clone_from(&scene.by);
                    }
                }
            }
        }
        let id =
            info.map_or(scene.by.as_str(), |info| info.id.as_str());
        let remade = stock(
            scene,
            id,
            &mut stage,
            &mut meshes,
            &mut shaders,
            &mut kept,
        );
        let mut making = Making {
            shapes: &shapes,
            standard: &mut standard,
            shaded: &mut shaded,
            off: off.is_some(),
        };
        let mut kept = HashMap::new();
        for node in &scene.nodes {
            let parent = node
                .parent
                .and_then(|parent| kept.get(&parent).copied())
                .unwrap_or(root);
            let entity = match stage.nodes.get(&node.id) {
                Some((entity, was))
                    if was == node && !remade.touches(node) =>
                {
                    *entity
                }
                Some((entity, _)) => {
                    let mut drawn = commands.entity(*entity);
                    drawn.remove::<(
                        Mesh3d,
                        MeshMaterial3d<StandardMaterial>,
                        MeshMaterial3d<PluginMaterial>,
                        PointLight,
                        SpotLight,
                    )>();
                    dress(
                        &mut drawn,
                        node,
                        parent,
                        &stage,
                        &mut making,
                    );
                    *entity
                }
                None => {
                    let mut drawn = commands.spawn_empty();
                    dress(
                        &mut drawn,
                        node,
                        parent,
                        &stage,
                        &mut making,
                    );
                    drawn.id()
                }
            };
            kept.insert(node.id, entity);
        }
        // The nodes that stay have their parents by now, so a node
        // that goes takes only those that go with it.
        for (id, (entity, _)) in &stage.nodes {
            if !kept.contains_key(id) {
                commands.entity(*entity).try_despawn();
            }
        }
        stage.nodes = scene
            .nodes
            .iter()
            .map(|node| (node.id, (kept[&node.id], node.clone())))
            .collect();
        stage.site = Some(scene.site.clone());
        stage.top = top(scene);
        commands.entity(root).insert(stage);
    }
}

/// The meshes and shaders that were made again for a scene.
struct Remade {
    meshes: Vec<String>,
    shaders: Vec<String>,
}

impl Remade {
    /// Whether `node` is made with one of them.
    fn touches(&self, node: &SceneNode) -> bool {
        let mesh = matches!(
            &node.shape,
            Some(Shape::Custom(id)) if self.meshes.contains(id)
        );
        let shader = matches!(
            &node.material,
            Paint::Shader { id, .. } if self.shaders.contains(id)
        );
        mesh || shader
    }
}

/// Makes the meshes and shaders of `scene`, by the plugin `plugin`,
/// that `stage` does not hold as they are, and forgets those it no
/// longer has. A shader that goes counts as made again, so that its
/// nodes are drawn plain.
fn stock(
    scene: &PluginScene,
    plugin: &str,
    stage: &mut Stage,
    meshes: &mut Assets<Mesh>,
    shaders: &mut Assets<Shader>,
    kept: &mut PluginShaders,
) -> Remade {
    let mut remade = Remade {
        meshes: Vec::new(),
        shaders: Vec::new(),
    };
    stage
        .meshes
        .retain(|id, _| scene.meshes.iter().any(|m| m.id == *id));
    for sent in &scene.meshes {
        if stage
            .meshes
            .get(&sent.id)
            .is_some_and(|(held, _)| held == sent)
        {
            continue;
        }
        let handle = meshes.add(mesh(sent));
        stage.meshes.insert(sent.id.clone(), (sent.clone(), handle));
        remade.meshes.push(sent.id.clone());
    }
    stage.shaders.retain(|id, _| {
        let sent = scene.shaders.iter().any(|s| s.id == *id);
        if !sent {
            remade.shaders.push(id.clone());
        }
        sent
    });
    for sent in &scene.shaders {
        let held = stage.shaders.get(&sent.id);
        if held.is_some_and(|(source, _)| *source == sent.source) {
            continue;
        }
        let compiled =
            shader::compiled(plugin, &sent.id, &sent.source);
        // One asset for each shader of each plugin, replaced in
        // place, so that its pipeline is made again and none is left
        // without its shader.
        let key = (plugin.to_string(), sent.id.clone());
        let handle = match kept.0.get(&key) {
            Some(handle) => {
                if let Some(mut held) = shaders.get_mut(handle) {
                    *held = compiled;
                }
                handle.clone()
            }
            None => {
                let handle = shaders.add(compiled);
                kept.0.insert(key, handle.clone());
                handle
            }
        };
        stage
            .shaders
            .insert(sent.id.clone(), (sent.source.clone(), handle));
        remade.shaders.push(sent.id.clone());
    }
    remade
}

/// Gives the entity of `node` all it is drawn with.
fn dress(
    drawn: &mut EntityCommands,
    node: &SceneNode,
    parent: Entity,
    stage: &Stage,
    making: &mut Making,
) {
    drawn.insert((
        Transform {
            translation: node.translation,
            rotation: node.rotation,
            scale: node.scale,
        },
        Visibility::Inherited,
        Pickable::IGNORE,
        ChildOf(parent),
    ));
    if let Some(mesh) = node
        .shape
        .as_ref()
        .and_then(|shape| making.shapes.of(shape, stage))
    {
        drawn.insert(Mesh3d(mesh));
        match &node.material {
            Paint::Shader { id, params } => match stage
                .shaders
                .get(id)
                .filter(|_| !making.off)
            {
                Some((_, shader)) => {
                    let material =
                        PluginMaterial::new(shader.clone(), *params);
                    drawn.insert((
                        MeshMaterial3d(making.shaded.add(material)),
                        NotShadowCaster,
                    ));
                }
                // Without `render.shader`, or after the renderer
                // failed, it is drawn plain.
                None => {
                    drawn.insert(MeshMaterial3d(
                        making
                            .standard
                            .add(StandardMaterial::default()),
                    ));
                }
            },
            surface => {
                drawn.insert(MeshMaterial3d(
                    making.standard.add(material(surface)),
                ));
            }
        }
    }
    if let Some(lamp) = &node.lamp {
        lit(drawn, lamp);
    }
}

fn material(paint: &Paint) -> StandardMaterial {
    let Paint::Surface {
        finish,
        colour: [r, g, b, a],
        glow,
        roughness,
        metallic,
    } = *paint
    else {
        return StandardMaterial::default();
    };
    let colour = Color::srgba_u8(r, g, b, a);
    StandardMaterial {
        base_color: colour,
        emissive: LinearRgba::from(colour) * glow,
        perceptual_roughness: roughness,
        metallic,
        unlit: finish == Finish::Unlit,
        alpha_mode: match finish {
            Finish::Clear => AlphaMode::Blend,
            _ => AlphaMode::Opaque,
        },
        ..default()
    }
}

fn lit(drawn: &mut EntityCommands, lamp: &Lamp) {
    let [r, g, b] = lamp.colour;
    let color = Color::srgb_u8(r, g, b);
    match lamp.cone {
        None => drawn.insert(PointLight {
            color,
            intensity: lamp.lumens,
            range: lamp.range,
            shadow_maps_enabled: false,
            ..default()
        }),
        Some(cone) => drawn.insert(SpotLight {
            color,
            intensity: lamp.lumens,
            range: lamp.range,
            outer_angle: cone,
            inner_angle: cone * 0.8,
            shadow_maps_enabled: false,
            ..default()
        }),
    };
}

fn mesh(sent: &SceneMesh) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_POSITION,
        sent.positions.clone(),
    )
    .with_inserted_indices(Indices::U32(sent.indices.clone()));
    match sent.normals.is_empty() {
        true => mesh.compute_smooth_normals(),
        false => mesh.insert_attribute(
            Mesh::ATTRIBUTE_NORMAL,
            sent.normals.clone(),
        ),
    }
    mesh
}

/// The height over the site the nodes of `scene` reach up to, by the
/// places of their middles.
fn top(scene: &PluginScene) -> f32 {
    let mut at = HashMap::<u32, Affine3A>::new();
    let mut top = 0.0f32;
    for node in &scene.nodes {
        let above = node
            .parent
            .and_then(|parent| at.get(&parent).copied())
            .unwrap_or(Affine3A::IDENTITY);
        let here = above
            * Affine3A::from_scale_rotation_translation(
                node.scale,
                node.rotation,
                node.translation,
            );
        top = top.max(here.translation.y + 0.5 * node.scale.y.abs());
        at.insert(node.id, here);
    }
    top
}

/// Spawns the name of the plugin `by` over its scene, which goes with
/// the plugin.
fn tag(
    commands: &mut Commands,
    plugin: Entity,
    root: Entity,
    by: &str,
    theme: &Themed,
) {
    let palette = &theme.palette;
    commands.spawn((
        Named(root),
        StagedBy(plugin),
        Node {
            position_type: PositionType::Absolute,
            padding: UiRect::axes(px(5), px(1)),
            border: UiRect::all(px(1)),
            ..Node::default()
        },
        BackgroundColor(palette.black.with_alpha(0.85)),
        BorderColor::all(palette.seam),
        // Its words are the plugin's, so it is under the host's own
        // tags, and over the marks of plugins, as the tag of those
        // who draw is.
        ZIndex(-1),
        Pickable::IGNORE,
        Visibility::Hidden,
        children![(
            Text::new(by),
            TextFont {
                font_size: FontSize::Px(10.0),
                ..TextFont::default()
            },
            TextColor(palette.grey_1),
            Pickable::IGNORE,
        )],
    ));
}

/// Takes away the scene of a plugin that no longer builds one.
fn clear(
    gone: On<Remove, PluginScene>,
    staged: Query<&Staged>,
    mut commands: Commands,
) {
    let Ok(staged) = staged.get(gone.entity) else {
        return;
    };
    for drawn in staged.iter() {
        commands.entity(drawn).try_despawn();
    }
}

/// Keeps each scene at its site, and hides one whose site is not
/// drawn.
fn place(
    whereabouts: Whereabouts,
    mut roots: Query<(&Stage, &mut Transform, &mut Visibility)>,
) {
    for (stage, mut transform, mut visibility) in &mut roots {
        let at = match &stage.site {
            Some(Anchor::Agent(name)) => whereabouts
                .agent(name)
                .map(|(_, at)| at.translation()),
            Some(Anchor::Hamlet(id)) => {
                whereabouts.hamlet(id).map(|(_, at)| at.translation())
            }
            Some(Anchor::Point(at)) => Some(*at),
            None => None,
        };
        match at {
            Some(at) => {
                if transform.translation != at {
                    transform.translation = at;
                }
                visibility.set_if_neq(Visibility::Inherited);
            }
            None => {
                visibility.set_if_neq(Visibility::Hidden);
            }
        }
    }
}

/// Keeps the name of each scene over it where the world's camera sees
/// it, and hides it while the scene is not in sight or the name lies
/// over a tag that asks for the chief.
#[allow(clippy::type_complexity)]
fn name(
    lens: Lens,
    roots: Query<(&Stage, &Transform, &Visibility), Without<Named>>,
    tags: Query<
        (&Node, &ComputedNode, &Visibility),
        (With<NeedTag>, Without<Named>),
    >,
    mut names: Query<(
        &Named,
        &ComputedNode,
        &mut Node,
        &mut Visibility,
    )>,
) {
    let asking = overlay::asking(tags.iter());
    let (camera, view) = lens.camera();
    for (named, computed, mut node, mut visibility) in &mut names {
        let seen =
            roots.get(named.0).ok().and_then(|(stage, at, shown)| {
                let top = at.translation + Vec3::Y * stage.top;
                (*shown != Visibility::Hidden)
                    .then(|| {
                        camera.world_to_viewport(&view, top).ok()
                    })
                    .flatten()
            });
        let Some(point) = seen else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        let size = computed.size() * computed.inverse_scale_factor();
        let corner =
            (point - Vec2::new(size.x * 0.5, size.y + GAP)).round();
        let (left, top) = (px(corner.x), px(corner.y));
        if (node.left, node.top) != (left, top) {
            node.left = left;
            node.top = top;
        }
        let reach = Rect::from_corners(corner, corner + size);
        visibility.set_if_neq(
            match overlay::covers(&asking, reach) {
                true => Visibility::Hidden,
                false => Visibility::Inherited,
            },
        );
    }
}
