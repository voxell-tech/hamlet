//! Each card's swatch: what its node gives, on a small sphere shaded
//! from the top left. Every swatch runs one shader, compiled from the
//! whole graph, told which node to show by a uniform.

use bevy::app::{App, Update};
use bevy::asset::{Asset, Assets, Handle, uuid_handle};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::query::Added;
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
use bevy::math::{UVec4, Vec4};
use bevy::picking::Pickable;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::{Shader, ShaderRef};
use bevy::time::Time;
use bevy::ui::{AlignSelf, Node, Val, px};
use bevy::ui_render::UiMaterialPlugin;
use bevy::ui_render::prelude::MaterialNode;
use bevy::ui_render::ui_material::UiMaterial;
use hamlet_graph_editor::Editor;
use hamlet_graph_editor::canvas::Room;
use hamlet_material::{SLOTS, swatch_shader};

use super::{Got, Material};

/// A swatch's width and height, in sheet pixels.
pub(super) const SIZE: f32 = 64.0;

/// The shader every swatch runs, written again when the graph's code
/// changes.
const SHADER: Handle<Shader> =
    uuid_handle!("6b0c5f0e-3f43-4a52-9a43-2c1d6f1b7a11");

pub(super) fn plugin(app: &mut App) {
    app.add_plugins(UiMaterialPlugin::<SwatchMaterial>::default())
        .add_systems(Update, (fill, shade, feed));
}

/// Every slot of the graph, as the shader's `Slots`.
#[derive(ShaderType, Debug, Clone)]
pub(super) struct Slots {
    values: [Vec4; SLOTS],
}

impl Default for Slots {
    fn default() -> Self {
        Self {
            values: [Vec4::ZERO; SLOTS],
        }
    }
}

impl Slots {
    pub(super) fn of(values: &[Vec4]) -> Self {
        let mut slots = Self::default();
        for (slot, value) in slots.values.iter_mut().zip(values) {
            *slot = *value;
        }
        slots
    }
}

/// The material of one swatch.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub(super) struct SwatchMaterial {
    #[uniform(0)]
    slots: Slots,
    /// The node shown, in x.
    #[uniform(1)]
    shown: UVec4,
}

impl UiMaterial for SwatchMaterial {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(SHADER)
    }
}

/// Puts a swatch in each card's room as the card is built.
fn fill(
    mut commands: Commands,
    rooms: Query<(Entity, &Room<Material>), Added<Room<Material>>>,
    mut materials: ResMut<Assets<SwatchMaterial>>,
) {
    for (room, Room(node, ..)) in &rooms {
        let material = materials.add(SwatchMaterial {
            slots: Slots::default(),
            shown: UVec4::new(node.0, 0, 0, 0),
        });
        commands.spawn((
            MaterialNode(material),
            Node {
                width: px(SIZE),
                height: px(SIZE),
                margin: bevy::ui::UiRect::all(Val::Auto),
                align_self: AlignSelf::Center,
                ..Node::default()
            },
            Pickable::IGNORE,
            ChildOf(room),
        ));
    }
}

/// Writes the swatches' shader again when the graph's code changes.
fn shade(
    got: Res<Got>,
    mut shaders: ResMut<Assets<Shader>>,
    mut shaded: Local<Option<u64>>,
) {
    if *shaded == Some(got.version) {
        return;
    }
    let Some(compiled) = got.compiled() else {
        return;
    };
    *shaded = Some(got.version);
    let _ = shaders.insert(
        SHADER.id(),
        Shader::from_wesl(
            swatch_shader(compiled),
            "hamlet_material/swatch.wesl",
        ),
    );
}

/// Fills every swatch's slots with the graph's values, again each
/// frame while the graph reads the time.
fn feed(
    editor: Res<Editor<Material>>,
    got: Res<Got>,
    time: Res<Time>,
    mut materials: ResMut<Assets<SwatchMaterial>>,
    added: Query<(), Added<MaterialNode<SwatchMaterial>>>,
) {
    let Some(compiled) = got.compiled() else {
        return;
    };
    if !compiled.reads_time()
        && !editor.is_changed()
        && !got.is_changed()
        && added.is_empty()
    {
        return;
    }
    let slots = Slots::of(
        &compiled.values(&editor.sheet.graph, time.elapsed_secs()),
    );
    for (_, material) in materials.iter_mut() {
        material.slots = slots.clone();
    }
}
