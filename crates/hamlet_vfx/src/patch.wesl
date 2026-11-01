// Where a point of a patch lies, as `patch.rs` lays the patches: the
// patch's grid scaled by its side and moved to its corner, each other
// point sliding onto the grid of the patch twice its size as it nears
// the distance at which that larger patch would be drawn instead. So a
// patch meets a larger one with no seam, and none pops as it splits.

// `QUADS` and `SPLIT` in `patch.rs`.
const QUADS: f32 = 32.0;
const SPLIT: f32 = 3.0;

// The share of the way to that distance at which a point begins to slide.
// No point where a patch meets a smaller one may have begun: those lie
// within `SPLIT` of their own side plus their diagonal of the eye, so this
// is at least 1/2 + 1/(SPLIT * sqrt(2)).
const SLIDE_FROM: f32 = 0.875;

// Where on the map the point `local` of a patch lies, from 0 to 1 over its
// side from its corner of least x and z, for a patch at `corner`, `side`
// across, on the plane `y` high, seen from `eye`.
fn slid(local: vec2<f32>, corner: vec2<f32>, side: f32, y: f32, eye: vec3<f32>) -> vec2<f32> {
    return sliding(local, corner, side, y, eye).xy;
}

// As `slid`, and how far the point has slid, from 0 on its own patch's grid
// to 1 on the larger patch's.
fn sliding(local: vec2<f32>, corner: vec2<f32>, side: f32, y: f32, eye: vec3<f32>) -> vec3<f32> {
    let grid = local * QUADS;
    let at = corner + local * side;
    // A larger patch is drawn beyond this distance from the eye; a point
    // there must already lie on its grid.
    let larger = SPLIT * side * 2.0;
    let d = distance(eye, vec3<f32>(at.x, y, at.y));
    let slide = clamp((d - larger * SLIDE_FROM) / (larger * (1.0 - SLIDE_FROM)), 0.0, 1.0);
    let onto = grid - fract(grid * 0.5) * 2.0 * slide;
    return vec3<f32>(corner + onto / QUADS * side, slide);
}
