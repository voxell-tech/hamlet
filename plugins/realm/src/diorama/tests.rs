//! The parts the `diorama` service answers: each well within what one
//! call hands over, whole, and the same for the same template.

use hamlet_engine_sdk::decode;

use super::*;

/// The engine's `[budgets] bytes_per_call`.
const CALL_BYTES: usize = 1_048_576;

fn answered(
    template: &str,
    agents: u32,
) -> (Vec<Vec<u8>>, Vec<Part>) {
    let bytes = parts(template, agents).unwrap();
    let parts = bytes
        .iter()
        .map(|bytes| decode::<Part>(bytes).unwrap())
        .collect();
    (bytes, parts)
}

#[test]
fn a_town_comes_in_parts_each_well_within_a_call() {
    for agents in [0, 5, 14] {
        let (bytes, parts) = answered("indie_saas_launch", agents);
        let looks = parts[0].stage.as_ref().unwrap().looks.len();
        for (at, (part, bytes)) in
            parts.iter().zip(&bytes).enumerate()
        {
            assert!(
                bytes.len() < CALL_BYTES / 2,
                "{} bytes",
                bytes.len()
            );
            assert_eq!(part.part, at as u32);
            assert_eq!(part.parts, parts.len() as u32);
            assert_eq!(part.stage.is_some(), at == 0);
            let made = part
                .pieces
                .iter()
                .map(Piece::mesh_bytes)
                .sum::<usize>();
            assert!(made <= PART_BYTES, "{made} bytes of meshes");
            for piece in &part.pieces {
                let vertices = piece.positions.len() / 3;
                assert!(vertices > 0);
                assert_eq!(piece.normals.len(), vertices * 3);
                assert_eq!(piece.colors.len(), vertices * 3);
                assert_eq!(piece.normals().len(), vertices * 3);
                assert_eq!(piece.colors().len(), vertices * 4);
                assert_eq!(piece.indices.len() % 3, 0);
                assert!(
                    piece
                        .indices
                        .iter()
                        .all(|&i| (i as usize) < vertices)
                );
                assert!((piece.look as usize) < looks);
            }
        }
    }
}

#[test]
fn a_template_gives_the_same_town_each_time() {
    let (once, _) = answered("research_guild", 4);
    let (again, _) = answered("research_guild", 4);
    assert_eq!(once, again);
    let (other, _) = answered("indie_saas_launch", 4);
    assert_ne!(once, other);
}
