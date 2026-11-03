//! The grass about the eye, which the GPU grows, places, thins and
//! bends tuft by tuft from the land's pages. The world tells it where
//! the eye is and what it sees, how far the quality lets it reach,
//! the land's pages, and the mask that keeps it off the roads and the
//! towns' squares.

use bevy::camera::primitives::Frustum;
use bevy::camera::visibility::VisibilitySystems;
use bevy::prelude::*;
use hamlet_vfx::NowOffset;
use hamlet_vfx::grass::{self, Grass, GrassReach, GrassSettings};
use hamlet_vfx::ground::GroundMaterial;

use super::land::Land;
use super::sync::Drawn;
use super::{Kit, relief};
use crate::WorldCamera;
use crate::sky::Quality;

/// Texels across the mask, enough to reach past the grass's reach.
const MASK: u32 = 1024;
/// Metres to a texel of the mask.
const MASK_TEXEL: f32 = 0.5;

/// How far the mask fades past what treads it, in metres. A tuft
/// whose middle is more than half trodden does not grow, so the
/// grass stops about halfway along it.
const EDGE: f32 = 0.4;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (begin, (follow.after(relief::PagesLaid), tread)).chain(),
    )
    // Once the eye has moved this frame and what it sees is
    // known, so the grass is kept to the view it is drawn from.
    .add_systems(
        PostUpdate,
        reach.after(VisibilitySystems::UpdateFrusta),
    );
}

/// Where the mask was last made about, and for what treads it.
#[derive(Resource, Default)]
struct Trodden(Option<(IVec2, u64)>);

/// Makes the grass once the kit is.
fn begin(
    mut commands: Commands,
    grass: Option<Res<Grass>>,
    kit: Option<Res<Kit>>,
    land: Res<Land>,
    set: Res<GrassSettings>,
    mut images: ResMut<Assets<Image>>,
) {
    if grass.is_some() || kit.is_none() {
        return;
    }
    let mut grass = Grass::new(&mut images, &set, land.soil());
    grass.mask = images.add(grass::mask(MASK));
    commands.insert_resource(grass);
    commands.init_resource::<Trodden>();
}

/// Gives the grass the land's pages and colours as the ground has
/// them, and the ground the grass's flowers to paint where the
/// grass's own are too small to show.
fn follow(
    grass: Option<ResMut<Grass>>,
    kit: Option<Res<Kit>>,
    offset: Res<NowOffset>,
    mut grounds: ResMut<Assets<GroundMaterial>>,
) {
    let (Some(mut grass), Some(kit)) = (grass, kit) else {
        return;
    };
    let Some(ground) = grounds.get(&kit.ground) else {
        return;
    };
    let blooms = grass.grass.blooms(*offset);
    if ground.extension.blooms != blooms
        && let Some(mut ground) = grounds.get_mut(&kit.ground)
    {
        ground.extension.blooms = blooms;
    }
    let Some(ground) = grounds.get(&kit.ground) else {
        return;
    };
    let ground = &ground.extension;
    if grass.pages != ground.pages {
        grass.pages = ground.pages.clone();
    }
    if grass.pages_at != ground.pages_at {
        grass.pages_at = ground.pages_at.clone();
    }
    if grass.soil != ground.soil {
        grass.soil = ground.soil.clone();
    }
}

/// Tells the grass where the eye is and what it sees, and how far,
/// how thick and how close together the quality lets it grow.
fn reach(
    grass: Option<ResMut<Grass>>,
    set: Res<GrassSettings>,
    quality: Res<Quality>,
    land: Res<Land>,
    camera: Query<(&GlobalTransform, &Frustum), With<WorldCamera>>,
) {
    let (Some(mut grass), Some((eye, frustum))) =
        (grass, camera.iter().next())
    else {
        return;
    };
    let share = quality.grass();
    let at = eye.translation();
    let low = (at.y - land.height(at.xz())).max(0.0);
    // The details are measured from the eye itself, so seen from
    // higher up even the ground under it is drawn coarser.
    let reach = GrassReach {
        eye: at,
        low,
        rings: set.rings.map(|r| r * share.reach),
        kept: share.kept,
        spacing: if share.coarse {
            set.coarse_spacing
        } else {
            set.spacing
        },
    };
    grass.grass.set_reach(reach);
    grass.sight = frustum.0.half_spaces.map(|half| half.normal_d());
}

/// Makes the mask anew about the eye when it has moved
/// far enough, or the roads or the towns change.
fn tread(
    grass: Option<ResMut<Grass>>,
    trodden: Option<ResMut<Trodden>>,
    land: Res<Land>,
    drawn: Res<Drawn>,
    mut images: ResMut<Assets<Image>>,
) {
    let (Some(mut grass), Some(mut trodden)) = (grass, trodden)
    else {
        return;
    };
    let centre = grass.grass.reach.xz();
    let side = MASK as f32 * MASK_TEXEL;
    let step = side / 8.0;
    let cell = (centre / step).round().as_ivec2();
    // The marks are gathered only when the eye has moved far enough
    // or what treads the grass may have changed.
    if trodden.0.is_some_and(|(at, _)| at == cell)
        && !land.is_changed()
        && !drawn.is_changed()
    {
        return;
    }
    let marks = marks(&land, &drawn);
    let sum = marks.iter().fold(0u64, |sum, (a, b, half)| {
        [a.x, a.y, b.x, b.y, *half].iter().fold(sum, |sum, v| {
            sum.wrapping_mul(31).wrapping_add(v.to_bits() as u64)
        })
    });
    if trodden.0 == Some((cell, sum)) {
        return;
    }
    trodden.0 = Some((cell, sum));
    let corner = cell.as_vec2() * step - side * 0.5;
    let mut data = vec![0u8; (MASK * MASK) as usize];
    for (a, b, half) in marks {
        tread_on(&mut data, corner, a, b, half);
    }
    grass.grass.mask_at = Vec4::new(corner.x, corner.y, side, 0.0);
    if let Some(mut image) = images.get_mut(&grass.mask) {
        image.data = Some(data);
    }
}

/// What treads the grass down: each road's steps and each town's
/// square, each as a line from one point to another with how far it
/// reaches to each side. Held ground is only levelled, and grows
/// grass up to the walls on it.
fn marks(land: &Land, drawn: &Drawn) -> Vec<(Vec2, Vec2, f32)> {
    let mut out = Vec::new();
    let square = land.set.town.plaza_radius;
    out.extend(land.settled().iter().filter_map(|&site| {
        land.sites.get(site).map(|s| (s.at, s.at, square))
    }));
    for road in drawn.roads.values() {
        let half =
            land.set.road.half_width[road.level as usize] + 0.2;
        let points = &road.route.line.pts;
        out.extend(
            points
                .windows(2)
                .map(|step| (step[0].xz(), step[1].xz(), half)),
        );
    }
    out
}

/// Marks the mask with its corner at `corner` trodden within `half`
/// of the line from `a` to `b`, and less so for [`EDGE`] past that.
fn tread_on(
    data: &mut [u8],
    corner: Vec2,
    a: Vec2,
    b: Vec2,
    half: f32,
) {
    let reach = half + EDGE;
    let lo = ((a.min(b) - reach - corner) / MASK_TEXEL)
        .floor()
        .as_ivec2();
    let hi =
        ((a.max(b) + reach - corner) / MASK_TEXEL).ceil().as_ivec2();
    let lo = lo.max(IVec2::ZERO);
    let hi = hi.min(IVec2::splat(MASK as i32 - 1));
    let along = b - a;
    for y in lo.y..=hi.y {
        for x in lo.x..=hi.x {
            let p = corner
                + (Vec2::new(x as f32, y as f32) + 0.5) * MASK_TEXEL;
            let t = ((p - a).dot(along)
                / along.length_squared().max(1e-6))
            .clamp(0.0, 1.0);
            let away = p.distance(a + along * t);
            let k = 1.0 - ((away - half) / EDGE).clamp(0.0, 1.0);
            let at = &mut data[(y as u32 * MASK + x as u32) as usize];
            *at = (*at).max((k * 255.0).round() as u8);
        }
    }
}
