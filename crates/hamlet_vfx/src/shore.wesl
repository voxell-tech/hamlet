// The land's pages as what floats on the water reads them: the height of
// the ground and the distance out from the shore about a point, and how
// the shore bears on the waves there.

import hamlet_vfx::ground_shape::Pages;
import hamlet_vfx::waves::{SHORE_REACH, Coast, coast};

@group(constants::MATERIAL_BIND_GROUP) @binding(115) var pages: texture_2d_array<u32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(116) var<uniform> pages_at: Pages;

// The four points of the land's pages about a point on the map, each the
// height and the distance out from the shore, and where between them the
// point lies.
struct PageCell {
    a: vec2<f32>,
    b: vec2<f32>,
    c: vec2<f32>,
    d: vec2<f32>,
    f: vec2<f32>,
}

// The height and the distance out from the shore at the texel `c` of the
// page of size `level`, as `relief.wesl` packs them.
fn page_texel(level: i32, c: vec2<i32>) -> vec2<f32> {
    let t = textureLoad(pages, c, level, 0).xy;
    return vec2<f32>(bitcast<f32>(t.x), (f32(t.y >> 16u) / 65535.0 * 2.0 - 1.0) * SHORE_REACH);
}

// The points about `p` on the finest of the land's pages from `least` up
// that reaches it, as `page_at` in `ground_shape.wesl` finds them. False
// until the pages are made.
fn page_cell(p: vec2<f32>, least: i32, cell: ptr<function, PageCell>) -> bool {
    let size = vec2<i32>(textureDimensions(pages)) - 2;
    if any(size < vec2<i32>(1)) {
        return false;
    }
    var level = least;
    var g = vec2<f32>(0.0);
    loop {
        let at = pages_at.at[level];
        g = (p - at.xy) / at.z;
        if level >= 7 || (all(g >= vec2<f32>(0.0)) && all(g < vec2<f32>(size))) {
            break;
        }
        level += 1;
    }
    let c = clamp(vec2<i32>(floor(g)), vec2<i32>(0), size);
    *cell = PageCell(
        page_texel(level, c),
        page_texel(level, c + vec2<i32>(1, 0)),
        page_texel(level, c + vec2<i32>(0, 1)),
        page_texel(level, c + vec2<i32>(1, 1)),
        fract(g),
    );
    return true;
}

// How the shore bears on the waves at the point `p` on the map, from the
// distance out from it the land's pages hold, smoothly between their points
// and from the finest that reaches it, the way to it falling across them;
// `bend` and `calm` are the water's. Open sea until the pages are made.
fn coast_at(p: vec2<f32>, bend: f32, calm: vec2<f32>) -> Coast {
    var cell: PageCell;
    if !page_cell(p, 0, &cell) {
        return coast(SHORE_REACH, vec2<f32>(0.0), bend, calm);
    }
    let f = cell.f;
    let d = mix(mix(cell.a.y, cell.b.y, f.x), mix(cell.c.y, cell.d.y, f.x), f.y);
    let rise = vec2<f32>(mix(cell.b.y - cell.a.y, cell.d.y - cell.c.y, f.y), mix(cell.c.y - cell.a.y, cell.d.y - cell.b.y, f.x));
    return coast(d, -rise, bend, calm);
}
