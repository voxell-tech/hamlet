// A blade of grass: rooted where its patch lays it, drawn in toward the
// middle of the clump it grows in and splayed out from there, on the land
// as the finest page has it, grown as tall as the ground there lets it and
// bent by the wind. A blade that may not grow is drawn as nothing, all its
// points at its root.

import hamlet_vfx::ground_shape::{land_at, pages_at};
import hamlet_vfx::noise::{fbm, perlin};
import hamlet_vfx::soil::{soil, grass_colour};
import hamlet_vfx::wind::{Wind, strength};

// `GrassUniform` in `grass.rs`; the settings are explained on
// `GrassSettings` there.
struct Grass {
    // x and y: `tall`. z: `wide`. w: `lean`.
    shape: vec4<f32>,
    // x: `root`. y: `tip`. z: `sun`. w: `glow`.
    look: vec4<f32>,
    // x: `bend`. y: the time in seconds. z: `flowers`.
    sway: vec4<f32>,
    // x: `clump`. y: `gather`. z: `splay`.
    clumps: vec4<f32>,
    // xyz: the eye. w: how far from it the blades are gone, shortening
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

// A point of a blade as it is drawn.
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

fn hash2(p: vec2<f32>) -> vec2<f32> {
    let q = vec2<f32>(dot(p, vec2<f32>(127.1, 311.7)), dot(p, vec2<f32>(269.5, 183.3)));
    return fract(sin(q) * 43758.5453);
}

// The clump nearest `p`: its middle on the map, and a number of its own
// from 0 to 1.
struct Clump {
    middle: vec2<f32>,
    own: f32,
}

fn clump_at(p: vec2<f32>) -> Clump {
    let size = grass.clumps.x;
    let cell = floor(p / size);
    var best = Clump(p, 0.0);
    var nearest = 1e9;
    for (var x = -1; x <= 1; x++) {
        for (var z = -1; z <= 1; z++) {
            let c = cell + vec2<f32>(f32(x), f32(z));
            let r = hash2(c);
            let middle = (c + 0.15 + 0.7 * r) * size;
            let d = distance(middle, p);
            if d < nearest {
                nearest = d;
                best = Clump(middle, fract(r.x * 7.31 + r.y * 3.17));
            }
        }
    }
    return best;
}

// The point `local` of a blade of a patch moved by `world_from_local`, on
// its `side` from -1 to 1, the blade's own four numbers being `own`. The
// patch's scale across is how much wider its blades are drawn, where it
// has fewer of them.
fn blade(local: vec3<f32>, side: f32, own: vec4<f32>, world_from_local: mat4x4<f32>) -> Blade {
    let laid = world_from_local[3].xz + local.xz;
    let clump = clump_at(laid);
    let root = mix(laid, clump.middle, grass.clumps.y);
    let out_of = laid - clump.middle;
    let up = local.y;
    let land = land_at(0, root);
    let y = land.page.x;
    let woods = land.page.y;
    let heath = land.page.z;
    let n = normalize(land.normal);

    // None in the water or on its shore, on rock, high up or trodden;
    // fewer and shorter in the woods; and shorter to the field's edge.
    let water = pages_at.water.x;
    let away = distance(vec3<f32>(root.x, y, root.y), grass.reach.xyz);
    var grows = smoothstep(water + 0.55, water + 1.0, y)
        * smoothstep(0.86, 0.93, n.y)
        * smoothstep(24.0, 17.0, y)
        * (1.0 - trodden(root))
        * smoothstep(grass.reach.w, grass.reach.w * 0.65, away);
    let kept = mix(1.0, 0.35, woods);
    if own.x > kept {
        grows = 0.0;
    }
    // Each clump is its own height, and taller in patches; where the
    // grass thins at an edge, it grows shorter first.
    let tufts = 0.5 + 0.5 * perlin(root * 0.09, soil.seed + 90u);
    let height = mix(grass.shape.x, grass.shape.y, 0.35 * own.y + 0.65 * clump.own)
        * (0.65 + 0.7 * tufts)
        * mix(1.0, 0.6, woods)
        * smoothstep(0.0, 0.35 + 0.65 * own.w, grows);

    var drawn: Blade;
    drawn.normal = n;
    if height < 0.004 {
        drawn.position = vec3<f32>(root.x, y - 0.05, root.y);
        drawn.colour = vec3<f32>(0.0);
        return drawn;
    }

    // The flowers grow in patches of one colour each.
    let blooms = smoothstep(0.58, 0.7, fbm(root * 0.08 + vec2<f32>(31.0, -17.0), 2u, soil.seed + 91u));
    let flower = own.x < blooms * grass.sway.z * (1.0 - woods);

    // It splays out from its clump's middle, leans a little of its own,
    // and the wind pushes it, rolling over the field in the wind's bands.
    let outward = select(vec2<f32>(1.0, 0.0), normalize(out_of), length(out_of) > 1e-4);
    let splay = vec3<f32>(outward.x, 0.0, outward.y) * grass.clumps.z;
    let turn = own.z * TAU;
    let lean = vec3<f32>(cos(turn), 0.0, sin(turn)) * (own.w * 2.0 - 1.0) * grass.shape.w;
    // Its flat face mostly turned across the way it splays.
    let across = normalize(vec3<f32>(-outward.y, 0.0, outward.x) + lean * 0.8);
    let strong = strength(wind, root);
    let t = grass.sway.y;
    let flutter = 0.8 + 0.2 * sin(t * 2.3 + dot(root, vec2<f32>(0.7, 0.5)) + own.w * 6.0);
    let flow = vec3<f32>(wind.flow.x, 0.0, wind.flow.y) * grass.sway.x * strong * flutter;
    let bend = (splay + lean + flow) * up * up;
    // A bent blade comes down a little, as its length is kept.
    let sink = 0.5 * dot(bend, bend);
    var at = vec3<f32>(root.x, y - 0.02, root.y)
        + (vec3<f32>(0.0, up - sink, 0.0) + bend) * height;
    var wide = grass.shape.z * length(world_from_local[0].xyz) * (0.7 + 0.6 * own.y) * (1.0 - 0.85 * up);
    if flower && up > 0.6 {
        wide = max(wide, 0.06);
    }
    at += across * side * wide * 0.5;
    drawn.position = at;
    // Lit as the ground is, a little round across the blade and turned
    // with its bend, so the field takes the light as one.
    drawn.normal = normalize(n + across * side * 0.25 + bend * 0.6);

    // The ground's colour at the root, lighter and sunnier to the tip,
    // each clump and blade a little its own, and lighter as the wind
    // bends it.
    let base = grass_colour(root, y, woods, heath);
    var colour = base * mix(grass.look.x, grass.look.y, up)
        * (0.82 + 0.24 * clump.own + 0.12 * own.w);
    colour = mix(colour, soil.grass_sun.rgb * grass.look.y, grass.look.z * up * up * (0.5 + clump.own));
    colour *= 1.0 + 0.2 * strong * up;
    if flower && up > 0.6 {
        let bed = vec2<i32>(floor(root / 9.0));
        let k = u32(bed.x * 73 + bed.y * 19 + 1000) % 5u;
        colour = grass.flowers[k].rgb;
    }
    drawn.colour = colour;
    return drawn;
}
