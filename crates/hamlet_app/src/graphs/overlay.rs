//! What each selected node gives, drawn over the preview in the
//! colour its card is lit in: placements as outlined boxes with their
//! axes, assets as their boxes, curves as lines through their points
//! with an arrow the way they run, points as dots, and a field as a
//! wash on the ground, brighter where it is higher.

use bevy::app::{App, Startup, Update};
use bevy::camera::visibility::RenderLayers;
use bevy::color::{Alpha, Color};
use bevy::ecs::system::{If, Res, ResMut};
use bevy::gizmos::AppGizmoBuilder;
use bevy::gizmos::config::{GizmoConfigGroup, GizmoConfigStore};
use bevy::gizmos::gizmos::Gizmos;
use bevy::math::{Isometry3d, Quat, Vec2, Vec3};
use bevy::reflect::Reflect;
use bevy::transform::components::Transform;
use bevy_fynix::Theme;
use hamlet_blueprint::{Frame, Placement, PlacementId, Value};
use hamlet_ui::theme::HamletTheme;

use super::Editor;
use super::canvas::picked;
use super::preview::{Framing, LAYER};

/// How many squares a field is sampled in, on each side.
const WASH: usize = 24;

pub(super) fn plugin(app: &mut App) {
    app.init_gizmo_group::<Overlay>()
        .add_systems(Startup, setup)
        .add_systems(Update, draw);
}

/// The lines drawn over the preview, seen only by its camera.
#[derive(Default, Reflect, GizmoConfigGroup)]
struct Overlay;

fn setup(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<Overlay>();
    config.render_layers = RenderLayers::layer(LAYER);
    config.line.width = 2.0;
    // In front of the assets it outlines.
    config.depth_bias = -1.0;
}

fn draw(
    mut gizmos: Gizmos<Overlay>,
    editor: Res<Editor>,
    framing: Res<Framing>,
    If(theme): If<Res<Theme<HamletTheme>>>,
) {
    for (at, node) in editor.selected.iter().enumerate() {
        let colour = picked(&theme.0.palette, at);
        for (_, value) in
            editor.watched.get(node).into_iter().flatten()
        {
            show(&mut gizmos, value, colour, &framing);
        }
    }
}

fn show(
    gizmos: &mut Gizmos<Overlay>,
    value: &Value,
    colour: Color,
    framing: &Framing,
) {
    match value {
        Value::Placements(placements) => {
            for placement in placements {
                outline(
                    gizmos,
                    &placement.frame,
                    placement.size,
                    colour,
                );
                gizmos.axes(
                    Transform::from_translation(
                        placement.frame.translation,
                    )
                    .with_rotation(placement.frame.rotation),
                    0.35,
                );
            }
        }
        Value::Geometry(instances) => {
            for instance in instances {
                outline(
                    gizmos,
                    &instance.frame,
                    instance.size,
                    colour,
                );
            }
        }
        Value::Curve(curve) => {
            let mut points = curve.points.clone();
            if curve.closed
                && let Some(first) = points.first().copied()
            {
                points.push(first);
            }
            gizmos.linestrip(points.iter().copied(), colour);
            for point in &curve.points {
                gizmos.sphere(
                    Isometry3d::from_translation(*point),
                    0.06,
                    colour,
                );
            }
            if let [first, second, ..] = curve.points[..] {
                gizmos.arrow(first, first.lerp(second, 0.5), colour);
            }
        }
        Value::Point(point) => {
            gizmos.sphere(
                Isometry3d::from_translation(*point),
                0.12,
                colour,
            );
        }
        Value::Field(field) => {
            // Sampled on the ground about what is drawn, from its
            // lowest to its highest there.
            let reach = framing.reach.max(2.0);
            let cell = reach * 2.0 / WASH as f32;
            let at = |i: usize, j: usize| {
                framing.middle * Vec3::new(1.0, 0.0, 1.0)
                    + Vec3::new(
                        (i as f32 + 0.5) * cell - reach,
                        0.02,
                        (j as f32 + 0.5) * cell - reach,
                    )
            };
            let samples: Vec<(Vec3, f32)> = (0..WASH)
                .flat_map(|i| (0..WASH).map(move |j| (i, j)))
                .map(|(i, j)| {
                    let point = at(i, j);
                    let placement = Placement::new(
                        PlacementId::ROOT,
                        Frame::at(point),
                    );
                    (point, field.sample(&placement))
                })
                .collect();
            let (low, high) = samples.iter().fold(
                (f32::MAX, f32::MIN),
                |(low, high), (_, value)| {
                    (low.min(*value), high.max(*value))
                },
            );
            let span = (high - low).max(1e-6);
            let flat =
                Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
            for (point, value) in samples {
                let bright = (value - low) / span;
                gizmos.rect(
                    Isometry3d::new(point, flat),
                    Vec2::splat(cell * 0.8),
                    colour.with_alpha(0.1 + 0.9 * bright),
                );
            }
        }
        Value::Number(_) | Value::Bool(_) | Value::Choice(_) => {}
    }
}

/// The box an asset of `size` standing on `frame` fills.
fn outline(
    gizmos: &mut Gizmos<Overlay>,
    frame: &Frame,
    size: Vec3,
    colour: Color,
) {
    let size = size * frame.scale;
    gizmos.cube(
        Transform {
            translation: frame.translation
                + frame.rotation * Vec3::new(0.0, size.y * 0.5, 0.0),
            rotation: frame.rotation,
            scale: size,
        },
        colour,
    );
}
