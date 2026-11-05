// A tuft of grass, the same in the pass that lists the tufts and in their
// draw. The map is cut into squares `spacing` across, a tuft in each; from
// its square alone it is found where in it the tuft stands, how it is
// scaled and turned, whether it grows there at all, its rank and shape,
// and whether it flowers. Each of its points is set on the land under it
// as the finest page has it, shrunk toward the edge of the eye's reach, and
// bent by the wind, which rolls over the field in the wind's own bands.

import hamlet_vfx::ground_shape::{land_at, page_at};
// `smooth_step`, not WGSL's `smoothstep`, wherever a share falls: WGSL
// leaves a falling `smoothstep` undefined, and some GPUs give it wrong.
import hamlet_vfx::noise::{key, scramble, smooth_step, swell};
import hamlet_vfx::soil::{
    BLOOMS, bloom_colour, blooms_at, grass_colour, grass_likely, heads_shown, soil,
};
import hamlet_vfx::wind::{Wind, blowing, overall, strength};

// `GrassUniform` in `grass.rs`; the settings are explained on
// `GrassSettings` there.
struct Grass {
    // x: `spacing`. y: `bare`. z: `flowers`. w: `shorten`.
    grow: vec4<f32>,
    // xy: `across`. zw: `up`.
    size: vec4<f32>,
    // x: `root`. y: `tip`. z: `sun`. w: `sheen`.
    shade: vec4<f32>,
    // x: `bend`. y: the time in seconds.
    sway: vec4<f32>,
    // x: `glow`, as far into the night as it is. y: the breadth of its
    // patches. z: how fast they drift. w: `halo`.
    glow: vec4<f32>,
    // The share of tufts kept where the middle detail begins, and where
    // the farthest does; then the same for each tuft's blades.
    thin: vec4<f32>,
    // xyz: the eye. w: how far from it the grass is gone, shortening
    // over its last `shorten` metres.
    reach: vec4<f32>,
    // x: where the middle detail begins. y: where the farthest does. z:
    // how high the eye stands over the ground under it. w: the share of
    // `thin`'s tufts the world keeps.
    rings: vec4<f32>,
    // xy: the mask's corner on the map. z: its side.
    mask_at: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> grass: Grass;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var<uniform> wind: Wind;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var mask: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(107) var mask_sampler: sampler;

const TAU: f32 = 6.2831853;

// A point of a tuft as it is drawn: where, the ground's normal under it,
// its colour, and the light it gives off.
struct Blade {
    position: vec3<f32>,
    normal: vec3<f32>,
    colour: vec3<f32>,
    glow: vec3<f32>,
}

// A number from 0 to 1 of the tuft whose hash is `h`, one of many by
// `salt`.
fn roll(h: u32, salt: u32) -> f32 {
    return f32(scramble(h ^ (salt * 0x9e3779b9u)) >> 8u) / 16777216.0;
}

// How brightly a flower at `p` on the map glows: as far into the night as
// it is, swelling and fading in patches that drift and change.
fn glow_at(p: vec2<f32>) -> f32 {
    return grass.glow.x * swell(p, grass.glow.y, grass.sway.y * grass.glow.z);
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

// Shapes of tuft, how many of the last are flowers, and details each is
// made in. `SHAPES`, `FLOWERS` and `DETAILS` in `grass.rs`.
const SHAPES: u32 = 8u;
const FLOWERS: u32 = 2u;
const DETAILS: u32 = 3u;
// How much of `flower_heart` a flower's heart takes over its petals' colour.
const HEART_SHARE: f32 = 0.6;
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

// The square of the map a tuft stands in, and where in it: its hash, its
// middle on the map, the land's height, woods and heath there, and the
// ground's normal under it.
struct Spot {
    h: u32,
    p: vec2<f32>,
    y: f32,
    woods: f32,
    heath: f32,
    normal: vec3<f32>,
}

// The hash of the tuft of the square `cell` of the map, counted in tufts.
fn tuft_hash(cell: vec2<i32>) -> u32 {
    return key(soil.seed ^ 0x6a09e667u, cell.x, cell.y);
}

// The middle on the map of that tuft, whose hash is `h`.
fn tuft_middle(cell: vec2<i32>, h: u32) -> vec2<f32> {
    return (vec2<f32>(cell) + vec2<f32>(roll(h, 0u), roll(h, 1u))) * grass.grow.x;
}

// The tuft of the square `cell` of the map, counted in tufts.
fn spot(cell: vec2<i32>) -> Spot {
    let h = tuft_hash(cell);
    let p = tuft_middle(cell, h);
    let here = land_at(0, p);
    return Spot(h, p, here.page.x, here.page.y, here.page.z, normalize(here.normal));
}

// Whether the tuft at `s` grows: none in or by the water, on steep or high
// ground, on trodden ground, nor in some places at random, and fewer in the
// woods. One that does grows whole, right up to the trodden ground's edge.
fn grows(s: Spot) -> bool {
    let likely = grass_likely(s.y, s.normal.y, s.woods);
    return roll(s.h, 2u) >= grass.grow.y && roll(s.h, 3u) <= likely && trodden(s.p) <= 0.5;
}

// How far off the tuft at `p` on the map is: across the map, and the
// eye's height over the ground.
fn away_from_eye(p: vec2<f32>) -> f32 {
    return length(vec2<f32>(distance(p, grass.reach.xz), grass.rings.z));
}

// Whether the tuft whose hash is `h`, at `p` on the map with `woods` the
// share of the woods there, is a flower: they grow in patches, fewer in
// the woods.
fn in_flower(h: u32, p: vec2<f32>, woods: f32) -> bool {
    return blooms_at(p) > BLOOMS && roll(h, 4u) < grass.grow.z * (1.0 - woods);
}

// The tuft's rank, from 0 to 1, which decides how far off it is kept.
fn rank_of(h: u32) -> f32 {
    return roll(h, 10u);
}

// The tuft's shape, of `SHAPES`: one of the last `FLOWERS` where it is a
// flower, else one of the others.
fn shape_of(s: Spot) -> u32 {
    let pick = roll(s.h, 11u);
    if in_flower(s.h, s.p, s.woods) {
        return SHAPES - FLOWERS + min(u32(pick * f32(FLOWERS)), FLOWERS - 1u);
    }
    let grasses = SHAPES - FLOWERS;
    return min(u32(pick * f32(grasses)), grasses - 1u);
}

// The share of tufts kept `away` from the eye.
fn tufts_kept(away: f32) -> f32 {
    return kept_at(away, grass.thin.x * grass.rings.w, grass.thin.y * grass.rings.w);
}

// What a kept tuft is drawn with, the same for each of its points, so the
// pass that lists it finds it once. `TUFT_BYTES` in `grass/draw.rs`.
struct Tuft {
    // xy: its middle on the map. z: the land's height there. w: how much
    // of a flower's heads it shows.
    middle: vec4<f32>,
    // x: how far across and y how tall it is scaled. zw: the cosine and
    // sine of its turn.
    shape: vec4<f32>,
    // x: how much of it is kept. y: the share of its blades kept. z: how
    // far it spreads and w how much its blades widen, to cover for those
    // not kept.
    kept: vec4<f32>,
    // x: how hard the wind blows on it. y: how brightly its flowers glow.
    // z: how near the middle of a patch of flowers it is. w: how much of
    // its height it keeps, shrunk far off.
    sway: vec4<f32>,
    // xyz: the ground's normal under it. w: 1 where it is a flower.
    normal: vec4<f32>,
    // The ground's grass colour where it stands, linear.
    grass: vec4<f32>,
    // Its patch's flowers' colour, linear.
    bloom: vec4<f32>,
}

// The tuft at `s`, which the pass keeps.
fn tuft_at(s: Spot) -> Tuft {
    let p = s.p;
    let h = s.h;
    let away = away_from_eye(p);

    // Further off, fewer tufts and fewer of each one's blades are kept, the
    // lowest ranked longest, and the rest shrink away; those kept spread and
    // widen to cover as much, so the field thins without a seam.
    let tufts = tufts_kept(away);
    let blades = kept_at(away, grass.thin.z, grass.thin.w);
    let kept_tuft = fade(rank_of(h), tufts);

    // Scaled and turned as its own; scaled more across than up.
    let across = grass.size.x * pow(grass.size.y / grass.size.x, roll(h, 6u));
    let r = roll(h, 7u);
    let tall = (grass.size.z + (grass.size.w - grass.size.z) * r * r) * (1.0 - 0.4 * s.woods);
    let yaw = roll(h, 8u) * TAU;

    let shine = select(0.0, glow_at(p), grass.glow.x > 0.0);
    let about = smooth_step(BLOOMS - 0.15, BLOOMS + 0.1, blooms_at(p));
    let flowering = select(0.0, 1.0, in_flower(h, p, s.woods));
    return Tuft(
        vec4<f32>(p, s.y, heads_shown(away, grass.reach.w)),
        vec4<f32>(across, tall, cos(yaw), sin(yaw)),
        vec4<f32>(
            kept_tuft,
            blades,
            inverseSqrt(clamp(tufts, 0.05, 1.0)),
            inverseSqrt(clamp(blades, 0.05, 1.0)),
        ),
        vec4<f32>(
            strength(wind, p, grass.sway.y),
            shine,
            about,
            smooth_step(grass.reach.w, grass.reach.w - grass.grow.w, away) * kept_tuft,
        ),
        vec4<f32>(s.normal, flowering),
        vec4<f32>(grass_colour(p, s.y, s.woods, s.heath), 0.0),
        vec4<f32>(bloom_colour(p), 0.0),
    );
}

// The point `local` of `tuft`, `off` across its blade's middle line, or
// for a flower's head, as `flower` says it is, out of the head's middle;
// `up` is how far up its blade the point is, from 0 to 1, and `rank` its
// blade's rank.
fn blade(
    local: vec3<f32>,
    up: f32,
    rank: f32,
    off: vec3<f32>,
    flower: bool,
    tuft: Tuft,
) -> Blade {
    let p = tuft.middle.xy;
    let y = tuft.middle.z;
    let middle = vec3<f32>(p.x, y, p.y);
    let flowering = tuft.normal.w > 0.5;
    let kept_tuft = tuft.kept.x;
    let kept_blade = fade(rank, tuft.kept.y);

    var drawn: Blade;
    drawn.normal = tuft.normal.xyz;
    drawn.glow = vec3<f32>(0.0);

    // Close by, a flower shows its heads. Further off, where a head would
    // be smaller than a pixel and flicker, the heads shrink away and its
    // tips take the flowers' colour instead.
    let heads = tuft.middle.w;
    var point = vec3<f32>(local.x * tuft.kept.z, local.y * kept_blade, local.z * tuft.kept.z);
    if flower {
        point += off * select(0.0, heads, flowering) * kept_blade;
    } else {
        point += off * tuft.kept.w * kept_blade * kept_tuft;
    }

    let across = tuft.shape.x;
    let c = tuft.shape.z;
    let s = tuft.shape.w;
    var at = middle + vec3<f32>(
        (point.x * c + point.z * s) * across,
        point.y * tuft.shape.y,
        (point.z * c - point.x * s) * across,
    );

    // How high over the ground the point stands, kept over the ground
    // under it, and shrunk far off.
    let ground = page_at(0, at.xz).x;
    let rise = (at.y - y) * tuft.sway.w;

    // The wind bends the tips the most: down the wind in a gust, and
    // back a little past upright as it eases, each tuft at its own
    // moment, all of it as much more as the whole wind blows harder. A
    // bent tip comes down a little, as its blade's length is kept.
    let t = grass.sway.y;
    let phase = dot(p, vec2<f32>(0.37, 0.23)) + rank * 1.7;
    let gust = 1.5 * tuft.sway.x - overall(wind, t) * (0.55 - 0.25 * sin(t * 2.1 + phase));
    let flow = blowing(wind, t);
    let push = grass.sway.x * gust * up * up * rise;
    let sink = 0.5 * push * push / max(rise, 0.05);
    at = vec3<f32>(at.x + flow.x * push, ground + rise - sink, at.z + flow.y * push);

    // The ground's colour where the tuft stands, darker at the roots and
    // lighter and sunnier to the tips, each blade a little lighter or
    // darker, greener or yellower, as its rank falls. Each patch of
    // flowers is one colour.
    let warm = fract(rank * 7.31) - 0.5;
    let tone = (0.88 + 0.24 * fract(rank * 13.7)) * vec3<f32>(1.0 + 0.12 * warm, 1.0, 1.0 - 0.25 * warm);
    let bloom = tuft.bloom.rgb;
    let shine = tuft.sway.y;
    var colour: vec3<f32>;
    if flower {
        // The head's middle, which alone lies at no offset, is its heart.
        let heart = select(0.0, HEART_SHARE, dot(off, off) == 0.0);
        colour = mix(bloom, soil.flower_heart.rgb, heart);
        drawn.glow = colour * shine;
    } else {
        colour = tuft.grass.rgb * tone * mix(grass.shade.x, grass.shade.y, up);
        colour = mix(colour, soil.grass_sun.rgb * grass.shade.y, grass.shade.z * up * up);
        colour *= 1.0 + 0.15 * max(gust, 0.0) * up;
        if flowering {
            let tinted = (1.0 - heads) * up * up;
            colour = mix(colour, bloom, 0.7 * tinted);
            drawn.glow = bloom * shine * tinted;
        }
        // The grass in and about a patch of flowers glows faintly in the
        // flowers' colour, the more toward their middle and up the blade.
        drawn.glow += bloom * shine * grass.glow.w * tuft.sway.z * (0.3 + 0.7 * up);
    }

    drawn.position = at;
    drawn.colour = colour;
    return drawn;
}
