//! The grass about the eye: tufts scattered over the land in squares
//! laid in rings, the nearest in the finest detail, which the GPU
//! sets on the land and bends; and the mask that keeps it off the
//! roads and the towns' squares.

use bevy::light::NotShadowCaster;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use hamlet_vfx::grass::{
    self, DETAILS, GrassMaterial, GrassSettings, IN_FLOWER, SHAPES,
    TUFTS,
};
use hamlet_vfx::ground::GroundMaterial;

use super::Kit;
use super::land::Land;
use super::noise::{key, unit};
use super::sync::Drawn;
use crate::WorldCamera;
use crate::meshes::{fbm, smoothstep};

/// Texels across the mask, and metres to a texel: enough to reach
/// past the furthest ring, as wide as the eye's height makes it.
const MASK: u32 = 512;
const MASK_TEXEL: f32 = 0.5;

/// How far the trodden ground spreads past what treads it, in metres,
/// as the grass thins to it.
const EDGE: f32 = 0.6;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, (begin, (follow, lay, tread)).chain());
}

/// The field: its material and each tuft's mesh at each detail, the
/// squares of tufts laid, each with its detail, and what the mask was
/// last made for.
#[derive(Resource)]
struct Field {
    material: Handle<GrassMaterial>,
    tufts: Vec<[Handle<Mesh>; DETAILS]>,
    laid: HashMap<IVec2, (Entity, usize)>,
    settings: GrassSettings,
    trodden: Option<(IVec2, u64)>,
}

/// A square of tufts laid together.
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
        tufts: tufts(&mut meshes, &set),
        laid: HashMap::new(),
        settings: set.clone(),
        trodden: None,
    });
}

/// Every tuft's mesh at every detail.
fn tufts(
    meshes: &mut Assets<Mesh>,
    set: &GrassSettings,
) -> Vec<[Handle<Mesh>; DETAILS]> {
    (0..TUFTS)
        .map(|shape| {
            std::array::from_fn(|detail| {
                meshes.add(grass::tuft(set, shape, detail))
            })
        })
        .collect()
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

/// Lays squares of tufts in rings about the eye, each in the detail
/// of the ring it lies in, and tells the grass where the eye is and
/// how far the rings reach. All are laid anew when the land changes.
fn lay(
    mut commands: Commands,
    field: Option<ResMut<Field>>,
    set: Res<GrassSettings>,
    land: Res<Land>,
    camera: Query<&GlobalTransform, With<WorldCamera>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<GrassMaterial>>,
) {
    let (Some(mut field), Some(eye)) = (field, camera.iter().next())
    else {
        return;
    };
    if field.settings != *set || land.is_changed() {
        if field.settings != *set {
            field.settings = set.clone();
            field.tufts = tufts(&mut meshes, &set);
        }
        for (entity, _) in field.laid.drain().map(|(_, laid)| laid) {
            commands.entity(entity).despawn();
        }
    }
    let at = eye.translation();
    let centre = at.xz();
    let low = (at.y - land.height(centre)).max(0.0);
    // Seen from higher up, the nearest blades are further off: the
    // rings widen with the eye's height, and draw a detail coarser.
    let wide = (low / 12.0).clamp(1.0, 1.8);
    let coarser = usize::from(wide > 1.4);
    let rings = set.rings.map(|r| r * wide);
    let far = rings[2];
    if let Some(mut grass) = materials.get_mut(&field.material) {
        let reach = at.extend(far);
        if grass.extension.grass.reach != reach {
            grass.extension.grass.reach = reach;
        }
    }

    let side = set.cell;
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
                wanted.insert(cell, (ring + coarser).min(2));
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
                Transform::default(),
                Visibility::default(),
            ))
            .id();
        for tuft in grow(&land, &set, corner, corner + side) {
            commands.spawn((
                Mesh3d(field.tufts[tuft.shape][detail].clone()),
                MeshMaterial3d(field.material.clone()),
                tuft.transform,
                NotShadowCaster,
                ChildOf(square),
            ));
        }
        field.laid.insert(cell, (square, detail));
    }
}

/// A tuft as it grows: its shape among [`TUFTS`], and where it
/// stands, turned and scaled.
struct Tuft {
    shape: usize,
    transform: Transform,
}

/// The tufts that grow between `lo` and `hi`: in each square
/// `spacing` across of the map at most one, somewhere in it, so they
/// lie scattered and never in rows. None grow in or by the water, on
/// steep or high ground, and fewer in the woods.
fn grow(
    land: &Land,
    set: &GrassSettings,
    lo: Vec2,
    hi: Vec2,
) -> Vec<Tuft> {
    let step = set.spacing;
    let first = (lo / step).floor().as_ivec2();
    let last = (hi / step).ceil().as_ivec2();
    let water = land.set.land.water_y;
    let mut out = Vec::new();
    for x in first.x..last.x {
        for z in first.y..last.y {
            let h = key(&[land.seed, 93, x as u32, z as u32]);
            let roll = |salt: u32| unit(key(&[h, salt]));
            let p = (IVec2::new(x, z).as_vec2()
                + Vec2::new(roll(0), roll(1)))
                * step;
            if p.cmplt(lo).any()
                || p.cmpge(hi).any()
                || roll(2) < set.bare
            {
                continue;
            }
            let y = land.height(p);
            let steep = 1.0 - land.normal(p).y;
            let woods = land.forest(p);
            let grows = smoothstep(water + 0.5, water + 1.0, y)
                * smoothstep(0.14, 0.07, steep)
                * smoothstep(26.0, 18.0, y)
                * (1.0 - 0.7 * woods);
            if roll(3) > grows {
                continue;
            }
            let blooms = fbm(p.x * 0.08 + 31.0, p.y * 0.08 - 17.0);
            let shape = if blooms > 0.6
                && roll(4) < set.flowers * (1.0 - woods)
            {
                SHAPES + (roll(5) * IN_FLOWER as f32) as usize
            } else {
                (roll(5) * SHAPES as f32) as usize
            };
            let across = set.across.0
                * (set.across.1 / set.across.0).powf(roll(6));
            let up = set.up.0
                + (set.up.1 - set.up.0)
                    * roll(7)
                    * roll(7)
                    * (1.0 - 0.4 * woods);
            out.push(Tuft {
                shape: shape.min(TUFTS - 1),
                transform: Transform::from_xyz(p.x, y, p.y)
                    .with_rotation(Quat::from_rotation_y(
                        roll(8) * std::f32::consts::TAU,
                    ))
                    .with_scale(Vec3::new(across, up, across)),
            });
        }
    }
    out
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
    let square = land.set.town.plaza_radius + 0.5;
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
