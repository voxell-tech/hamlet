// A point of a tuft of grass, the same in its own pass and in the
// prepass. Its square lays only slots, one to a square `spacing` across
// of the map; from that square alone it is found where in it the tuft
// stands, how it is scaled and turned, whether it grows there at all and
// whether it flowers. Then it is set on the land under it as the finest
// page has it, shrunk where the ground is trodden and toward the edge of
// the eye's reach, and bent by the wind, which rolls over the field in the
// wind's own bands.

import hamlet_vfx::ground_shape::{land_at, page_at, pages_at};
// `smooth_step`, not WGSL's `smoothstep`, wherever a share falls: WGSL
// leaves a falling `smoothstep` undefined, and some GPUs give it wrong.
import hamlet_vfx::noise::{fbm, key, perlin, scramble, smooth_step};
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
    // of its patches. z: how fast they drift. w: `halo`.
    glow: vec4<f32>,
    // The share of tufts kept where the middle detail begins, and where
    // the farthest does; then the same for each tuft's blades.
    thin: vec4<f32>,
    // xyz: the eye. w: how far from it the grass is gone, shortening
    // over the last half of the way.
    reach: vec4<f32>,
    // x: where the middle detail begins. y: where the farthest does. z:
    // how high the eye stands over the ground under it.
    rings: vec4<f32>,
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
// which it is lit by, its colour, and the light it gives off.
struct Blade {
    position: vec3<f32>,
    normal: vec3<f32>,
    colour: vec3<f32>,
    glow: vec3<f32>,
}

// Where the field of flowers is over this, tufts flower.
const BLOOMS: f32 = 0.2;

// The field the flowers grow in, in about -1..1.
fn blooms_at(p: vec2<f32>) -> f32 {
    return fbm(p * 0.08 + vec2<f32>(31.0, -17.0), 2u, soil.seed + 91u);
}

// The colour of the flowers at `p` on the map: one to a patch, from broad
// noise much larger than a patch.
fn bloom_colour(p: vec2<f32>) -> vec3<f32> {
    let n = fbm(p * 0.02 + vec2<f32>(7.0, 3.0), 2u, soil.seed + 92u);
    return grass.flowers[u32(clamp(n * 1.6 + 0.5, 0.0, 0.999) * 5.0)].rgb;
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

// How much of a whole slot a tuft's rank takes in its slot's x. `RANKED`
// in `grass.rs`.
const RANKED: f32 = 0.98;
// How far below the share kept a rank begins to shrink away.
const FADING: f32 = 0.08;

// The share of tufts, or of a tuft's blades, kept `d` from the eye: all
// near it, `at_middle` where the middle detail begins and `at_far` where
// the farthest does, as the squares of those details hold. Near the eye it
// is a little over all, so that none is shrinking there.
fn kept_at(d: f32, at_middle: f32, at_far: f32) -> f32 {
    let near = grass.rings.x;
    if d < near {
        return mix(1.0 + FADING, at_middle, smooth_step(near * 0.6, near, d));
    }
    return mix(at_middle, at_far, smooth_step(near, grass.rings.y, d));
}

// How much of itself something ranked `rank` keeps where `share` of its
// kind are kept: all of it well under, and nothing over.
fn fade(rank: f32, share: f32) -> f32 {
    return clamp((share - rank) / FADING, 0.0, 1.0);
}

// The point `local` of the tuft in slot `slot` of a square whose corner
// `world_from_local` moves it to, `off` across its blade's middle line, or
// for a flower's head, as `flower` says it is, out of the head's middle;
// `up` is how far up its blade the point is, from 0 to 1, and `own` its
// blade's rank. The fraction of the slot's x is the tuft's rank.
fn blade(
    local: vec3<f32>,
    up: f32,
    own: f32,
    off: vec3<f32>,
    flower: bool,
    slot: vec2<f32>,
    world_from_local: mat4x4<f32>,
) -> Blade {
    // The square of the map the slot is, and where in it the tuft stands.
    let step = grass.grow.x;
    let whole = floor(slot);
    let rank = (slot.x - whole.x) / RANKED;
    let cell = vec2<i32>(round(world_from_local[3].xz / step)) + vec2<i32>(whole);
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
    drawn.glow = vec3<f32>(0.0);
    // None grow in or by the water, on steep or high ground, on trodden
    // ground, nor in some places at random, and fewer in the woods. One
    // that does not is drawn as nothing, all its points at its middle;
    // one that does grows whole, right up to the trodden ground's edge.
    let water = pages_at.water.x;
    let grows = smooth_step(water + 0.5, water + 1.0, y)
        * smooth_step(0.14, 0.07, 1.0 - n.y)
        * smooth_step(26.0, 18.0, y)
        * (1.0 - 0.7 * woods);
    if roll(h, 2u) < grass.grow.y || roll(h, 3u) > grows || trodden(p) > 0.5 {
        drawn.position = middle - vec3<f32>(0.0, 0.1, 0.0);
        drawn.colour = vec3<f32>(0.0);
        return drawn;
    }

    // Tufts flower in patches; a flower's head is drawn only on one that
    // does, and on others shrinks to nothing at its middle.
    let blooms = blooms_at(p);
    let flowering = blooms > BLOOMS && roll(h, 4u) < grass.grow.z * (1.0 - woods);
    // How far off it is, as the squares are laid by: across the map, and
    // the eye's height over the ground.
    let away = length(vec2<f32>(distance(p, grass.reach.xz), grass.rings.z));

    // Further off, fewer tufts and fewer of each one's blades are kept, the
    // lowest ranked longest, and the rest shrink away; those kept spread and
    // widen to cover as much, so the field thins without a seam.
    let tufts = kept_at(away, grass.thin.x, grass.thin.y);
    let blades = kept_at(away, grass.thin.z, grass.thin.w);
    let kept_tuft = fade(rank, tufts);
    let kept_blade = fade(own, blades);
    let spread = inverseSqrt(clamp(tufts, 0.05, 1.0));
    let widen = inverseSqrt(clamp(blades, 0.05, 1.0));

    // Close by, a tuft in flower shows its flowers' heads. Further off,
    // where a head would be smaller than a pixel and flicker, the heads
    // shrink away and its tips take the flowers' colour instead.
    let heads = smooth_step(grass.reach.w * 0.3, grass.reach.w * 0.15, away);
    var point = vec3<f32>(local.x * spread, local.y * kept_blade, local.z * spread);
    if flower {
        point += off * select(0.0, heads, flowering) * kept_blade;
    } else {
        point += off * widen * kept_blade * kept_tuft;
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
    // under it, and shrunk far off.
    let ground = page_at(0, at.xz).x;
    let kept = smooth_step(grass.reach.w, grass.reach.w * 0.5, away) * kept_tuft;
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
    // lighter and sunnier to the tips, each blade a little lighter or
    // darker, greener or yellower, as its rank falls. Each patch of
    // flowers is one colour.
    let warm = fract(own * 7.31) - 0.5;
    let tone = (0.88 + 0.24 * fract(own * 13.7)) * vec3<f32>(1.0 + 0.12 * warm, 1.0, 1.0 - 0.25 * warm);
    let bloom = bloom_colour(p);
    let shine = select(0.0, glow_at(p), grass.glow.x > 0.0);
    var colour: vec3<f32>;
    if flower {
        colour = bloom;
        drawn.glow = bloom * shine;
    } else {
        colour = grass_colour(p, y, woods, heath) * tone * mix(grass.look.x, grass.look.y, up);
        colour = mix(colour, soil.grass_sun.rgb * grass.look.y, grass.look.z * up * up);
        colour *= 1.0 + 0.15 * max(gust, 0.0) * up;
        if flowering {
            let tinted = (1.0 - heads) * up * up;
            colour = mix(colour, bloom, 0.7 * tinted);
            drawn.glow = bloom * shine * tinted;
        }
        // The grass in and about a patch of flowers glows faintly in the
        // flowers' colour, the more toward their middle and up the blade.
        let about = smooth_step(BLOOMS - 0.15, BLOOMS + 0.1, blooms);
        drawn.glow += bloom * shine * grass.glow.w * about * (0.3 + 0.7 * up);
    }

    drawn.position = at;
    drawn.colour = colour;
    return drawn;
}
