// What every pass of the leaves shares: how they move in the wind, so a
// tree's shadow and depth move with it, and which of their pixels are
// cut away, so its depth and shadow have the same holes.

import hamlet_vfx::clock::scene_time;
import hamlet_vfx::wind::{Wind, blowing, strength};

// The fields are explained on `LeafSettings` in `leaves.rs`.
struct Leaves {
    // x: `NowOffset`. y: `bend`, 0 for leaves that keep still. z:
    // `rock`. w: `flutter`, 0 for leaves that keep still.
    sway: vec4<f32>,
    // x: `shiver`. y: `glow`. z: `sky`. w: how far into the night it is.
    edge: vec4<f32>,
    // rgb: `shade`, linear. w: `cool`.
    shade: vec4<f32>,
    // rgb: `rim`, linear. w: `rim_light`.
    rim: vec4<f32>,
}

// Texels across the strokes' texture, `SIDE` in `leaves.rs`.
const STROKES: f32 = 512.0;

// The height of a tree's crown on its mesh, which leans the most.
const CROWN: f32 = 3.0;

// Where the point `local` of a tree's mesh is blown to, in the world.
// The tree stands on its mesh's origin, and bends from there: not at
// all at its foot, and the more the higher up. All of it leans down the
// wind and rocks there, each tree at its own moment; its leaves shiver
// on top of that, each at its own.
fn swayed(
    leaves: Leaves,
    wind: Wind,
    local: vec3<f32>,
    world_from_local: mat4x4<f32>,
) -> vec3<f32> {
    let foot = world_from_local[3].xyz;
    let size = length(world_from_local[1].xyz);
    let world = (world_from_local * vec4<f32>(local, 1.0)).xyz;
    let rise = clamp(local.y / CROWN, 0.0, 1.0);
    return blown(leaves, wind, world, foot.xz, rise * rise * size, rise * size);
}

// Where the point `world` of a plant among many in one mesh is blown
// to. Its mesh gives each point how far it `bends`, as a share of how
// far a tree's crown does, and it shivers as much as it bends.
fn grown(leaves: Leaves, wind: Wind, world: vec3<f32>, bends: f32) -> vec3<f32> {
    return blown(leaves, wind, world, world.xz, bends, sqrt(bends));
}

// Where the point `world` of a plant standing at `foot` is blown to,
// leaning `bent` of the way a crown does and shivering as much as
// `shaken`.
fn blown(
    leaves: Leaves,
    wind: Wind,
    world: vec3<f32>,
    foot: vec2<f32>,
    bent: f32,
    shaken: f32,
) -> vec3<f32> {
    let t = scene_time(leaves.sway.x);
    let strong = strength(wind, foot, t);
    let own = foot.x * 0.37 + foot.y * 0.23;
    let rock = leaves.sway.z;
    let lean = leaves.sway.y * strong
        * (0.7 + 0.3 * sin(t * rock + own));
    let side = leaves.sway.y * strong * 0.25
        * sin(t * rock * 0.71 + own * 1.9);
    let way = blowing(wind, t);
    let down = vec3<f32>(way.x, 0.0, way.y);
    let across = vec3<f32>(-way.y, 0.0, way.x);
    // A bent top comes down a little, as its length is kept.
    let sink = -0.5 * (lean * lean + side * side) / CROWN;
    var moved = (down * lean + across * side + vec3<f32>(0.0, sink, 0.0)) * bent;
    // Slow across the crown, so a card moves as one and is not torn.
    let leaf = dot(world, vec3<f32>(1.3, 1.1, 1.2));
    let shiver = leaves.sway.w * strong * shaken;
    moved += shiver * vec3<f32>(
        sin(t * leaves.edge.x + leaf),
        0.5 * sin(t * leaves.edge.x * 1.31 + leaf * 1.7),
        sin(t * leaves.edge.x * 0.83 + leaf * 2.3),
    );
    return world + moved;
}

// How much of a stroke covers the point `uv` of its card, where the
// strokes' texture gives `cover`. Further down its levels a stroke is
// blurred into the paper about it, so its cover is raised there by as
// much as it is spread, and a far card keeps the size of a near one.
fn stroke_cover(cover: f32, uv: vec2<f32>) -> f32 {
    let texels = uv * STROKES;
    let spread = max(length(dpdx(texels)), length(dpdy(texels)));
    let level = clamp(log2(max(spread, 1.0)), 0.0, 4.0);
    return cover * (1.0 + 0.25 * level);
}

// Whether to cut away a pixel of a leaf `cover`ed so much by its
// stroke: sharp, at the stroke's edge.
fn is_cut(cover: f32) -> bool {
    return cover < 0.5;
}
