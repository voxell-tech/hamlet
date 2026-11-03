// The pass that lists the tufts to draw. Each invocation checks one square
// of the map about the eye: if a tuft grows there, is kept that far off and
// is in sight, it is listed under its shape and the detail its distance
// calls for, with what its points are drawn with, and its list's draw is
// counted one more. A second step keeps each count within what its list
// holds.

import hamlet_vfx::grass_shape::{
    DETAILS, SHAPES, Tuft, away_from_eye, fade, grass, grows, rank_of,
    shape_of, spot, tuft_at, tuft_hash, tuft_middle, tufts_kept,
};
import hamlet_vfx::soil::{GRASS_HIGHEST, GRASS_OVER_WATER, soil};

// The most tufts a list holds. `CAPACITY` in `grass/draw.rs`.
const CAPACITY: u32 = 8192u;
// Lists there are, and words in a list's draw's arguments, of which the
// second is how many tufts it draws. `LISTS` and `ARGUMENTS` in
// `grass/draw.rs`.
const LISTS: u32 = SHAPES * DETAILS;
const ARGUMENTS: u32 = 5u;
// How far a tuft reaches from its middle, in metres, as drawn whole, and
// what the wind and its height add: a tuft further than its reach outside
// a plane the eye sees between is out of sight. Kept tufts spread to cover
// for those thinned away, so further off they reach further.
const TUFT_REACH: f32 = 1.2;
const TUFT_MORE: f32 = 0.6;
// How far over the ground a tuft's middle is taken to stand.
const TUFT_RISE: f32 = 0.3;

// `PlaceInput` in `grass/draw.rs`: the square, in tufts, the first
// invocation checks, how many each way there are, and the planes the eye
// sees between.
struct Place {
    origin: vec2<i32>,
    size: vec2<u32>,
    sight: array<vec4<f32>, 6>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(120) var<uniform> place_at: Place;
@group(constants::MATERIAL_BIND_GROUP) @binding(121) var<storage, read_write> tufts: array<Tuft>;
@group(constants::MATERIAL_BIND_GROUP) @binding(122) var<storage, read_write> draws: array<atomic<u32>>;

// Whether a tuft whose middle is `at`, reaching `reach` from it, is in
// sight.
fn in_sight(at: vec3<f32>, reach: f32) -> bool {
    for (var i = 0u; i < 6u; i++) {
        let plane = place_at.sight[i];
        if dot(plane.xyz, at) + plane.w < -reach {
            return false;
        }
    }
    return true;
}

// Whether a tuft at `p` on the map, reaching `reach` from its middle, may
// be in sight at any height grass grows at, for before the land is read.
fn may_be_in_sight(p: vec2<f32>, reach: f32) -> bool {
    let low = soil.water + GRASS_OVER_WATER + TUFT_RISE;
    let high = GRASS_HIGHEST + TUFT_RISE;
    for (var i = 0u; i < 6u; i++) {
        let plane = place_at.sight[i];
        let y = select(low, high, plane.y > 0.0);
        if dot(plane.xyz, vec3<f32>(p.x, y, p.y)) + plane.w < -reach {
            return false;
        }
    }
    return true;
}

@compute @workgroup_size(8, 8, 1)
fn place(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= place_at.size) {
        return;
    }
    let cell = place_at.origin + vec2<i32>(id.xy);
    // What needs only the tuft's square first, then the land under it.
    let h = tuft_hash(cell);
    let p = tuft_middle(cell, h);
    if distance(p, grass.reach.xz) > grass.reach.w {
        return;
    }
    let away = away_from_eye(p);
    let kept = tufts_kept(away);
    if fade(rank_of(h), kept) <= 0.0 {
        return;
    }
    let reach = TUFT_REACH * inverseSqrt(clamp(kept, 0.05, 1.0)) + TUFT_MORE;
    if !may_be_in_sight(p, reach) {
        return;
    }
    let s = spot(cell);
    if !grows(s) || !in_sight(vec3<f32>(s.p.x, s.y + TUFT_RISE, s.p.y), reach) {
        return;
    }
    let detail = select(select(2u, 1u, away < grass.rings.y), 0u, away < grass.rings.x);
    let list = detail * SHAPES + shape_of(s);
    let at = atomicAdd(&draws[list * ARGUMENTS + 1u], 1u);
    if at < CAPACITY {
        tufts[list * CAPACITY + at] = tuft_at(s);
    }
}

@compute @workgroup_size(32, 1, 1)
fn cap(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= LISTS {
        return;
    }
    let count = &draws[id.x * ARGUMENTS + 1u];
    atomicStore(count, min(atomicLoad(count), CAPACITY));
}
