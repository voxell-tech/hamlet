// A point of a tuft of grass, the same in its own pass and in the
// prepass: placed, scaled and turned as its tuft is, then set on the land
// under it as the finest page has it, shrunk where the ground is trodden
// and toward the edge of the eye's reach, and bent by the wind, which rolls
// over the field in the wind's own bands.

import hamlet_vfx::ground_shape::{land_at, page_at};
import hamlet_vfx::soil::{soil, grass_colour};
import hamlet_vfx::wind::{Wind, strength};

// `GrassUniform` in `grass.rs`; the settings are explained on
// `GrassSettings` there.
struct Grass {
    // x: `root`. y: `tip`. z: `sun`. w: `sheen`.
    look: vec4<f32>,
    // x: `bend`. y: the time in seconds.
    sway: vec4<f32>,
    // xyz: the eye. w: how far from it the grass is gone, shortening
    // over the last third of the way.
    reach: vec4<f32>,
    // xy: the mask's corner on the map. z: its side.
    mask_at: vec4<f32>,
    // The flowers' colours, linear.
    flowers: array<vec4<f32>, 5>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> grass: Grass;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var<uniform> wind: Wind;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var mask: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(107) var mask_sampler: sampler;

// A point of a tuft as it is drawn: where, the ground's normal under it,
// which it is lit by, and its colour.
struct Blade {
    position: vec3<f32>,
    normal: vec3<f32>,
    colour: vec3<f32>,
}

// How trodden the ground is at `p` on the map, from 0 to 1. None outside
// the mask.
fn trodden(p: vec2<f32>) -> f32 {
    let uv = (p - grass.mask_at.xy) / grass.mask_at.z;
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return 0.0;
    }
    return textureSampleLevel(mask, mask_sampler, uv, 0.0).r;
}

// The point `local` of a tuft moved by `world_from_local`, which stands it
// on the land at its middle; `up` is how far up its blade the point is, from
// 0 to 1, `own` its blade's own number, `tone` what its blade multiplies the
// ground's colour by, and `flower` whether it is a flower's head.
fn blade(
    local: vec3<f32>,
    up: f32,
    own: f32,
    tone: vec3<f32>,
    flower: bool,
    world_from_local: mat4x4<f32>,
) -> Blade {
    let middle = world_from_local[3].xyz;
    var at = (world_from_local * vec4<f32>(local, 1.0)).xyz;
    let land = land_at(0, at.xz);
    let ground = land.page.x;
    // How high over its middle's ground the point stands, kept over the
    // ground under it, and shrunk where trodden and far off.
    let away = distance(middle, grass.reach.xyz);
    let kept = (1.0 - trodden(at.xz))
        * smoothstep(grass.reach.w, grass.reach.w * 0.67, away);
    let rise = (at.y - middle.y) * kept;

    // The wind bends the tips the most: down the wind in a gust, and
    // back a little past upright as it eases, each tuft at its own
    // moment. A bent tip comes down a little, as its blade's length is
    // kept.
    let strong = strength(wind, middle.xz);
    let t = grass.sway.y;
    let phase = dot(middle.xz, vec2<f32>(0.37, 0.23)) + own * 1.7;
    let gust = 1.5 * strong - 0.55 + 0.25 * sin(t * 2.1 + phase);
    let flow = vec2<f32>(wind.flow.x, wind.flow.y);
    let push = grass.sway.x * gust * up * up * rise;
    let sink = 0.5 * push * push / max(rise, 0.05);
    at = vec3<f32>(at.x + flow.x * push, ground + rise - sink, at.z + flow.y * push);

    // The ground's colour where the tuft stands, darker at the roots and
    // lighter and sunnier to the tips.
    let here = page_at(0, middle.xz);
    var colour = grass_colour(middle.xz, middle.y, here.y, here.z) * tone
        * mix(grass.look.x, grass.look.y, up);
    colour = mix(colour, soil.grass_sun.rgb * grass.look.y, grass.look.z * up * up);
    colour *= 1.0 + 0.15 * max(gust, 0.0) * up;
    if flower {
        let bed = vec2<i32>(floor(middle.xz / 9.0));
        let k = u32(bed.x * 73 + bed.y * 19 + 100000) % 5u;
        colour = grass.flowers[k].rgb * tone;
    }

    var drawn: Blade;
    drawn.position = at;
    drawn.normal = normalize(land.normal);
    drawn.colour = colour;
    return drawn;
}
