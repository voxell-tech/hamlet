//! Small `.glb` files, written in place of a modelling tool's.

/// The glTF of one triangle in the material `roof_tiles`, with no
/// normals, lifted by its node, and a socket at its top corner.
pub const TRIANGLE: &str = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0,1]}],"nodes":[{"mesh":0,"translation":[0,1,0]},{"name":"socket_lamp","translation":[2,2,0]}],"meshes":[{"primitives":[{"attributes":{"POSITION":0},"material":0}]}],"materials":[{"name":"roof_tiles"}],"buffers":[{"byteLength":36}],"bufferViews":[{"buffer":0,"byteLength":36}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[2,1,0]}]}"#;

/// [`TRIANGLE`] as a `.glb` file.
pub fn triangle() -> Vec<u8> {
    glb(TRIANGLE, &corners())
}

/// The corners of [`TRIANGLE`], as its binary chunk holds them.
pub fn corners() -> Vec<u8> {
    [0.0f32, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 1.0, 0.0]
        .iter()
        .flat_map(|f| f.to_le_bytes())
        .collect()
}

/// A `.glb` file of the glTF `json` and the binary chunk `bin`.
pub fn glb(json: &str, bin: &[u8]) -> Vec<u8> {
    let mut json = json.as_bytes().to_vec();
    json.resize(json.len().next_multiple_of(4), b' ');
    let mut glb = b"glTF".to_vec();
    glb.extend(2u32.to_le_bytes());
    let length = 12 + 8 + json.len() + 8 + bin.len();
    glb.extend((length as u32).to_le_bytes());
    glb.extend((json.len() as u32).to_le_bytes());
    glb.extend(b"JSON");
    glb.extend(json);
    glb.extend((bin.len() as u32).to_le_bytes());
    glb.extend(b"BIN\0");
    glb.extend(bin);
    glb
}
