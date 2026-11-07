//! The camera's view of the two islands, and how far a hand may take
//! it.

use bevy::prelude::*;

use super::Sited;
use crate::generator::land::{IslandId, Land};
use crate::generator::society::Society;
use crate::generator::{Ahead, GeneratorView};
use crate::pick::{Centre, Glide, Leash, Orbit, Stance};
use crate::sky::CameraSettings;

/// The places worth looking at: what stands in the city, then the
/// towns, or the middle of the hamlet island while it has none.
fn anchors(
    land: &Land,
    society: &Society,
    hamlets: &Query<&Sited>,
) -> (Vec<Vec2>, usize) {
    let mut out = land.island(IslandId::City).shores.clone();
    let towns = hamlets
        .iter()
        .filter_map(|sited| society.town(sited.0))
        .map(|town| land.sites[town.site].at)
        .collect::<Vec<_>>();
    let count = towns.len();
    if towns.is_empty() {
        out.push(land.hamlet_centre());
    } else {
        out.extend(towns);
    }
    (out, count)
}

/// Number of hamlets with a town, the middle between the two
/// islands' places, and the distance the camera stands back at to see
/// them all.
fn framing(
    camera: &CameraSettings,
    land: &Land,
    society: &Society,
    hamlets: &Query<&Sited>,
) -> (usize, Vec2, f32) {
    let (places, count) = anchors(land, society, hamlets);
    let mean =
        |ats: &[Vec2]| ats.iter().sum::<Vec2>() / ats.len() as f32;
    let city = land.island(IslandId::City).shores.len();
    let middle =
        (mean(&places[..city]) + mean(&places[city..])) * 0.5;
    let spread = places
        .iter()
        .map(|at| at.distance(middle))
        .fold(0.0, f32::max);
    let base = (camera.distance + spread * camera.spread_distance)
        .min(camera.reach);
    (count, middle, base)
}

/// Keeps a hand from taking the camera away from every place.
pub(super) fn tether(
    mut leash: ResMut<Leash>,
    camera: Res<CameraSettings>,
    land: Res<Land>,
    society: Res<Society>,
    hamlets: Query<&Sited>,
) {
    let (_, _, base) = framing(&camera, &land, &society, &hamlets);
    let next = Leash {
        zoom: Some((
            camera.distance * camera.zoom.0 / base,
            camera.reach / base,
        )),
        anchors: anchors(&land, &society, &hamlets).0,
    };
    leash.set_if_neq(next);
}

/// What `aim` kept of the view it last framed.
#[derive(Clone, Copy)]
pub(super) struct Framed {
    count: usize,
    middle: Vec2,
    base: f32,
    hamlet: Vec2,
}

/// Points the camera at the middle between the islands, far enough
/// to see them all, moved by hand through `Orbit`. A town that is set
/// down somewhere else, or an island that moves, leaves the view
/// where it is, or moves with the hamlet island if it is on it.
pub(super) fn aim(
    mut orbit: ResMut<Orbit>,
    mut glide: ResMut<Glide>,
    mut centre: ResMut<Centre>,
    mut stance: ResMut<Stance>,
    camera: Res<CameraSettings>,
    land: Res<Land>,
    society: Res<Society>,
    hamlets: Query<&Sited>,
    mut view: ResMut<GeneratorView>,
    mut ahead: ResMut<Ahead>,
    mut framed: Local<Option<Framed>>,
) {
    let (count, middle, base) =
        framing(&camera, &land, &society, &hamlets);
    let hamlet = land.hamlet_centre();
    // The same hamlets in other places: the hand's share makes up
    // for what the middle and the distance changed by.
    if let Some(was) = *framed
        && was.count == count
        && (was.middle, was.base, was.hamlet)
            != (middle, base, hamlet)
    {
        let by = hamlet - was.hamlet;
        for held in [&mut *orbit, &mut glide.0] {
            let mut focus = was.middle + held.pan;
            if focus.distance(was.hamlet) < focus.length() {
                focus += by;
            }
            held.pan = focus - middle;
            held.zoom *= was.base / base;
        }
    }
    *framed = Some(Framed {
        count,
        middle,
        base,
        hamlet,
    });
    if centre.0 != middle {
        centre.0 = middle;
    }
    let seen = |orbit: Orbit| GeneratorView {
        focus: middle + orbit.pan,
        dist: base * orbit.zoom,
        yaw: orbit.yaw,
        pitch: (camera.pitch + orbit.pitch.to_degrees())
            .clamp(camera.pitch_limits.0, camera.pitch_limits.1),
    };
    // The ground is drawn ahead of a camera that is on its way.
    let going = Ahead((*orbit != glide.0).then(|| seen(*orbit)));
    ahead.set_if_neq(going);
    let next = seen(glide.0);
    let stands = Some((next.dist, next.pitch.to_radians()));
    if stance.0 != stands {
        stance.0 = stands;
    }
    view.set_if_neq(next);
}
