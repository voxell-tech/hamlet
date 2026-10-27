//! Wires drawn as curves: one node over each, whose material
//! (`wire.wesl`) draws a cubic Bezier from one port to the other.

use bevy::app::App;
use bevy::asset::{Asset, embedded_asset};
use bevy::color::{Color, ColorToComponents};
use bevy::math::{Vec2, Vec4};
use bevy::reflect::TypePath;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::ui_render::UiMaterialPlugin;
use bevy::ui_render::ui_material::UiMaterial;

/// Half the line's thickness, in screen pixels.
const HALF_THICK: f32 = 1.1;

/// Room left around the curve so its soft edge is not cut.
const ROOM: f32 = 6.0;

pub(super) fn plugin(app: &mut App) {
    embedded_asset!(app, "wire.wesl");
    app.add_plugins(UiMaterialPlugin::<WireMaterial>::default());
}

/// The material of a wire.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub(super) struct WireMaterial {
    #[uniform(0)]
    wire: WireUniform,
}

/// Mirrors `Wire` in `wire.wesl`; the fields are explained there.
#[derive(ShaderType, Debug, Clone, Copy, Default, PartialEq)]
struct WireUniform {
    colour: Vec4,
    ends: Vec4,
    handles: Vec4,
    size: Vec4,
}

impl UiMaterial for WireMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://hamlet_ui/graphs/wire.wesl".into()
    }
}

/// Where a wire from `a` to `b` is drawn: the corner and size of the
/// node over it, and its material, all in the sheet's units.
pub(super) struct Shape {
    pub corner: Vec2,
    pub size: Vec2,
    pub material: WireMaterial,
}

impl Shape {
    /// The wire from `a` to `b` in `colour`. It leaves and arrives
    /// level, its handles reaching half the way across, and no less
    /// than 40.
    pub fn of(a: Vec2, b: Vec2, colour: Color) -> Self {
        let reach = ((b.x - a.x).abs() * 0.5).max(40.0);
        let (p1, p2) =
            (a + Vec2::new(reach, 0.0), b - Vec2::new(reach, 0.0));
        // The curve lies within its four points.
        let low = a.min(b).min(p1).min(p2) - Vec2::splat(ROOM);
        let high = a.max(b).max(p1).max(p2) + Vec2::splat(ROOM);
        let size = high - low;
        // Two points in the node, as one vector.
        let pair = |p: Vec2, q: Vec2| {
            let (p, q) = (p - low, q - low);
            Vec4::new(p.x, p.y, q.x, q.y)
        };
        let wire = WireUniform {
            colour: Vec4::from_array(
                colour.to_linear().to_f32_array(),
            ),
            ends: pair(a, b),
            handles: pair(p1, p2),
            size: Vec4::new(size.x, size.y, HALF_THICK, 0.0),
        };
        Self {
            corner: low,
            size,
            material: WireMaterial { wire },
        }
    }
}

impl WireMaterial {
    /// Whether it draws the same as `other`.
    pub fn same_as(&self, other: &WireMaterial) -> bool {
        self.wire == other.wire
    }
}
