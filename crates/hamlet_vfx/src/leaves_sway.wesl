// What every pass of the leaves shares: how they move in the wind, so a
// tree's shadow and depth move with it, and which of their pixels are
// cut away, so its depth and shadow have the same holes.

import hamlet_vfx::wind::{Wind, strength};

// The fields are explained on `LeafSettings` in `leaves.rs`.
struct Leaves {
    // x: time in seconds. y: `bend`, 0 for leaves that keep still. z:
    // `rock`. w: `flutter`, 0 for leaves that keep still.
    sway: vec4<f32>,
    // x: `shiver`. y: `glow`. z: `sky`. w: how far into the night it is.
    edge: vec4<f32>,
    // rgb: `shade`, linear. w: `cool`.
    shade: vec4<f32>,
    // rgb: `rim`, linear. w: `rim_light`.
    rim: vec4<f32>,
    // x: `clear`. The rest is unused.
    open: vec4<f32>,
    // xyz: the point the eye watches. w: `opening`, 0 for none.
    watch: vec4<f32>,
}

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
    let bent = rise * rise * size;
    let strong = strength(wind, foot.xz);
    let own = foot.x * 0.37 + foot.z * 0.23;
    let t = leaves.sway.x;
    let rock = leaves.sway.z;
    let lean = leaves.sway.y * strong
        * (0.7 + 0.3 * sin(t * rock + own));
    let side = leaves.sway.y * strong * 0.25
        * sin(t * rock * 0.71 + own * 1.9);
    let down = vec3<f32>(wind.flow.x, 0.0, wind.flow.y);
    let across = vec3<f32>(-wind.flow.y, 0.0, wind.flow.x);
    // A bent top comes down a little, as its length is kept.
    let sink = -0.5 * (lean * lean + side * side) / CROWN;
    var moved = (down * lean + across * side + vec3<f32>(0.0, sink, 0.0)) * bent;
    // Slow across the crown, so a card moves as one and is not torn.
    let leaf = dot(local, vec3<f32>(1.3, 1.1, 1.2)) + own;
    let shiver = leaves.sway.w * strong * rise * size;
    moved += shiver * vec3<f32>(
        sin(t * leaves.edge.x + leaf),
        0.5 * sin(t * leaves.edge.x * 1.31 + leaf * 1.7),
        sin(t * leaves.edge.x * 0.83 + leaf * 2.3),
    );
    return world + moved;
}

// A threshold from 0 to 1 for the pixel at `pixel`, spread so that
// neighbours differ as much as they can: a dither that reads as an even
// fade, not as a pattern.
fn dither(pixel: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(pixel, vec2<f32>(0.06711056, 0.00583715))));
}

// How much of a leaf at `at` is left between the eye at `eye` and the
// point it watches: a cone from the eye that opens to `opening` metres
// at that point. Leaves at or past the point are left whole.
fn left_open(leaves: Leaves, at: vec3<f32>, eye: vec3<f32>) -> f32 {
    let opening = leaves.watch.w;
    if opening <= 0.0 {
        return 1.0;
    }
    let to = leaves.watch.xyz - eye;
    let far = length(to);
    let way = to / max(far, 1e-3);
    let along = dot(at - eye, way);
    let k = along / max(far, 1e-3);
    let off = length(at - eye - way * along);
    let wide = opening * max(k, 0.15);
    let inside = smoothstep(wide, wide * 0.55, off)
        * smoothstep(0.0, 0.1, k)
        * smoothstep(1.0, 0.85, k);
    return 1.0 - inside * leaves.open.x;
}

// Whether to cut away the pixel at `pixel` of a leaf as much `cover`ed.
fn is_cut(cover: f32, pixel: vec2<f32>) -> bool {
    return cover <= dither(pixel);
}
