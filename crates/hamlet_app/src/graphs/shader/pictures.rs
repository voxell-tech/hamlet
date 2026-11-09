//! Each shader's picture, wherever the library shows one: the shader
//! on a sphere, its surface's colour shaded from the top left, as a
//! swatch shows a node. Each shader runs a program of its own, which
//! its pictures' material picks, written again when its graph
//! changes.

use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::time::Duration;

use bevy::app::{App, Update};
use bevy::asset::uuid::Uuid;
use bevy::asset::{Asset, Assets, Handle};
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::query::Added;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
use bevy::math::UVec4;
use bevy::picking::Pickable;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor,
};
use bevy::shader::{Shader, ShaderRef};
use bevy::time::Time;
use bevy::ui::widget::ImageNode;
use bevy::ui::{Node, percent};
use bevy::ui_render::UiMaterialPlugin;
use bevy::ui_render::prelude::MaterialNode;
use bevy::ui_render::ui_material::{UiMaterial, UiMaterialKey};
use hamlet_graph::{Does, Graph};
use hamlet_graph_editor::library::picker::ShaderPicture;
use hamlet_graph_editor::library::{Kind, Library, lone};
use hamlet_shader::{Compiled, compile, swatch_shader};
use hamlet_ui::widgets::icons::Glyph;

use super::swatch::Slots;

/// How often the shaders pictured are read again, for one changed.
const LOOK: Duration = Duration::from_secs(2);

/// The kind of node whose colour a picture shows.
const SURFACE: &str = "surface";

pub(super) fn plugin(app: &mut App) {
    app.add_plugins(UiMaterialPlugin::<PictureMaterial>::default())
        .init_resource::<Pictured>()
        .add_systems(Update, (fill, refresh));
}

/// The program each shader's pictures run, by the shader's name: its
/// graph as last compiled, none for one that does not compile.
#[derive(Resource, Default)]
struct Pictured(BTreeMap<String, Option<Graph>>);

/// The material of a shader's picture: the shader's values, the node
/// it shows, and which program it runs.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
#[bind_group_data(Program)]
struct PictureMaterial {
    #[uniform(0)]
    slots: Slots,
    /// The node shown, in x.
    #[uniform(1)]
    shown: UVec4,
    program: Program,
}

/// The program a picture runs, by its shader's name.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct Program(u128);

impl Program {
    fn of(name: &str) -> Self {
        // FNV-1a over the name, apart from every other program.
        let mut hash: u128 =
            0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
        for byte in
            b"hamlet shader picture ".iter().chain(name.as_bytes())
        {
            hash ^= u128::from(*byte);
            hash = hash.wrapping_mul(
                0x0000_0000_0100_0000_0000_0000_0000_013b,
            );
        }
        Self(hash)
    }

    fn handle(self) -> Handle<Shader> {
        Handle::Uuid(Uuid::from_u128(self.0), PhantomData)
    }
}

impl From<&PictureMaterial> for Program {
    fn from(material: &PictureMaterial) -> Self {
        material.program
    }
}

impl UiMaterial for PictureMaterial {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Default
    }

    fn specialize(
        descriptor: &mut RenderPipelineDescriptor,
        key: UiMaterialKey<Self>,
    ) {
        if let Some(fragment) = descriptor.fragment.as_mut() {
            fragment.shader = key.bind_group_data.handle();
        }
    }
}

/// The shader `name`'s graph, as the library has it.
fn graph_of(library: &Library, name: &str) -> Option<Graph> {
    let item = library.find(Kind::Shader, name)?;
    Some(lone(item.read().ok()?).graph)
}

/// The node of `graph` a picture shows: its surface.
fn surface(graph: &Graph) -> Option<u32> {
    graph
        .nodes
        .iter()
        .find(|node| matches!(&node.does, Does::Kind(kind) if kind == SURFACE))
        .map(|node| Compiled::which(node.id))
}

/// Writes the program of the shader `name` from `graph`, and gives
/// its values and the node it shows, none where it does not compile
/// or has no surface.
fn write(
    shaders: &mut Assets<Shader>,
    name: &str,
    graph: &Graph,
) -> Option<(Slots, UVec4)> {
    let compiled = compile(graph).ok()?;
    let shown = surface(graph)?;
    let program = Program::of(name);
    let _ = shaders.insert(
        program.handle().id(),
        Shader::from_wesl(
            swatch_shader(&compiled),
            format!("hamlet_shader/picture_{:032x}.wesl", program.0),
        ),
    );
    Some((
        Slots::of(&compiled.values(graph, 0.0)),
        UVec4::new(shown, 0, 0, 0),
    ))
}

/// Puts the shader on its sphere in each picture as it is built, or
/// the shader glyph where it does not compile.
fn fill(
    mut commands: Commands,
    plates: Query<(Entity, &ShaderPicture), Added<ShaderPicture>>,
    library: Res<Library>,
    mut pictured: ResMut<Pictured>,
    mut shaders: ResMut<Assets<Shader>>,
    mut materials: ResMut<Assets<PictureMaterial>>,
) {
    for (plate, ShaderPicture(name)) in &plates {
        let graph = graph_of(&library, name);
        let made = graph
            .as_ref()
            .and_then(|graph| write(&mut shaders, name, graph));
        pictured.0.insert(name.clone(), graph);
        let fill = Node {
            width: percent(100.0),
            height: percent(100.0),
            ..Node::default()
        };
        match made {
            Some((slots, shown)) => {
                let material = materials.add(PictureMaterial {
                    slots,
                    shown,
                    program: Program::of(name),
                });
                commands.spawn((
                    MaterialNode(material),
                    fill,
                    Pickable::IGNORE,
                    ChildOf(plate),
                ));
            }
            None => {
                commands.spawn((
                    ImageNode::new(Glyph::Sphere.image(64.0)),
                    fill,
                    Pickable::IGNORE,
                    ChildOf(plate),
                ));
            }
        }
    }
}

/// Writes again the program and values of each shader pictured whose
/// graph changed.
fn refresh(
    time: Res<Time>,
    library: Res<Library>,
    mut pictured: ResMut<Pictured>,
    mut shaders: ResMut<Assets<Shader>>,
    mut materials: ResMut<Assets<PictureMaterial>>,
    mut looked: Local<Duration>,
) {
    let now = time.elapsed();
    if now - *looked < LOOK {
        return;
    }
    *looked = now;
    let names = pictured.0.keys().cloned().collect::<Vec<_>>();
    for name in names {
        let graph = graph_of(&library, &name);
        if pictured.0.get(&name) == Some(&graph) {
            continue;
        }
        let made = graph
            .as_ref()
            .and_then(|graph| write(&mut shaders, &name, graph));
        pictured.0.insert(name.clone(), graph);
        let Some((slots, shown)) = made else {
            continue;
        };
        let program = Program::of(&name);
        for (_, material) in materials.iter_mut() {
            if material.program == program {
                material.slots = slots.clone();
                material.shown = shown;
            }
        }
    }
}
