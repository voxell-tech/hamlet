// The pass that lists the tufts to draw. Each invocation looks at one square
// of the map about the eye: if a tuft grows there, is kept that far off and
// is in sight, it is listed under its shape and the detail its distance
// calls for, and its list's draw is counted one more. A second step keeps
// each count within what its list holds.

import hamlet_vfx::grass_shape::{
    DETAILS, SHAPES, away_from_eye, fade, grass, grows, rank_of, shape_of,
    spot, tufts_kept,
};

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

// `PlaceInput` in `grass/draw.rs`: the square, in tufts, the first
// invocation looks at, how many each way there are, and the planes the eye
// sees between.
struct Place {
    origin: vec2<i32>,
    size: vec2<u32>,
    sight: array<vec4<f32>, 6>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(120) var<uniform> place_at: Place;
@group(constants::MATERIAL_BIND_GROUP) @binding(121) var<storage, read_write> tufts: array<vec2<i32>>;
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

@compute @workgroup_size(8, 8, 1)
fn place(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= place_at.size) {
        return;
    }
    let cell = place_at.origin + vec2<i32>(id.xy);
    let s = spot(cell);
    if distance(s.p, grass.reach.xz) > grass.reach.w || !grows(s) {
        return;
    }
    let away = away_from_eye(s.p);
    let kept = tufts_kept(away);
    if fade(rank_of(s.h), kept) <= 0.0 {
        return;
    }
    let reach = TUFT_REACH * inverseSqrt(clamp(kept, 0.05, 1.0)) + TUFT_MORE;
    if !in_sight(vec3<f32>(s.p.x, s.y + 0.3, s.p.y), reach) {
        return;
    }
    let detail = select(select(2u, 1u, away < grass.rings.y), 0u, away < grass.rings.x);
    let list = detail * SHAPES + shape_of(s.h);
    let at = atomicAdd(&draws[list * ARGUMENTS + 1u], 1u);
    if at < CAPACITY {
        tufts[list * CAPACITY + at] = cell;
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
