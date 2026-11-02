// The colour of the ground, point by point, from where it lies, its height
// and its slope: shared by the ground and the grass on it, so the grass
// grows from the ground's own colour.

import hamlet_vfx::noise::{fbm, perlin, smooth_step};

// The colours the ground is painted with, linear, and what places them.
// The fields are explained on `SoilUniform` in `ground.rs`.
struct Soil {
    grass_shade: vec4<f32>,
    grass: vec4<f32>,
    grass_sun: vec4<f32>,
    grass_high: vec4<f32>,
    rock_dark: vec4<f32>,
    rock: vec4<f32>,
    rock_warm: vec4<f32>,
    snow: vec4<f32>,
    dirt: vec4<f32>,
    dirt_shade: vec4<f32>,
    mud: vec4<f32>,
    water: f32,
    seed: u32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(103) var<uniform> soil: Soil;

// The colour of the grass at `p` on the map, `y` high, with `woods` and
// `heath` the shares of each there.
fn grass_colour(p: vec2<f32>, y: f32, woods: f32, heath: f32) -> vec3<f32> {
    // Grass in broad patches of shade and sun, flecked with sun.
    let broad = 0.5 + 0.5 * fbm(p * 0.035, 3u, soil.seed + 80u);
    let fine = 0.5 + 0.5 * perlin(p * 0.33, soil.seed + 81u);
    var c = mix(soil.grass_shade.rgb, soil.grass.rgb, smoothstep(0.3, 0.62, broad));
    c = mix(c, soil.grass_sun.rgb, 0.45 * fine * smoothstep(0.35, 0.7, broad));
    c = mix(c, soil.grass_high.rgb, heath);
    c = mix(c, soil.grass_shade.rgb * 0.9, 0.55 * woods);
    // Thinner, duller grass with height.
    return mix(c, soil.grass_high.rgb, smoothstep(10.0, 23.0, y));
}

// The colour of the ground at `p` on the map, `y` high, facing `n`, with
// `woods` and `heath` the shares of each there: the grass, then rock, then
// snow, and a sandy shore.
fn ground_colour(p: vec2<f32>, y: f32, n: vec3<f32>, woods: f32, heath: f32) -> vec3<f32> {
    var c = grass_colour(p, y, woods, heath);
    let grain = perlin(p * 0.21, soil.seed + 82u);
    let steep = 1.0 - n.y;
    let rock = max(
        smoothstep(0.1, 0.24, steep + 0.03 * grain),
        smoothstep(20.0, 34.0, y + 4.0 * grain),
    );
    let stone = mix(
        mix(soil.rock_dark.rgb, soil.rock.rgb, 0.5 + 0.5 * grain),
        soil.rock_warm.rgb,
        smoothstep(-0.3, 0.6, n.x),
    );
    c = mix(c, stone, rock);
    let snow = smoothstep(36.0, 44.0, y + 5.0 * grain) * smooth_step(0.5, 0.25, steep);
    c = mix(c, soil.snow.rgb, snow);
    // A sandy shore, wet at the waterline.
    let shore = smooth_step(soil.water + 0.75, soil.water + 0.2, y);
    c = mix(c, mix(soil.dirt.rgb, soil.dirt_shade.rgb, 0.4), shore * 0.8);
    return mix(c, soil.mud.rgb, smooth_step(soil.water + 0.2, soil.water - 0.3, y));
}
