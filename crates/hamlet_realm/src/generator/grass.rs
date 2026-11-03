//! The grass about the eye: squares of tuft slots laid in rings, the
//! nearest in the finest detail, on which the GPU grows, places and
//! bends every tuft from the land's pages; the mask that keeps it off
//! the roads and the towns' squares.

use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::NoAutoAabb;
use bevy::light::NotShadowCaster;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use hamlet_vfx::grass::{
    self, DETAILS, GrassMaterial, GrassReach, GrassSettings,
};
use hamlet_vfx::ground::GroundMaterial;

use super::Kit;
use super::land::Land;
use super::sync::Drawn;
use crate::WorldCamera;
use crate::sky::Quality;

/// Texels across the mask, and metres to a texel: enough to reach
/// past the furthest ring, as wide as the eye's height makes it.
const MASK: u32 = 1024;
const MASK_TEXEL: f32 = 0.5;

/// How far the mask fades past what treads it, in metres. A tuft
/// whose middle is more than half trodden does not grow, so the
/// grass stops about halfway along it.
const EDGE: f32 = 0.4;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, (begin, (follow, lay, tread)).chain());
}

/// The field: its material and a square's mesh at each detail, the
/// squares laid, each with its detail, and what the mask was last
/// made for.
#[derive(Resource)]
struct Field {
    material: Handle<GrassMaterial>,
    squares: [Handle<Mesh>; DETAILS],
    laid: HashMap<IVec2, (Entity, usize)>,
    settings: GrassSettings,
    trodden: Option<(IVec2, u64)>,
}

/// A square of tuft slots.
#[derive(Component)]
struct Square;

/// Makes the field once the kit is.
fn begin(
    mut commands: Commands,
    field: Option<Res<Field>>,
    kit: Option<Res<Kit>>,
    land: Res<Land>,
    set: Res<GrassSettings>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<GrassMaterial>>,
) {
    if field.is_some() || kit.is_none() {
        return;
    }
    let mut material =
        grass::material(&mut images, &set, land.soil());
    material.extension.mask = images.add(grass::mask(MASK));
    material.extension.grass.flowers =
        land.pal.flowers.map(|c| c.extend(1.0));
    commands.insert_resource(Field {
        material: materials.add(material),
        squares: squares(&mut meshes, &set),
        laid: HashMap::new(),
        settings: set.clone(),
        trodden: None,
    });
}

/// A square's mesh at every detail.
fn squares(
    meshes: &mut Assets<Mesh>,
    set: &GrassSettings,
) -> [Handle<Mesh>; DETAILS] {
    std::array::from_fn(|detail| {
        meshes.add(grass::square(set, detail))
    })
}

/// Gives the grass the land's pages and colours as the ground has
/// them.
fn follow(
    field: Option<Res<Field>>,
    kit: Option<Res<Kit>>,
    grounds: Res<Assets<GroundMaterial>>,
    mut materials: ResMut<Assets<GrassMaterial>>,
) {
    let (Some(field), Some(kit)) = (field, kit) else {
        return;
    };
    let Some(ground) = grounds.get(&kit.ground) else {
        return;
    };
    let ground = &ground.extension;
    let Some(grass) = materials.get(&field.material) else {
        return;
    };
    let grass = &grass.extension;
    if grass.pages == ground.pages
        && grass.pages_at == ground.pages_at
        && grass.soil == ground.soil
    {
        return;
    }
    if let Some(mut grass) = materials.get_mut(&field.material) {
        grass.extension.pages = ground.pages.clone();
        grass.extension.pages_at = ground.pages_at.clone();
        grass.extension.soil = ground.soil.clone();
    }
}

/// Lays squares of tuft slots in rings about the eye, each in the
/// detail of the ring it lies in, and tells the grass where the eye
/// is and how far the rings reach. The land under them is the GPU's
/// to read, so nothing here changes with it.
fn lay(
    mut commands: Commands,
    field: Option<ResMut<Field>>,
    set: Res<GrassSettings>,
    quality: Res<Quality>,
    land: Res<Land>,
    camera: Query<&GlobalTransform, With<WorldCamera>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<GrassMaterial>>,
) {
    let (Some(mut field), Some(eye)) = (field, camera.iter().next())
    else {
        return;
    };
    // The quality draws the grass less far, its tufts further apart,
    // and fewer of them past the nearest ring, which the coarser
    // squares leave out.
    let share = quality.grass();
    let built = GrassSettings {
        kept: [
            set.kept[0],
            set.kept[1] * share.kept,
            set.kept[2] * share.kept,
        ],
        spacing: set.spacing * share.spacing,
        ..set.clone()
    };
    if field.settings != built {
        field.squares = squares(&mut meshes, &built);
        field.settings = built;
        for (entity, _) in field.laid.drain().map(|(_, laid)| laid) {
            commands.entity(entity).despawn();
        }
    }
    let at = eye.translation();
    let centre = at.xz();
    let low = (at.y - land.height(centre)).max(0.0);
    // The rings are measured from the eye itself, so seen from higher
    // up even the ground under it falls to a coarser one.
    let rings = set.rings.map(|r| r * share.reach);
    let far = rings[2];
    let reach = GrassReach {
        eye: at,
        low,
        rings,
        kept: share.kept,
        spacing: field.settings.spacing,
    };
    if let Some(grass) = materials.get(&field.material)
        && grass.extension.grass.reach() != reach
        && let Some(mut grass) = materials.get_mut(&field.material)
    {
        grass.extension.grass.set_reach(reach);
    }

    // A whole number of tufts' squares, so the slots of one square
    // run on from the last's.
    let side = field.settings.spacing * set.slots.max(1) as f32;
    let lo = ((centre - far) / side).floor().as_ivec2();
    let hi = ((centre + far) / side).ceil().as_ivec2();
    let mut wanted = HashMap::new();
    for x in lo.x..hi.x {
        for z in lo.y..hi.y {
            let cell = IVec2::new(x, z);
            let middle = (cell.as_vec2() + 0.5) * side;
            // The nearest point of the patch decides its ring.
            let near = (centre - middle).abs() - side * 0.5;
            let away = near.max(Vec2::ZERO).length().hypot(low);
            if let Some(ring) = rings.iter().position(|r| away < *r) {
                wanted.insert(cell, ring);
            }
        }
    }
    let mut gone = Vec::new();
    for (cell, &(entity, ring)) in &field.laid {
        if wanted.get(cell) != Some(&ring) {
            commands.entity(entity).despawn();
            gone.push(*cell);
        }
    }
    for cell in gone {
        field.laid.remove(&cell);
    }
    for (cell, detail) in wanted {
        if field.laid.contains_key(&cell) {
            continue;
        }
        let corner = cell.as_vec2() * side;
        let square = commands
            .spawn((
                Square,
                Mesh3d(field.squares[detail].clone()),
                MeshMaterial3d(field.material.clone()),
                Transform::from_xyz(corner.x, 0.0, corner.y),
                // The tufts stand wherever the land is, and reach a
                // little past their slots; the square holds every
                // height they may stand at. Its mesh holds only one
                // tuft's room, at the origin, so Bevy must not work
                // its bounds out from it.
                Aabb::from_min_max(
                    Vec3::new(-2.0, -40.0, -2.0),
                    Vec3::new(side + 2.0, 120.0, side + 2.0),
                ),
                NoAutoAabb,
                NotShadowCaster,
            ))
            .id();
        field.laid.insert(cell, (square, detail));
    }
}

/// Makes the mask anew about the eye when it has moved
/// far enough, or the ground held, the roads or the towns change.
fn tread(
    field: Option<ResMut<Field>>,
    land: Res<Land>,
    drawn: Res<Drawn>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<GrassMaterial>>,
) {
    let Some(mut field) = field else {
        return;
    };
    let Some(centre) = materials
        .get(&field.material)
        .map(|grass| grass.extension.grass.reach.xz())
    else {
        return;
    };
    let side = MASK as f32 * MASK_TEXEL;
    let step = side / 8.0;
    let cell = (centre / step).round().as_ivec2();
    let marks = marks(&land, &drawn);
    let sum = marks.iter().fold(0u64, |sum, (a, b, half)| {
        [a.x, a.y, b.x, b.y, *half].iter().fold(sum, |sum, v| {
            sum.wrapping_mul(31).wrapping_add(v.to_bits() as u64)
        })
    });
    if field.trodden == Some((cell, sum)) {
        return;
    }
    field.trodden = Some((cell, sum));
    let corner = cell.as_vec2() * step - side * 0.5;
    let mut data = vec![0u8; (MASK * MASK) as usize];
    for (a, b, half) in marks {
        tread_on(&mut data, corner, a, b, half);
    }
    let Some(mut grass) = materials.get_mut(&field.material) else {
        return;
    };
    grass.extension.grass.mask_at =
        Vec4::new(corner.x, corner.y, side, 0.0);
    if let Some(mut image) = images.get_mut(&grass.extension.mask) {
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
