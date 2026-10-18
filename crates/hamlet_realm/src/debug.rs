//! The debug overlay: what the pointer can hit and what it is
//! hitting, drawn over the world, with the frame rate. A world draws
//! its own parts with `WorldGizmos` while [`shown`] holds.

use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::RenderLayers;
use bevy::dev_tools::fps_overlay::{
    FpsOverlayConfig, FpsOverlayPlugin, FrameTimeGraphConfig,
};
use bevy::math::bounding::Aabb3d;
use bevy::picking::pointer::PointerInteraction;
use bevy::prelude::*;
use hamlet_assets::tint;

use super::pick::{Hovered, Selected, Thing};
use super::{WorldCamera, WorldSystems};

pub fn plugin(app: &mut App) {
    app.init_resource::<DebugSettings>()
        .init_resource::<Hovered>()
        .init_resource::<Selected>()
        .init_gizmo_group::<WorldGizmos>()
        .add_systems(Startup, spawn_readout)
        .add_systems(
            Update,
            (toggle, apply.run_if(resource_changed::<DebugSettings>))
                .chain()
                .before(WorldSystems),
        )
        .add_systems(
            PostUpdate,
            (hit_boxes, pointer_hits)
                .run_if(shown)
                .after(TransformSystems::Propagate),
        );
    if !app.is_plugin_added::<FpsOverlayPlugin>() {
        app.add_plugins(FpsOverlayPlugin {
            config: FpsOverlayConfig {
                enabled: false,
                frame_time_graph_config: FrameTimeGraphConfig {
                    enabled: false,
                    ..FrameTimeGraphConfig::target_fps(60.0)
                },
                ..default()
            },
        });
    }
}

/// The debug overlay.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DebugSettings {
    pub shown: bool,
    /// The key that shows and hides it.
    pub key: KeyCode,
    /// The box of everything the pointer can hit, and where the
    /// pointer's ray meets them.
    pub hit_boxes: bool,
    /// The line each road follows.
    pub routes: bool,
    /// The room and the door of every plot.
    pub plots: bool,
    /// Each robot's standing place for its chores, and the way it is
    /// walking.
    pub stations: bool,
    /// The frame rate and a graph of frame times.
    pub frame_time: bool,
    /// The render layer its lines are on, which only the world's
    /// camera draws.
    pub layer: usize,
    /// Width of its lines, in pixels.
    pub line_width: f32,
    /// Height of its lines above the ground.
    pub lift: f32,
    /// Radius of a mark at a point.
    pub mark: f32,
    pub text_size: f32,
    #[serde(with = "tint")]
    pub text: Color,
    #[serde(with = "tint")]
    pub hit: Color,
    #[serde(with = "tint")]
    pub hovered: Color,
    #[serde(with = "tint")]
    pub selected: Color,
    #[serde(with = "tint")]
    pub route: Color,
    #[serde(with = "tint")]
    pub plot: Color,
    /// A plot nobody has.
    #[serde(with = "tint")]
    pub unused: Color,
    #[serde(with = "tint")]
    pub station: Color,
    #[serde(with = "tint")]
    pub path: Color,
}

/// The overlay's lines. Only the world's camera draws them, in front
/// of everything.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub(crate) struct WorldGizmos;

/// The line of words under the frame rate.
#[derive(Component)]
struct Readout;

/// True when the overlay is shown.
pub fn shown(set: Res<DebugSettings>) -> bool {
    set.shown
}

fn spawn_readout(mut commands: Commands, set: Res<DebugSettings>) {
    commands.spawn((
        Readout,
        Text::new(""),
        TextFont::from_font_size(set.text_size),
        TextColor(set.text),
        Node {
            position_type: PositionType::Absolute,
            left: px(4),
            top: px(set.text_size * 6.0),
            display: Display::None,
            ..default()
        },
        GlobalZIndex(i32::MAX - 64),
        Pickable::IGNORE,
    ));
}

fn toggle(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut set: ResMut<DebugSettings>,
) {
    if keys.is_some_and(|keys| keys.just_pressed(set.key)) {
        set.shown = !set.shown;
    }
}

/// Shows or hides everything the overlay is made of.
fn apply(
    mut commands: Commands,
    set: Res<DebugSettings>,
    mut store: ResMut<GizmoConfigStore>,
    mut frames: ResMut<FpsOverlayConfig>,
    camera: Query<Entity, With<WorldCamera>>,
    mut readout: Query<&mut Node, With<Readout>>,
) {
    let (config, _) = store.config_mut::<WorldGizmos>();
    config.enabled = set.shown;
    config.line.width = set.line_width;
    // In front of whatever it outlines.
    config.depth_bias = -1.0;
    config.render_layers = RenderLayers::layer(set.layer);
    for camera in &camera {
        commands
            .entity(camera)
            .try_insert(RenderLayers::from_layers(&[0, set.layer]));
    }
    let frame_time = set.shown && set.frame_time;
    if frames.enabled != frame_time {
        frames.enabled = frame_time;
        frames.frame_time_graph_config.enabled = frame_time;
        frames.text_config = TextFont::from_font_size(set.text_size);
        frames.text_color = set.text;
    }
    for mut node in &mut readout {
        node.display = if set.shown {
            Display::DEFAULT
        } else {
            Display::None
        };
    }
}

/// The box of everything the pointer can hit.
fn hit_boxes(
    set: Res<DebugSettings>,
    hovered: Res<Hovered>,
    selected: Res<Selected>,
    things: Query<(&Thing, &GlobalTransform, &Aabb)>,
    mut gizmos: Gizmos<WorldGizmos>,
) {
    if !set.hit_boxes {
        return;
    }
    for (thing, place, bounds) in &things {
        let colour = if selected.0 == Some(*thing) {
            set.selected
        } else if hovered.0 == Some(*thing) {
            set.hovered
        } else {
            set.hit
        };
        gizmos.aabb_3d(
            Aabb3d::new(bounds.center, bounds.half_extents),
            *place,
            colour,
        );
    }
}

/// The point the pointer's ray meets each thing under it at, the
/// nearest marked, and its findings in words.
fn pointer_hits(
    set: Res<DebugSettings>,
    hovered: Res<Hovered>,
    selected: Res<Selected>,
    pointers: Query<&PointerInteraction>,
    things: Query<&Thing>,
    mut readout: Single<&mut Text, With<Readout>>,
    mut gizmos: Gizmos<WorldGizmos>,
) {
    let mut under = Vec::new();
    for pointer in &pointers {
        for (nth, (entity, hit)) in pointer.iter().enumerate() {
            let Some(at) = hit.position else {
                continue;
            };
            let colour = if nth == 0 { set.hovered } else { set.hit };
            gizmos.sphere(at, set.mark, colour);
            if let Some(normal) = hit.normal {
                gizmos.arrow(
                    at,
                    at + normal * set.mark * 4.0,
                    colour,
                );
            }
            if let Ok(thing) = things.get(*entity) {
                under.push(format!("{thing:?} at {:.1}", hit.depth));
            }
        }
    }
    let line = format!(
        "under the pointer: {}\nhovered: {:?}   selected: {:?}",
        if under.is_empty() {
            "nothing that can be hit".into()
        } else {
            under.join(", ")
        },
        hovered.0,
        selected.0,
    );
    if readout.0 != line {
        readout.0 = line;
    }
}
