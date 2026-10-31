use super::squared_distance;
use crate::generator::noise::{key, unit};

#[test]
fn distances_are_those_of_the_nearest_target() {
    let (w, h) = (23, 17);
    let target = (0..w * h)
        .map(|i| unit(key(&[i as u32])) < 0.08)
        .collect::<Vec<_>>();
    let found = squared_distance(&target, w, h);
    for (i, &d) in found.iter().enumerate() {
        let (x, y) = ((i % w) as f32, (i / w) as f32);
        let nearest = target
            .iter()
            .enumerate()
            .filter(|(_, t)| **t)
            .map(|(j, _)| {
                let (tx, ty) = ((j % w) as f32, (j / w) as f32);
                (x - tx).powi(2) + (y - ty).powi(2)
            })
            .fold(f32::MAX, f32::min);
        assert_eq!(d, nearest, "at ({x}, {y})");
    }
}
