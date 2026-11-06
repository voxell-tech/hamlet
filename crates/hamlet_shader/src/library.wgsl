// What every compiled shader shares: the values its graph is set
// to, what a surface is read from and gives, and the patterns its
// nodes draw. Plain WGSL, so the realm's PBR shader and the editor's
// swatches both take it as it is.

// Every value set on the graph, one to a slot. Slot 0 holds the time
// in seconds in x.
struct Slots {
    v: array<vec4<f32>, 256>,
}

// What a surface is read from, at one point of it.
struct SurfaceIn {
    uv: vec2<f32>,
    // In the world.
    position: vec3<f32>,
    // In the world, of length 1.
    normal: vec3<f32>,
    // Toward the camera, of length 1.
    view: vec3<f32>,
    // The colour the mesh gives, its palette colour.
    color: vec4<f32>,
    time: f32,
    // The position, the normal and the way to the camera in the
    // mesh's own space, before it is moved, turned and scaled into
    // the world.
    local_position: vec3<f32>,
    local_normal: vec3<f32>,
    local_view: vec3<f32>,
}

// What a surface gives. Colours are linear.
struct Surface {
    base_color: vec4<f32>,
    roughness: f32,
    metallic: f32,
    emission: vec3<f32>,
    normal: vec3<f32>,
    // Above 0.5, base colour plus emission is written unshaded.
    unlit: f32,
}

fn hm_hash3(p: vec3<f32>) -> vec3<f32> {
    let q = vec3<f32>(
        dot(p, vec3<f32>(127.1, 311.7, 74.7)),
        dot(p, vec3<f32>(269.5, 183.3, 246.1)),
        dot(p, vec3<f32>(113.5, 271.9, 124.6)),
    );
    return fract(sin(q) * 43758.5453123);
}

fn hm_hash1(p: vec3<f32>) -> f32 {
    return fract(sin(dot(p, vec3<f32>(12.9898, 78.233, 37.719))) * 43758.5453);
}

// The gradient at a corner of the noise's lattice, dotted with the
// way to `p`.
fn hm_corner(i: vec3<f32>, f: vec3<f32>, c: vec3<f32>) -> f32 {
    return dot(hm_hash3(i + c) * 2.0 - 1.0, f - c);
}

// Smooth gradient noise, about -1 to 1.
fn hm_noise(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let x00 = mix(hm_corner(i, f, vec3<f32>(0.0, 0.0, 0.0)), hm_corner(i, f, vec3<f32>(1.0, 0.0, 0.0)), u.x);
    let x10 = mix(hm_corner(i, f, vec3<f32>(0.0, 1.0, 0.0)), hm_corner(i, f, vec3<f32>(1.0, 1.0, 0.0)), u.x);
    let x01 = mix(hm_corner(i, f, vec3<f32>(0.0, 0.0, 1.0)), hm_corner(i, f, vec3<f32>(1.0, 0.0, 1.0)), u.x);
    let x11 = mix(hm_corner(i, f, vec3<f32>(0.0, 1.0, 1.0)), hm_corner(i, f, vec3<f32>(1.0, 1.0, 1.0)), u.x);
    return mix(mix(x00, x10, u.y), mix(x01, x11, u.y), u.z);
}

// Noise in layers, each twice as fine and `roughness` as strong as
// the one before, from 0 to 1.
fn hm_fbm(p: vec3<f32>, detail: f32, roughness: f32) -> f32 {
    var sum = 0.0;
    var amp = 1.0;
    var total = 0.0;
    var q = p;
    for (var layer = 0; layer < 8; layer++) {
        if f32(layer) >= max(detail, 1.0) {
            break;
        }
        sum += amp * hm_noise(q);
        total += amp;
        amp *= roughness;
        q *= 2.0;
    }
    return clamp(sum / total * 0.75 + 0.5, 0.0, 1.0);
}

// The distance to the nearest of points scattered one to a cell, in
// x, and a colour picked for that cell, in yzw.
fn hm_voronoi(p: vec3<f32>, randomness: f32) -> vec4<f32> {
    let i = floor(p);
    let f = fract(p);
    var best = 8.0;
    var cell = vec3<f32>(0.0);
    for (var x = -1; x <= 1; x++) {
        for (var y = -1; y <= 1; y++) {
            for (var z = -1; z <= 1; z++) {
                let c = vec3<f32>(f32(x), f32(y), f32(z));
                let at = c + hm_hash3(i + c) * randomness;
                let d = length(at - f);
                if d < best {
                    best = d;
                    cell = hm_hash3(i + c + 17.0);
                }
            }
        }
    }
    return vec4<f32>(best, cell);
}

// 1 on every other cube, 0 between.
fn hm_checker(p: vec3<f32>) -> f32 {
    let c = floor(p);
    let s = c.x + c.y + c.z;
    return s - 2.0 * floor(s * 0.5);
}

// Bricks laid in rows of `height`, each `width` long and every other
// row moved along by `offset` of one: 1 on a brick, falling to 0 in
// the mortar `mortar` wide, in x, and a number picked for each brick,
// in y.
fn hm_brick(p: vec2<f32>, width: f32, height: f32, mortar: f32, offset: f32) -> vec2<f32> {
    let size = vec2<f32>(max(width, 1e-3), max(height, 1e-3));
    let row = floor(p.y / size.y);
    let shifted = vec2<f32>(p.x + offset * size.x * (row - 2.0 * floor(row * 0.5)), p.y);
    let cell = floor(shifted / size);
    let f = shifted - cell * size;
    let edge = min(min(f.x, size.x - f.x), min(f.y, size.y - f.y));
    let half = mortar * 0.5;
    let brick = smoothstep(half, half + max(mortar * 0.25, 1e-3), edge);
    return vec2<f32>(brick, hm_hash1(vec3<f32>(cell, 3.0)));
}

fn hm_rgb_to_hsv(c: vec3<f32>) -> vec3<f32> {
    let k = vec4<f32>(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);
    let p = mix(vec4<f32>(c.bg, k.wz), vec4<f32>(c.gb, k.xy), step(c.b, c.g));
    let q = mix(vec4<f32>(p.xyw, c.r), vec4<f32>(c.r, p.yzx), step(p.x, c.r));
    let d = q.x - min(q.w, q.y);
    let e = 1.0e-10;
    return vec3<f32>(abs(q.z + (q.w - q.y) / (6.0 * d + e)), d / (q.x + e), q.x);
}

fn hm_hsv_to_rgb(c: vec3<f32>) -> vec3<f32> {
    let k = vec4<f32>(1.0, 2.0 / 3.0, 1.0 / 3.0, 3.0);
    let p = abs(fract(c.xxx + k.xyz) * 6.0 - k.www);
    return c.z * mix(k.xxx, clamp(p - k.xxx, vec3<f32>(0.0), vec3<f32>(1.0)), c.y);
}

// `layer` over `base`, each way a paint program blends them.
fn hm_overlay(base: vec3<f32>, layer: vec3<f32>) -> vec3<f32> {
    let low = 2.0 * base * layer;
    let high = 1.0 - 2.0 * (1.0 - base) * (1.0 - layer);
    return mix(low, high, step(vec3<f32>(0.5), base));
}

// A value as a swatch shows it.
fn hm_show1(v: f32) -> vec4<f32> {
    return vec4<f32>(vec3<f32>(v), 1.0);
}

fn hm_show2(v: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(v, 0.0, 1.0);
}

fn hm_show3(v: vec3<f32>) -> vec4<f32> {
    return vec4<f32>(v, 1.0);
}

fn hm_show4(v: vec4<f32>) -> vec4<f32> {
    return v;
}
