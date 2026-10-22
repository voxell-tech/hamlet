//! Where the bazaar is pitched: on a site of its own beside the first
//! town, its ground claimed against the towns and held level.

use glam::{Quat, Vec2};

use crate::build::frame::Frame;
use crate::folk::chief::stand;
use crate::society::{Claim, Place, Society};
use crate::world::land::Land;

/// The radius of the ground the bazaar keeps level and clear.
pub const GROUNDS: f32 = 15.0;

/// Pitches the bazaar where the land puts it, clear of every town and
/// every other claim: claims its ground so that no town is founded or
/// set down on it and roads go round it, and holds it level and clear
/// of trees. Returns its frame, its gate toward the first town. None
/// when the land has no room for it.
pub fn pitch(
    land: &mut Land,
    society: &mut Society,
) -> Option<Frame> {
    let taken = society
        .towns
        .iter()
        .map(|town| land.sites[town.site].at)
        .chain(society.claims.iter().map(|claim| claim.at))
        .collect::<Vec<_>>();
    let at = stand(land, &taken)?;
    society.claims.push(Claim {
        place: Place::Bazaar,
        at,
        radius: (land.set.town.keep_apart * 0.5).max(GROUNDS),
    });
    let mut rounds = land.held();
    rounds.push((at, GROUNDS));
    rounds.sort_by(|a, b| {
        a.0.x.total_cmp(&b.0.x).then(a.0.y.total_cmp(&b.0.y))
    });
    land.hold(&rounds);
    let first = land
        .sites
        .iter()
        .find(|site| site.buildable)
        .map_or(Vec2::ZERO, |site| site.at);
    let toward = (first - at).normalize_or(Vec2::Y);
    Some(
        Frame::at(land.on_ground(at)).with_turn(
            Quat::from_rotation_y(toward.x.atan2(toward.y)),
        ),
    )
}
