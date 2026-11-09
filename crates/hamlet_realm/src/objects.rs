//! What a blueprint puts down, drawn: each object's asset as the
//! catalogue has it, a shape of Hamlet's own or a mesh read from its
//! file, in the colours of the palette and of the house it is for.
//! The realm draws houses this way, and the blueprint editor its
//! preview.

mod model;
mod parts;

use bevy::prelude::*;
use hamlet_blueprint::Frame;
use hamlet_graph::{Asset, BODY, Paint, Shape};

pub use self::model::{Model, Models, read};
pub(crate) use self::parts::bush;
use crate::meshes::{Meshing, Rng};
use crate::palette::{Palette, TownStyle};

/// The palette's colours a material slot can take, of those a
/// building is made of, with those each house picks for itself.
pub const COLOURS: [&str; 26] = [
    "plaster",
    "plaster_worn",
    "whitewash",
    "ochre",
    "limestone",
    "limestone_shade",
    "rock",
    "rock_dark",
    "timber",
    "timber_dark",
    "log",
    "bark",
    "bed",
    "terracotta",
    "tile_drift",
    "slate_roof",
    "moss",
    "foliage",
    "foliage_shade",
    "brass",
    "pane",
    "wall",
    "upper_wall",
    "roof",
    "drift",
    "livery",
];

/// The colours the assets of one house take: the palette's, and the
/// ones the house picks for itself.
#[derive(Clone, Copy)]
pub struct Paints<'a> {
    pub palette: &'a Palette,
    /// Its town's roofs and their drift.
    pub style: TownStyle,
    /// The door's and the shutters' colour.
    pub livery: Vec3,
    /// The walls of its ground floor.
    pub wall: Vec3,
    /// The walls above, and the gables.
    pub upper_wall: Vec3,
}

impl<'a> Paints<'a> {
    /// The colours of a house of the first town, plastered, for a
    /// house drawn outside the realm.
    pub fn of(palette: &'a Palette) -> Self {
        Self {
            palette,
            style: palette.towns[0],
            livery: palette.livery[0],
            wall: palette.plaster,
            upper_wall: palette.plaster,
        }
    }

    /// The colour named `name`, of [`COLOURS`].
    pub fn colour(&self, name: &str) -> Option<Vec3> {
        let palette = self.palette;
        Some(match name {
            "plaster" => palette.plaster,
            "plaster_worn" => palette.plaster_worn,
            "whitewash" => palette.whitewash,
            "ochre" => palette.ochre,
            "limestone" => palette.limestone,
            "limestone_shade" => palette.limestone_shade,
            "rock" => palette.rock,
            "rock_dark" => palette.rock_dark,
            "timber" => palette.timber,
            "timber_dark" => palette.timber_dark,
            "log" => palette.log,
            "bark" => palette.bark,
            "bed" => palette.bed,
            "terracotta" => palette.terracotta,
            "tile_drift" => palette.tile_drift,
            "slate_roof" => palette.slate_roof,
            "moss" => palette.moss,
            "foliage" => palette.foliage,
            "foliage_shade" => palette.foliage_shade,
            "brass" => palette.brass,
            "pane" => {
                let pane = palette.pane_lit.to_linear();
                Vec3::new(pane.red, pane.green, pane.blue)
            }
            "wall" => self.wall,
            "upper_wall" => self.upper_wall,
            "roof" => self.style.roof,
            "drift" => self.style.drift,
            "livery" => self.livery,
            _ => return None,
        })
    }
}

/// Where objects are drawn: the body, and the panes apart when they
/// light at night on their own. Without them, panes are drawn into
/// the body in their lit colour.
pub struct Target<'a> {
    pub body: &'a mut Meshing,
    pub panes: Option<&'a mut Meshing>,
}

/// The colour of an asset's material slot, as its catalogue entry
/// paints it: a palette colour, or `own`, the one its file gives.
/// None for a pane drawn apart from the body.
fn paint(
    paints: Option<&Paints>,
    asset: Option<&Asset>,
    slot: &str,
    own: Vec3,
    apart: bool,
) -> Option<Vec3> {
    let Some(paints) = paints else {
        return Some(Vec3::splat(0.7));
    };
    let named = |name: &str, shade: f32| {
        if apart && name == "pane" {
            return None;
        }
        Some(
            paints.colour(name).unwrap_or(paints.palette.plaster)
                * shade,
        )
    };
    match asset.and_then(|asset| asset.slots.get(slot)) {
        Some(Paint::Palette(name)) => named(name, 1.0),
        Some(Paint::Shade(name, shade)) => named(name, *shade),
        Some(Paint::Own) => Some(own),
        None => Some(paints.palette.plaster),
    }
}

/// The size an asset is drawn at on `frame`: its own size times the
/// frame's scale, a metre on each side where it has none.
pub fn drawn_size(frame: &Frame, asset: Option<&Asset>) -> Vec3 {
    frame.scale * asset.map_or(Vec3::ONE, |asset| asset.size)
}

/// The asset `name`, with the catalogue entry `asset` and the
/// `model` its file was read as, drawn standing on `frame`: a box
/// where it has no shape of its own, and a small one for a mark.
/// `seed` gives tiles their colours. Gives the middle of its box.
pub fn stand(
    target: &mut Target,
    frame: &Frame,
    name: &str,
    asset: Option<&Asset>,
    model: Option<&Model>,
    paints: Option<&Paints>,
    seed: u64,
) -> Vec3 {
    let size = drawn_size(frame, asset);
    let apart = target.panes.is_some();
    let colour = |slot: &str, own: Vec3| {
        paint(paints, asset, slot, own, apart)
    };
    let solid = colour(BODY, Vec3::splat(0.7)).unwrap_or(Vec3::ONE);
    let shape = asset.map(|asset| &asset.shape);
    if shape == Some(&Shape::Builtin)
        && parts::draw(
            name,
            target.body,
            target.panes.as_deref_mut(),
            frame,
            &|slot| colour(slot, Vec3::splat(0.7)),
        )
    {
        return frame.translation;
    }
    let middle = frame.translation
        + frame.rotation * Vec3::new(0.0, size.y * 0.5, 0.0);
    match (model, shape, paints) {
        (Some(model), ..) => model.draw(
            target.body,
            target.panes.as_deref_mut(),
            middle,
            size,
            frame.rotation,
            colour,
        ),
        (None, Some(Shape::Mark), _) => {
            let mark = paints.map_or(Vec3::splat(0.7), |paints| {
                paints.palette.ochre
            });
            target.body.rbox(
                frame.translation,
                Vec3::splat(0.05),
                0.02,
                frame.rotation,
                mark,
                0.0,
            );
            return frame.translation;
        }
        (None, Some(Shape::Wedge { peak }), _) => {
            wedge(target.body, frame, size, *peak, solid);
        }
        (None, Some(Shape::Tiles { taper }), Some(paints)) => {
            let mut rng = Rng::seeded(&format!(
                "{seed} {:?}",
                frame.translation
            ));
            tiles(
                target.body,
                &mut rng,
                frame,
                size,
                *taper,
                solid,
                paints,
            );
        }
        (None, Some(Shape::Rounded { bevel, ao }), _) => {
            target.body.rbox(
                middle,
                size * 0.5,
                *bevel,
                frame.rotation,
                solid,
                *ao,
            );
        }
        _ => target.body.rbox(
            middle,
            size * 0.5,
            0.04,
            frame.rotation,
            solid,
            0.25,
        ),
    }
    middle
}

/// A triangle across x on `frame`, `size.y` high at `peak` along x
/// and `size.z` thick: a gable's end.
fn wedge(
    meshing: &mut Meshing,
    frame: &Frame,
    size: Vec3,
    peak: f32,
    colour: Vec3,
) {
    let at = |x: f32, y: f32, z: f32| {
        frame.translation
            + frame.rotation * (Vec3::new(x, y, z) * size)
    };
    let turn = |v: Vec3| frame.rotation * v;
    let (front, back) = (0.5, -0.5);
    let corner = |x: f32, z: f32| at(x, 0.0, z);
    let top = |z: f32| at(peak, 1.0, z);
    meshing.face(
        [
            corner(-0.5, front),
            corner(0.5, front),
            top(front),
            top(front),
        ],
        turn(Vec3::Z),
        colour,
    );
    meshing.face(
        [corner(0.5, back), corner(-0.5, back), top(back), top(back)],
        turn(-Vec3::Z),
        colour,
    );
    // Each sloping side faces out and up, steeper the nearer the
    // peak.
    let sides = [
        (-0.5, Vec3::new(-size.y, (peak + 0.5) * size.x, 0.0)),
        (0.5, Vec3::new(size.y, (0.5 - peak) * size.x, 0.0)),
    ];
    for (x, toward) in sides {
        meshing.face(
            [
                corner(x, back),
                corner(x, front),
                top(front),
                top(back),
            ],
            turn(toward),
            colour * 0.95,
        );
    }
}

/// Lapped tiles over `frame` as a roof's slope is laid, from the eave
/// at +z to the ridge at -z, `taper` times as wide. A slope in the
/// town's own roof colour drifts as the town's roofs do.
fn tiles(
    meshing: &mut Meshing,
    rng: &mut Rng,
    frame: &Frame,
    size: Vec3,
    taper: f32,
    colour: Vec3,
    paints: &Paints,
) {
    // A slope flat along x or z has no eave or no run to lay on, and
    // `slope` would give its vertices NaN.
    if size.x.abs() < 1e-4 || size.z.abs() < 1e-4 {
        return;
    }
    let at = |x: f32, z: f32| {
        frame.translation
            + frame.rotation
                * (Vec3::new(x * size.x, 0.0, z * size.z))
    };
    let eave = [at(-0.5, 0.5), at(0.5, 0.5)];
    let ridge = [at(-0.5 * taper, -0.5), at(0.5 * taper, -0.5)];
    let style = TownStyle {
        roof: colour,
        drift: if colour == paints.style.roof {
            paints.style.drift
        } else {
            colour * 1.15
        },
        banner: paints.style.banner,
    };
    slope(meshing, rng, eave, ridge, &style, paints.palette);
}

/// One slope of tiles from an eave to a ridge, laid in lapped
/// courses with each tile its own colour.
pub fn slope(
    meshing: &mut Meshing,
    rng: &mut Rng,
    eave: [Vec3; 2],
    ridge: [Vec3; 2],
    style: &TownStyle,
    pal: &Palette,
) {
    let (rows, cols) = (8, 9);
    let run = (ridge[0] + ridge[1] - eave[0] - eave[1]) * 0.5;
    let n = (eave[1] - eave[0]).cross(run).normalize();
    let n = if n.y < 0.0 { -n } else { n };
    let down = -run.normalize();
    let at = |u: f32, v: f32| {
        let sag = 0.07 * (u * std::f32::consts::PI).sin() * v;
        eave[0].lerp(eave[1], u).lerp(ridge[0].lerp(ridge[1], u), v)
            - Vec3::Y * sag
    };
    // The underside, so an eave seen from behind is not a hole.
    meshing.face(
        [
            eave[0] - n * 0.03,
            eave[1] - n * 0.03,
            ridge[1] - n * 0.03,
            ridge[0] - n * 0.03,
        ],
        -n,
        pal.timber_dark * 0.8,
    );
    let mossy = rng.f() * 40.0;
    for r in 0..rows {
        let (v0, v1) = (
            r as f32 / rows as f32,
            ((r as f32 + 1.3) / rows as f32).min(1.0),
        );
        let shift = if r % 2 == 0 { 0.0 } else { 0.5 };
        for c in 0..cols + 1 {
            let u0 = ((c as f32 - shift) / cols as f32).max(0.0);
            let u1 =
                ((c as f32 + 1.0 - shift) / cols as f32).min(1.0);
            if u1 - u0 < 1e-3 {
                continue;
            }
            let mut col = style.roof.lerp(style.drift, rng.f() * 0.8)
                * rng.value(0.1);
            if crate::meshes::noise2(u0 * 3.0 + mossy, v0 * 3.0)
                > 0.68
            {
                col = col.lerp(pal.moss, rng.range(0.35, 0.75));
            }
            let lip = n * 0.05 + down * rng.range(0.0, 0.05);
            meshing.face_cols(
                [
                    at(u0, v0) + lip,
                    at(u1, v0) + lip,
                    at(u1, v1),
                    at(u0, v1),
                ],
                n,
                [col * 0.7, col * 0.7, col, col],
            );
        }
    }
}
