//! The grass about where the eye looks: square patches of the field
//! laid in rings, the nearest with the most blades, which the GPU
//! grows on the land; and the mask that keeps it off held ground,
//! the roads and the towns' squares.

use bevy::camera::primitives::Aabb;
use bevy::light::NotShadowCaster;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use hamlet_vfx::grass::{self, GrassMaterial, GrassSettings};
use hamlet_vfx::ground::GroundMaterial;

use super::Kit;
use super::land::Land;
use super::sync::Drawn;
use crate::WorldCamera;

/// Texels across the mask, and metres to a texel.
const MASK: u32 = 256;
const MASK_TEXEL: f32 = 0.5;

/// How far the trodden ground spreads past what treads it, in metres,
/// as the grass thins to it.
const EDGE: f32 = 0.6;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, (begin, (follow, lay, tread)).chain());
}

/// The field: its material and a patch mesh for each ring, the
/// patches laid by their cell, each with its ring, and what the mask
/// was last made for.
#[derive(Resource)]
struct Field {
    material: Handle<GrassMaterial>,
    patches: [Handle<Mesh>; 3],
    laid: HashMap<IVec2, (Entity, usize)>,
    settings: GrassSettings,
    trodden: Option<(IVec2, u64)>,
}

/// A patch of the field.
#[derive(Component)]
struct Patch;

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
        patches: patches(&mut meshes, &set),
        laid: HashMap::new(),
        settings: set.clone(),
        trodden: None,
    });
}

fn patches(
    meshes: &mut Assets<Mesh>,
    set: &GrassSettings,
) -> [Handle<Mesh>; 3] {
    set.blades
        .map(|count| meshes.add(grass::patch(count, set.patch)))
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

/// Where on the map the eye looks: where its gaze meets the land, or
/// under it when it looks too far ahead.
fn looked_at(eye: &GlobalTransform, land: &Land) -> Vec2 {
    let at = eye.translation();
    let ahead = eye.forward();
    let below = at.y - land.height(at.xz());
    if ahead.y > -0.05 {
        return at.xz();
    }
    let far = (below / -ahead.y).min(80.0);
    at.xz() + ahead.xz() * far
}

/// Lays the patches in rings about where the eye looks, each as the
/// ring it lies in has it, and tells the grass where that is.
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
    if field.settings != *set {
        field.settings = set.clone();
        field.patches = patches(&mut meshes, &set);
        for (entity, _) in field.laid.drain().map(|(_, laid)| laid) {
            commands.entity(entity).despawn();
        }
    }
    let centre = looked_at(eye, &land);
    if let Some(mut grass) = materials.get_mut(&field.material) {
        let reach = &mut grass.extension.grass.reach;
        if reach.xy() != centre {
            *reach = centre.extend(reach.z).extend(reach.w);
        }
    }

    let side = set.patch;
    let far = set.rings[2];
    let lo = ((centre - far) / side).floor().as_ivec2();
    let hi = ((centre + far) / side).ceil().as_ivec2();
    let mut wanted = HashMap::new();
    for x in lo.x..hi.x {
        for z in lo.y..hi.y {
            let cell = IVec2::new(x, z);
            let middle = (cell.as_vec2() + 0.5) * side;
            // The nearest point of the patch decides its ring.
            let near = (centre - middle).abs() - side * 0.5;
            let away = near.max(Vec2::ZERO).length();
            if let Some(ring) =
                set.rings.iter().position(|r| away < *r)
            {
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
    for (cell, ring) in wanted {
        if field.laid.contains_key(&cell) {
            continue;
        }
        // A ring of fewer blades draws them wider, to cover as much.
        let wider = (set.blades[0] as f32 / set.blades[ring] as f32)
            .sqrt()
            .max(1.0);
        let corner = cell.as_vec2() * side;
        let entity = commands
            .spawn((
                Patch,
                Mesh3d(field.patches[ring].clone()),
                MeshMaterial3d(field.material.clone()),
                Transform::from_xyz(corner.x, 0.0, corner.y)
                    .with_scale(Vec3::new(wider, 1.0, wider)),
                // The blades stand wherever the land is; the patch
                // reaches over all the heights it may have.
                Aabb::from_min_max(
                    Vec3::new(-1.0, -40.0, -1.0) / wider,
                    Vec3::new(side + 1.0, 120.0, side + 1.0) / wider,
                ),
                NotShadowCaster,
            ))
            .id();
        field.laid.insert(cell, (entity, ring));
    }
}

/// Makes the mask anew about where the eye looks when that has moved
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
        .map(|grass| grass.extension.grass.reach.xy())
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

/// What treads the grass down: the ground held, each road's steps and
/// each town's square, each as a line from one point to another with
/// how far it reaches to each side.
fn marks(land: &Land, drawn: &Drawn) -> Vec<(Vec2, Vec2, f32)> {
    let mut out = land
        .rounds()
        .map(|(at, radius)| (at, at, radius + 0.3))
        .collect::<Vec<_>>();
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
