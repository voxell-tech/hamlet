//! The camera's view of the hamlets, and how far a hand may take it.

use bevy::prelude::*;

use super::Sited;
use crate::generator::land::Land;
use crate::generator::society::Society;
use crate::generator::{Ahead, GeneratorView};
use crate::pick::{Centre, Glide, Leash, Orbit, Stance};
use crate::sky::CameraSettings;

/// Number of hamlets with a town, the middle of them, and the
/// distance the camera stands back at to see them all.
fn framing(
    camera: &CameraSettings,
    land: &Land,
    society: &Society,
    hamlets: &Query<&Sited>,
) -> Option<(usize, Vec2, f32)> {
    let sites = || {
        hamlets
            .iter()
            .filter_map(|sited| society.town(sited.0))
            .map(|town| land.sites[town.site].at)
    };
    let count = sites().count();
    if count == 0 {
        return None;
    }
    let middle = sites().sum::<Vec2>() / count as f32;
    let spread =
        sites().map(|at| at.distance(middle)).fold(0.0, f32::max);
    let base = (camera.distance + spread * camera.spread_distance)
        .min(camera.reach);
    Some((count, middle, base))
}

/// Keeps a hand from taking the camera away from every place.
pub(super) fn tether(
    mut leash: ResMut<Leash>,
    camera: Res<CameraSettings>,
    land: Res<Land>,
    society: Res<Society>,
    hamlets: Query<&Sited>,
) {
    let Some((_, _, base)) =
        framing(&camera, &land, &society, &hamlets)
    else {
        return;
    };
    let next = Leash {
        zoom: Some((
            camera.distance * camera.zoom.0 / base,
            camera.reach / base,
        )),
        anchors: society
            .places()
            .filter_map(|place| society.stands(&land, place))
            .map(|(at, _)| at)
            .collect(),
    };
    leash.set_if_neq(next);
}

/// Points the camera at the middle of the hamlets, far enough to
/// see them all, moved by hand through `Orbit`. A town that is set
/// down somewhere else leaves the view where it is.
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
    mut framed: Local<Option<(usize, Vec2, f32)>>,
) {
    let Some((count, middle, base)) =
        framing(&camera, &land, &society, &hamlets)
    else {
        return;
    };
    // The same hamlets in other places: the hand's share makes up
    // for what the middle and the distance changed by.
    if let Some((before, was, far)) = *framed
        && before == count
        && (was, far) != (middle, base)
    {
        for held in [&mut *orbit, &mut glide.0] {
            held.pan += was - middle;
            held.zoom *= far / base;
        }
    }
    *framed = Some((count, middle, base));
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
