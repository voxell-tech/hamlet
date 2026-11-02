// A dispatch that does nothing, so the pass around it runs and Metal
// writes its timestamps.
@compute @workgroup_size(1)
fn main() {}
