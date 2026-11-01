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

@group(constants::MATERIAL_BIND_GROUP) @binding(104) var pages: texture_2d_array<f32>;
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

// The page of size `level` at the point `p` on the map, between its texels:
// the height, the woods and the heath.
fn page_at(level: i32, p: vec2<f32>) -> vec3<f32> {
    let at = pages_at.at[level];
    let size = vec2<i32>(textureDimensions(pages)) - 2;
    let g = (p - at.xy) / at.z;
    let i = floor(g);
    let f = g - i;
    let c = clamp(vec2<i32>(i), vec2<i32>(0), size);
    let a = textureLoad(pages, c, level, 0).xyz;
    let b = textureLoad(pages, c + vec2<i32>(1, 0), level, 0).xyz;
    let d = textureLoad(pages, c + vec2<i32>(0, 1), level, 0).xyz;
    let e = textureLoad(pages, c + vec2<i32>(1, 1), level, 0).xyz;
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
    let texel = pages_at.at[level].z;
    let left = page_at(level, p - vec2<f32>(texel, 0.0)).x;
    let right = page_at(level, p + vec2<f32>(texel, 0.0)).x;
    let ahead = page_at(level, p - vec2<f32>(0.0, texel)).x;
    let behind = page_at(level, p + vec2<f32>(0.0, texel)).x;
    return Land(page_at(level, p), vec3<f32>(left - right, 2.0 * texel, ahead - behind));
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
    var land = land_at(level, p);
    if laid.z > 0.0 && level + 1 < LEVELS {
        let larger = land_at(level + 1, p);
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
