// A point of a tuft of grass, the same in its own pass and in the
// prepass. Its square lays only slots, one to a square `spacing` across
// of the map; from that square alone it is found where in it the tuft
// stands, how it is scaled and turned, whether it grows there at all and
// whether it flowers. Then it is set on the land under it as the finest
// page has it, shrunk where the ground is trodden and toward the edge of
// the eye's reach, and bent by the wind, which rolls over the field in the
// wind's own bands.

import hamlet_vfx::ground_shape::{land_at, page_at, pages_at};
import hamlet_vfx::noise::{fbm, key, perlin, scramble};
import hamlet_vfx::soil::{soil, grass_colour};
import hamlet_vfx::wind::{Wind, strength};

// `GrassUniform` in `grass.rs`; the settings are explained on
// `GrassSettings` there.
struct Grass {
    // x: `spacing`. y: `bare`. z: `flowers`. w: `slots`.
    grow: vec4<f32>,
    // xy: `across`. zw: `up`.
    size: vec4<f32>,
    // x: `root`. y: `tip`. z: `sun`. w: `sheen`.
    look: vec4<f32>,
    // x: `bend`. y: the time in seconds.
    sway: vec4<f32>,
    // x: `glow`, as far into the night as it is. y: one over the breadth
    // of its patches. z: how fast they drift.
    glow: vec4<f32>,
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

const TAU: f32 = 6.2831853;

// A point of a tuft as it is drawn: where, the ground's normal under it,
// which it is lit by, its colour, and how brightly it glows.
struct Blade {
    position: vec3<f32>,
    normal: vec3<f32>,
    colour: vec3<f32>,
    glow: f32,
}

// A number from 0 to 1 of the tuft whose hash is `h`, one of many by
// `salt`.
fn roll(h: u32, salt: u32) -> f32 {
    return f32(scramble(h ^ (salt * 0x9e3779b9u)) >> 8u) / 16777216.0;
}

// How brightly a flower at `p` on the map glows: as far into the night as
// it is, swelling and fading in patches that drift and change, on gradient
// noise of their own seeds, two layers drifting different ways so the
// patches change shape and do not merely slide.
fn glow_at(p: vec2<f32>) -> f32 {
    let t = grass.sway.y * grass.glow.z;
    let q = p * grass.glow.y;
    let n = perlin(q + vec2<f32>(0.6, 0.8) * t * grass.glow.y, 41u)
        + 0.5 * perlin(q * 2.1 - vec2<f32>(0.9, -0.4) * t * grass.glow.y * 1.7, 42u);
    return grass.glow.x * mix(0.25, 1.0, smoothstep(-0.45, 0.6, n / 1.5));
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

// The point `local` of the tuft in slot `slot` of a square whose corner
// `world_from_local` moves it to; `up` is how far up its blade the point
// is, from 0 to 1, `own` its blade's own number, and `tone` what its blade
// multiplies the ground's colour by, or for a flower's head, as `flower`
// says it is, how far out of the head's middle the point lies.
fn blade(
    local: vec3<f32>,
    up: f32,
    own: f32,
    tone: vec3<f32>,
    flower: bool,
    slot: vec2<f32>,
    world_from_local: mat4x4<f32>,
) -> Blade {
    // The square of the map the slot is, and where in it the tuft stands.
    let step = grass.grow.x;
    let cell = vec2<i32>(round(world_from_local[3].xz / step)) + vec2<i32>(slot);
    let h = key(soil.seed ^ 0x6a09e667u, cell.x, cell.y);
    let p = (vec2<f32>(cell) + vec2<f32>(roll(h, 0u), roll(h, 1u))) * step;
    let here = land_at(0, p);
    let y = here.page.x;
    let woods = here.page.y;
    let heath = here.page.z;
    let n = normalize(here.normal);
    let middle = vec3<f32>(p.x, y, p.y);

    var drawn: Blade;
    drawn.normal = n;
    drawn.glow = 0.0;
    // None grow in or by the water, on steep or high ground, nor in some
    // places at random, and fewer in the woods. One that does not is
    // drawn as nothing, all its points at its middle.
    let water = pages_at.water.x;
    let grows = smoothstep(water + 0.5, water + 1.0, y)
        * smoothstep(0.14, 0.07, 1.0 - n.y)
        * smoothstep(26.0, 18.0, y)
        * (1.0 - 0.7 * woods);
    if roll(h, 2u) < grass.grow.y || roll(h, 3u) > grows {
        drawn.position = middle - vec3<f32>(0.0, 0.1, 0.0);
        drawn.colour = vec3<f32>(0.0);
        return drawn;
    }

    // Tufts flower in patches; a flower's head is drawn only on one that
    // does, and on others shrinks to nothing at its middle.
    let blooms = fbm(p * 0.08 + vec2<f32>(31.0, -17.0), 2u, soil.seed + 91u);
    let flowering = blooms > 0.2 && roll(h, 4u) < grass.grow.z * (1.0 - woods);
    var point = local;
    if flower && flowering {
        point += tone;
    }

    // Scaled and turned as its own; scaled more across than up.
    let across = grass.size.x * pow(grass.size.y / grass.size.x, roll(h, 6u));
    let r = roll(h, 7u);
    let tall = (grass.size.z + (grass.size.w - grass.size.z) * r * r) * (1.0 - 0.4 * woods);
    let yaw = roll(h, 8u) * TAU;
    let c = cos(yaw);
    let s = sin(yaw);
    var at = middle + vec3<f32>(
        (point.x * c + point.z * s) * across,
        point.y * tall,
        (point.z * c - point.x * s) * across,
    );

    // How high over the ground the point stands, kept over the ground
    // under it, and shrunk where trodden and far off.
    let ground = page_at(0, at.xz).x;
    let away = distance(middle, grass.reach.xyz);
    let kept = (1.0 - trodden(at.xz))
        * smoothstep(grass.reach.w, grass.reach.w * 0.67, away);
    let rise = (at.y - y) * kept;

    // The wind bends the tips the most: down the wind in a gust, and
    // back a little past upright as it eases, each tuft at its own
    // moment. A bent tip comes down a little, as its blade's length is
    // kept.
    let strong = strength(wind, p);
    let t = grass.sway.y;
    let phase = dot(p, vec2<f32>(0.37, 0.23)) + own * 1.7;
    let gust = 1.5 * strong - 0.55 + 0.25 * sin(t * 2.1 + phase);
    let flow = vec2<f32>(wind.flow.x, wind.flow.y);
    let push = grass.sway.x * gust * up * up * rise;
    let sink = 0.5 * push * push / max(rise, 0.05);
    at = vec3<f32>(at.x + flow.x * push, ground + rise - sink, at.z + flow.y * push);

    // The ground's colour where the tuft stands, darker at the roots and
    // lighter and sunnier to the tips.
    var colour: vec3<f32>;
    if flower {
        let bed = vec2<i32>(floor(p / 9.0));
        let k = u32(bed.x * 73 + bed.y * 19 + 100000) % 5u;
        colour = grass.flowers[k].rgb;
        if grass.glow.x > 0.0 {
            drawn.glow = glow_at(p);
        }
    } else {
        colour = grass_colour(p, y, woods, heath) * tone * mix(grass.look.x, grass.look.y, up);
        colour = mix(colour, soil.grass_sun.rgb * grass.look.y, grass.look.z * up * up);
        colour *= 1.0 + 0.15 * max(gust, 0.0) * up;
    }

    drawn.position = at;
    drawn.colour = colour;
    return drawn;
}
