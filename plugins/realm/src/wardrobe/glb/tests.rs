//! A `.glb` written by hand, read back as a worn mesh is.

use super::*;

/// The bytes of a `.glb` holding `meshes`, each on a node of its own,
/// and `spots`, each an empty node.
pub fn write(
    meshes: &[(&str, &Model)],
    spots: &[(&str, Vec3)],
) -> Vec<u8> {
    let mut bin = Vec::<u8>::new();
    let (mut views, mut accessors, mut parts, mut nodes) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut view = |bin: &mut Vec<u8>,
                    bytes: Vec<u8>,
                    target: u32| {
        let at = bin.len();
        bin.extend(&bytes);
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        views.push(format!(
            r#"{{"buffer":0,"byteOffset":{at},"byteLength":{},"target":{target}}}"#,
            bytes.len()
        ));
        views.len() - 1
    };
    let floats = |vs: &[Vec3]| {
        vs.iter()
            .flat_map(|v| v.to_array())
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>()
    };
    for (k, (name, model)) in meshes.iter().enumerate() {
        let (lo, hi) = model.bounds();
        let n = model.pos.len();
        let mut attribute = |vs: &[Vec3], bounds: bool| {
            let at = view(&mut bin, floats(vs), 34962);
            let bounds = match bounds {
                true => format!(
                    r#","min":[{},{},{}],"max":[{},{},{}]"#,
                    lo.x, lo.y, lo.z, hi.x, hi.y, hi.z
                ),
                false => String::new(),
            };
            accessors.push(format!(
                r#"{{"bufferView":{at},"componentType":5126,"count":{n},"type":"VEC3"{bounds}}}"#
            ));
            accessors.len() - 1
        };
        let pos = attribute(&model.pos, true);
        let nor = attribute(&model.nor, false);
        let col = attribute(&model.col, false);
        let idx = model
            .idx
            .iter()
            .flat_map(|i| i.to_le_bytes())
            .collect::<Vec<_>>();
        let at = view(&mut bin, idx, 34963);
        accessors.push(format!(
            r#"{{"bufferView":{at},"componentType":5125,"count":{},"type":"SCALAR"}}"#,
            model.idx.len()
        ));
        let idx = accessors.len() - 1;
        parts.push(format!(
            r#"{{"name":"{name}","primitives":[{{"attributes":{{"POSITION":{pos},"NORMAL":{nor},"COLOR_0":{col}}},"indices":{idx}}}]}}"#
        ));
        nodes.push(format!(r#"{{"name":"{name}","mesh":{k}}}"#));
    }
    for (name, at) in spots {
        nodes.push(format!(
            r#"{{"name":"{name}","translation":[{},{},{}]}}"#,
            at.x, at.y, at.z
        ));
    }
    let all = (0..nodes.len())
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let mut json = format!(
        r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[{all}]}}],"nodes":[{}],"meshes":[{}],"accessors":[{}],"bufferViews":[{}],"buffers":[{{"byteLength":{}}}]}}"#,
        nodes.join(","),
        parts.join(","),
        accessors.join(","),
        views.join(","),
        bin.len()
    )
    .into_bytes();
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    let mut out = Vec::new();
    let total = 12 + 8 + json.len() + 8 + bin.len();
    for word in [0x4654_6c67, 2, total as u32] {
        out.extend(u32::to_le_bytes(word));
    }
    out.extend((json.len() as u32).to_le_bytes());
    out.extend(0x4E4F_534Au32.to_le_bytes());
    out.extend(json);
    out.extend((bin.len() as u32).to_le_bytes());
    out.extend(0x004E_4942u32.to_le_bytes());
    out.extend(bin);
    out
}

/// A model of one triangle of `colour`, its corners `corners`.
pub fn triangle(corners: [Vec3; 3], colour: Vec3) -> Model {
    Model {
        pos: corners.to_vec(),
        nor: vec![Vec3::Z; 3],
        col: vec![colour; 3],
        idx: vec![0, 1, 2],
    }
}

#[test]
fn a_mesh_written_as_glb_reads_back_whole_with_its_spots() {
    let walls = triangle(
        [Vec3::ZERO, Vec3::X, Vec3::Y],
        Vec3::new(0.2, 0.4, 0.6),
    );
    let door = Vec3::new(0.25, 0.0, 1.0);
    let bytes = write(&[("walls", &walls)], &[("door", door)]);
    let glb = Glb::read(&bytes).unwrap();
    assert_eq!(glb.mesh("walls"), Some(&walls));
    assert_eq!(glb.spot("door"), Some(door));
    assert!(glb.mesh("panes").is_none());
    assert!(Glb::read(b"not a glb").is_err());
}
