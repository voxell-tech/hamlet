//! The output of each selected node, drawn over the preview in the
//! colour of its card as the gizmo's fills are: transforms as
//! outlined boxes, whose axes the gizmo shows, assets as their boxes,
//! curves as ribbons through their points with an arrowhead the way
//! they run, points as discs, and a field as a wash on the ground,
//! stronger where it is higher.

use bevy::app::{App, PostUpdate, Startup};
use bevy::asset::Assets;
use bevy::camera::Camera;
use bevy::camera::visibility::Visibility;
use bevy::color::{Alpha, Color, Luminance};
use bevy::ecs::change_detection::Mut;
use bevy::ecs::component::Component;
use bevy::ecs::query::With;
use bevy::ecs::system::{
    Commands, If, Local, Query, Res, ResMut, Single,
};
use bevy::math::{Quat, Vec2, Vec3};
use bevy::mesh::{Mesh, Mesh3d};
use bevy::transform::components::GlobalTransform;
use bevy_fynix::Theme;
use hamlet_blueprint::{
    Curve, Field, Frame, Object, ObjectId, Value,
};
use hamlet_graph::Catalogue;
use hamlet_graph_editor::Editor;
use hamlet_graph_editor::canvas::picked;
use hamlet_ui::theme::HamletTheme;

use super::Blueprint;
use super::gizmo::fills::{FillMaterial, Fills, spawn_shape};
use super::gizmo::{Background, Frame as Seen, LIGHT_DARKEN, Style};
use super::preview::{Framing, Preview};

/// How many squares a field is sampled in, on each side.
const WASH: usize = 24;

/// How far a drawn curve may stray from the curve, in metres.
const CURVE_TOLERANCE: f32 = 0.01;

/// The width of an outline or a ribbon, in logical pixels.
const LINE: f32 = 2.0;

/// The radius of a curve's point, in logical pixels.
const DOT: f32 = 3.0;

/// The radius of a point value, in logical pixels.
const POINT: f32 = 6.0;

/// The length and the width of an arrowhead, in logical pixels.
const ARROW: Vec2 = Vec2::new(12.0, 10.0);

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn)
        .add_systems(PostUpdate, draw);
}

/// Marks the mesh of the overlay's shapes.
#[derive(Component)]
struct OverlayFills;

fn spawn(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FillMaterial>>,
) {
    spawn_shape(
        &mut commands,
        &mut meshes,
        &mut materials,
        OverlayFills,
    );
}

#[expect(
    clippy::too_many_arguments,
    reason = "it reads the editor, the view and the mesh it fills"
)]
fn draw(
    editor: Res<Editor<Blueprint>>,
    framing: Res<Framing>,
    preview: Option<Res<Preview>>,
    If(theme): If<Res<Theme<HamletTheme>>>,
    background: Res<Background>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    filled: Single<(&Mesh3d, &mut Visibility), With<OverlayFills>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut drawn: Local<Fills>,
) {
    let palette = &theme.0.palette;
    let style = Style::of(palette).against(palette, *background);
    let mut fills = Fills::default();
    if let Some((camera, eye)) =
        preview.and_then(|preview| cameras.get(preview.camera).ok())
    {
        let mut sketch = Sketch {
            fills: &mut fills,
            camera,
            eye,
            assets: &editor.rules.assets,
            framing: &framing,
            wash: style.plane_fill,
        };
        for (at, node) in editor.selected.iter().enumerate() {
            let colour =
                tone(picked(palette, at), *background, &style);
            for (_, value) in
                editor.got.watched.get(node).into_iter().flatten()
            {
                sketch.show(value, colour);
            }
        }
    }
    let (mesh, visibility): (&Mesh3d, Mut<Visibility>) =
        filled.into_inner();
    fills.show(&mut drawn, mesh, visibility, &mut meshes);
}

/// `colour` as drawn on `background`: darker on a light one, at the
/// style's rest opacity.
fn tone(
    colour: Color,
    background: Background,
    style: &Style,
) -> Color {
    let colour = match background {
        Background::Dark => colour,
        Background::Light => colour.darker(LIGHT_DARKEN),
    };
    colour.with_alpha(colour.alpha() * style.rest_alpha)
}

/// The points a curve is drawn through, its first again at the end
/// when it is closed.
fn curve_points(curve: &Curve) -> Vec<Vec3> {
    curve.sampled(CURVE_TOLERANCE)
}

/// The shapes of the overlay put into `fills`, as `camera` sees them.
struct Sketch<'a> {
    fills: &'a mut Fills,
    camera: &'a Camera,
    eye: &'a GlobalTransform,
    assets: &'a Catalogue,
    framing: &'a Framing,
    /// How much of its opacity a flat fill keeps.
    wash: f32,
}

impl Sketch<'_> {
    fn show(&mut self, value: &Value, colour: Color) {
        match value {
            Value::Transform(transform) => {
                // Read at the origin, as an object put on it alone
                // stands.
                let frame = transform.at(&Object::new(
                    String::new(),
                    ObjectId::ROOT,
                    Frame::IDENTITY,
                ));
                self.outline(&frame, frame.scale, colour);
            }
            Value::Objects(objects) => {
                for object in objects {
                    let own = self
                        .assets
                        .get(&object.asset)
                        .map_or(Vec3::ONE, |asset| asset.size);
                    self.outline(
                        &object.frame,
                        object.frame.scale * own,
                        colour,
                    );
                }
            }
            Value::Curve(curve) => self.curve(curve, colour),
            Value::Point(point) => self.dot(*point, POINT, colour),
            Value::Field(field) => self.field(field, colour),
            Value::Number(_)
            | Value::PointField(_)
            | Value::Integer(_)
            | Value::Bool(_)
            | Value::Choice(_)
            | Value::Path(_)
            | Value::Color(_) => {}
        }
    }

    /// `at` as the camera sees it.
    fn see(&self, at: Vec3) -> Option<Seen> {
        Seen::of(at, Quat::IDENTITY, self.camera, self.eye)
    }

    /// A ribbon of constant width on screen through `path`.
    fn stroke(&mut self, path: &[Seen], colour: Color) {
        let last = path.len().saturating_sub(1);
        let spine = path.iter().enumerate().map(|(at, here)| {
            let along = path[(at + 1).min(last)].origin
                - path[at.saturating_sub(1)].origin;
            let across = along.cross(here.look).normalize_or_zero();
            (here.origin, across * LINE * 0.5 * here.scale)
        });
        self.fills.ribbon(spine, colour);
    }

    /// A disc of `radius` logical pixels facing the camera.
    fn dot(&mut self, at: Vec3, radius: f32, colour: Color) {
        if let Some(seen) = self.see(at) {
            self.fills.disc(
                at,
                -seen.look,
                radius * seen.scale,
                colour,
            );
        }
    }

    /// The edges of the box of `size` an asset standing on `frame`
    /// fills.
    fn outline(&mut self, frame: &Frame, size: Vec3, colour: Color) {
        let middle = frame.translation
            + frame.rotation * Vec3::new(0.0, size.y * 0.5, 0.0);
        let halves = [Vec3::X, Vec3::Y, Vec3::Z]
            .map(|axis| frame.rotation * axis * size * 0.5);
        let corners = std::array::from_fn::<_, 8, _>(|corner| {
            let at = (0..3).fold(middle, |at, axis| {
                let sign =
                    if corner >> axis & 1 == 0 { -1.0 } else { 1.0 };
                at + halves[axis] * sign
            });
            self.see(at)
        });
        for corner in 0..8 {
            for axis in 0..3 {
                if corner >> axis & 1 == 0
                    && let (Some(from), Some(to)) =
                        (corners[corner], corners[corner | 1 << axis])
                {
                    self.stroke(&[from, to], colour);
                }
            }
        }
    }

    /// A ribbon through the curve's points, a dot at each and an
    /// arrowhead on its first edge for the way it runs.
    fn curve(&mut self, curve: &Curve, colour: Color) {
        let path = curve_points(curve)
            .iter()
            .filter_map(|point| self.see(*point))
            .collect::<Vec<_>>();
        self.stroke(&path, colour);
        for point in curve.points() {
            self.dot(point, DOT, colour);
        }
        if let Some((middle, along)) = curve
            .segments()
            .first()
            .map(|first| first.at(first.length() * 0.5))
            && let Some(seen) = self.see(middle)
        {
            let side = along.cross(seen.look).normalize_or_zero()
                * ARROW.y
                * 0.5
                * seen.scale;
            let base = seen.origin - along * ARROW.x * seen.scale;
            self.fills.fan(
                seen.origin,
                [base + side, base - side],
                colour,
            );
        }
    }

    /// Squares on the ground about what is drawn, from the field's
    /// lowest there to its highest, stronger where it is higher.
    fn field(&mut self, field: &Field, colour: Color) {
        let reach = self.framing.reach.max(2.0);
        let cell = reach * 2.0 / WASH as f32;
        let at = |i: usize, j: usize| {
            self.framing.middle * Vec3::new(1.0, 0.0, 1.0)
                + Vec3::new(
                    (i as f32 + 0.5) * cell - reach,
                    0.02,
                    (j as f32 + 0.5) * cell - reach,
                )
        };
        let samples = (0..WASH)
            .flat_map(|i| (0..WASH).map(move |j| (i, j)))
            .map(|(i, j)| {
                let point = at(i, j);
                let at = Object::new(
                    String::new(),
                    ObjectId::ROOT,
                    Frame::at(point),
                );
                (point, field.sample(&at))
            })
            .collect::<Vec<_>>();
        let (low, high) = samples.iter().fold(
            (f32::MAX, f32::MIN),
            |(low, high), (_, value)| {
                (low.min(*value), high.max(*value))
            },
        );
        let span = (high - low).max(1e-6);
        let half = cell * 0.4;
        for (point, value) in samples {
            let strength = 0.1 + 0.9 * (value - low) / span;
            let [a, b, c, d] =
                [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                    .map(|(x, z)| {
                        point + Vec3::new(x, 0.0, z) * half
                    });
            self.fills.fan(
                a,
                [b, c, d],
                colour.with_alpha(
                    colour.alpha() * self.wash * strength,
                ),
            );
        }
    }
}
