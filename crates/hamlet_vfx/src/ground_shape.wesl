// The ground's shape, the same for its own pass and its prepass: each point
// of a patch, laid as `patch.wesl` has it, raised to the land's height from
// the page of the patch's size, with its slope and what the ground's shader
// colours it by. A patch larger than the smallest is sunk a little, as the
// realm's coarse tiles were: its flat triangles would otherwise rise over
// the ground as it is drawn up close and cover what stands on it.

import bevy_pbr::render::mesh_view_bindings::view;
import hamlet_vfx::patch::sliding;

// `PagesUniform` in `ground.rs`.
struct Pages {
    at: array<vec4<f32>, 8>,
    shape: vec4<f32>,
    water: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(104) var pages: texture_2d_array<u32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var<uniform> pages_at: Pages;

// `LEVELS` in `ground.rs`.
const LEVELS: i32 = 8;

// A point of the ground as it is drawn.
struct GroundPoint {
    position: vec3<f32>,
    normal: vec3<f32>,
    // The woods in red, the heath in green, and in blue how far the
    // ground stands above where it is drawn.
    colour: vec4<f32>,
}

// The texel `c` of the page of size `level`: the height, kept as its bits,
// and the woods and the heath, kept as two shares of 8 bits beside the
// distance from the shore, which the water reads.
fn texel(level: i32, c: vec2<i32>) -> vec3<f32> {
    let t = textureLoad(pages, c, level, 0).xy;
    return vec3<f32>(bitcast<f32>(t.x), unpack4x8unorm(t.y).xy);
}

// The page of size `level` at the point `p` on the map, between its texels:
// the height, the woods and the heath.
fn page_at(level: i32, p: vec2<f32>) -> vec3<f32> {
    let at = pages_at.at[level];
    let size = vec2<i32>(textureDimensions(pages)) - 2;
    let g = (p - at.xy) / at.z;
    let i = floor(g);
    let f = g - i;
    let c = clamp(vec2<i32>(i), vec2<i32>(0), size);
    let a = texel(level, c);
    let b = texel(level, c + vec2<i32>(1, 0));
    let d = texel(level, c + vec2<i32>(0, 1));
    let e = texel(level, c + vec2<i32>(1, 1));
    return mix(mix(a, b, f.x), mix(d, e, f.x), f.y);
}

// How far a patch of size `level` is sunk, where its ground stands high
// enough above the water: none for the smallest, twice as much for each size
// up, to the most.
fn sunk_by(level: i32) -> f32 {
    if level <= 0 {
        return 0.0;
    }
    return min(pages_at.shape.z * exp2(f32(level - 1)), pages_at.shape.w);
}

// The land at `p` as the page of size `level` has it, and its slope there,
// found a texel to each side: the height, the woods, the heath, and the
// normal.
struct Land {
    page: vec3<f32>,
    normal: vec3<f32>,
}

fn land_at(level: i32, p: vec2<f32>) -> Land {
    let at = pages_at.at[level];
    let g = (p - at.xy) / at.z;
    let i = floor(g);
    let f = g - i;
    let c = vec2<i32>(i);
    // The points a texel to each side lie between the same texels shifted
    // by one, so the twelve about the point's own four serve all five.
    let a = texel_near(level, c, 0, 0);
    let b = texel_near(level, c, 1, 0);
    let d = texel_near(level, c, 0, 1);
    let e = texel_near(level, c, 1, 1);
    let left = between(height_near(level, c, -1, 0), a.x, height_near(level, c, -1, 1), d.x, f);
    let right = between(b.x, height_near(level, c, 2, 0), e.x, height_near(level, c, 2, 1), f);
    let ahead = between(height_near(level, c, 0, -1), height_near(level, c, 1, -1), a.x, b.x, f);
    let behind = between(d.x, e.x, height_near(level, c, 0, 2), height_near(level, c, 1, 2), f);
    let page = mix(mix(a, b, f.x), mix(d, e, f.x), f.y);
    return Land(page, vec3<f32>(left - right, 2.0 * at.z, ahead - behind));
}

// The texel `x` across and `y` along from `c` on the page of size `level`,
// kept on the page.
fn texel_near(level: i32, c: vec2<i32>, x: i32, y: i32) -> vec3<f32> {
    let last = vec2<i32>(textureDimensions(pages)) - 1;
    return texel(level, clamp(c + vec2<i32>(x, y), vec2<i32>(0), last));
}

// The height of that texel alone.
fn height_near(level: i32, c: vec2<i32>, x: i32, y: i32) -> f32 {
    let last = vec2<i32>(textureDimensions(pages)) - 1;
    let at = clamp(c + vec2<i32>(x, y), vec2<i32>(0), last);
    return bitcast<f32>(textureLoad(pages, at, level, 0).x);
}

// The value `f` of the way between four, as `page_at` blends its texels.
fn between(a: f32, b: f32, d: f32, e: f32, f: vec2<f32>) -> f32 {
    return mix(mix(a, b, f.x), mix(d, e, f.x), f.y);
}

// The land at `p` as the page of size `level` has it, or as the first page
// up that holds it, should `p` lie off that page.
fn held_land(level: i32, p: vec2<f32>) -> Land {
    let side = vec2<f32>(textureDimensions(pages));
    for (var l = level; l < LEVELS - 1; l++) {
        let g = (p - pages_at.at[l].xy) / pages_at.at[l].z;
        if all(g > vec2<f32>(2.0)) && all(g < side - 3.0) {
            return land_at(l, p);
        }
    }
    return land_at(LEVELS - 1, p);
}

// The land at `p` for a pixel `pixel` metres across, whatever patch it is
// drawn on: from the page whose texels are as wide as the pixel, blended
// into the next size up as the pixel nears its texels, so that no seam
// shows where one gives way to the other.
fn seen_land(p: vec2<f32>, pixel: f32) -> Land {
    let level = clamp(log2(pixel / pages_at.at[0].z), 0.0, f32(LEVELS - 2));
    let low = i32(level);
    let fine = held_land(low, p);
    let t = fract(level);
    if t == 0.0 {
        return fine;
    }
    let coarse = held_land(low + 1, p);
    return Land(
        mix(fine.page, coarse.page, t),
        mix(normalize(fine.normal), normalize(coarse.normal), t),
    );
}

// The land under a point of a patch of size `level` at `p`. Only a normal
// prepass reads the slope there: the ground's own pixels read theirs from
// the pages.
fn point_land(level: i32, p: vec2<f32>) -> Land {
@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS) {
    return land_at(level, p);
}
@else {
    return Land(page_at(level, p), vec3<f32>(0.0, 1.0, 0.0));
}
}

// The point `local` of the patch moved by `world_from_local`, from 0 to 1
// over its side, as the ground draws it. As a point slides onto the grid
// of the patch twice its size, it takes that size's page too, so where the
// two sizes meet they stand on the same land, though the larger draws its
// rivers wider.
fn ground_point(local: vec2<f32>, world_from_local: mat4x4<f32>) -> GroundPoint {
    let corner = world_from_local[3].xz;
    let side = world_from_local[0].x;
    let level = clamp(i32(round(log2(side / pages_at.shape.x))), 0, LEVELS - 1);
    let laid = sliding(local, corner, side, pages_at.shape.y, view.world_position);
    let p = laid.xy;
    var land = point_land(level, p);
    if laid.z > 0.0 && level + 1 < LEVELS {
        let larger = point_land(level + 1, p);
        land = Land(
            mix(land.page, larger.page, laid.z),
            mix(normalize(land.normal), normalize(larger.normal), laid.z),
        );
    }
    let here = land.page;
    let water = pages_at.water.x;
    let sink = mix(sunk_by(level), sunk_by(level + 1), laid.z)
        * smoothstep(water + 0.3, water + 1.5, here.x);
    var out: GroundPoint;
    out.position = vec3<f32>(p.x, here.x - sink, p.y);
    out.normal = normalize(land.normal);
    out.colour = vec4<f32>(here.y, here.z, sink, 1.0);
    return out;
}
