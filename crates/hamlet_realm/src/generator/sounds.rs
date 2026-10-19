//! The sounds of the generated world: what its robots are at, who
//! comes and goes on its roads, whose state has changed, and what
//! becomes of its houses, its agents and its hamlets.

use std::collections::HashMap;

use bevy::ecs::entity::{EntityHashMap, EntityHashSet};
use bevy::prelude::*;
use hamlet_vfx::magic::MagicSettings;

use super::magic::Musing;
use super::shift::{Part, Shift};
use super::society::{AgentId, Pair, RoadLevel, Society, TownId};
use super::sync::Drawn;
use super::traffic::Traffic;
use super::work::{Robot, Voting};
use super::{House, HouseMotion};
use crate::keeper::{KState, Keeper};
use crate::sound::{Cue, Cues, Ears, Sound, SoundSettings};
use crate::{Clock, Step};

/// Tells the sound what the generated world is doing.
pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Lives>().add_systems(
        Update,
        (
            (robot_cues, traffic_cues).in_set(Cues),
            (
                (state_cues, life_cues)
                    .run_if(resource_changed::<Society>),
                house_cues,
                vote_cues,
                muse_cues,
            )
                .chain()
                .after(Step::Scene)
                .before(Cues),
        ),
    );
}

/// The sound each robot in earshot was last making, and its count.
#[derive(Default)]
pub(super) struct Heard {
    last: EntityHashMap<(Sound, f32)>,
    now: EntityHashMap<(Sound, f32)>,
}

/// A cue for each robot's motion each time it makes its sound.
pub(super) fn robot_cues(
    clock: Res<Clock>,
    set: Res<SoundSettings>,
    ears: Single<&Transform, With<Ears>>,
    robots: Query<
        (Entity, &Keeper, &GlobalTransform, &InheritedVisibility),
        With<Robot>,
    >,
    mut heard: Local<Heard>,
    mut cues: MessageWriter<Cue>,
) {
    if clock.frozen {
        return;
    }
    let heard = &mut *heard;
    heard.now.clear();
    for (robot, keeper, place, shown) in &robots {
        let at = place.translation();
        if !shown.get() || ears.translation.distance(at) > set.earshot
        {
            continue;
        }
        let Some((sound, count)) = keeper.beat(clock.t) else {
            continue;
        };
        // A robot first heard, or one whose count has leapt, makes
        // no sound.
        if let Some(&(was, last)) = heard.last.get(&robot)
            && was == sound
            && count.floor() > last.floor()
            && count - last < 1.5
        {
            cues.write(Cue {
                sound,
                at,
                turn: robot.index_u32() ^ count as u32,
            });
        }
        heard.now.insert(robot, (sound, count));
    }
    std::mem::swap(&mut heard.last, &mut heard.now);
}

/// A door as a robot sets out with a message and as it comes home,
/// and the letter changing hands where it turns back.
pub(super) fn traffic_cues(
    traffic: Res<Traffic>,
    drawn: Res<Drawn>,
    places: Query<&GlobalTransform>,
    mut out: Local<HashMap<AgentId, (bool, Vec3)>>,
    mut cues: MessageWriter<Cue>,
) {
    let mut cue = |sound: Sound, at: Vec3, turn: u32| {
        cues.write(Cue { sound, at, turn });
    };
    out.retain(|walker, (_, at)| {
        let stays = traffic.trips.iter().any(|t| t.walker == *walker);
        if !stays {
            cue(Sound::Door, *at, walker.0);
        }
        stays
    });
    for trip in &traffic.trips {
        let Some(at) = drawn
            .robots
            .get(&trip.walker)
            .and_then(|&robot| places.get(robot).ok())
            .map(GlobalTransform::translation)
        else {
            continue;
        };
        match out.insert(trip.walker, (trip.back, at)) {
            None => {
                cue(Sound::Door, at, trip.walker.0 + 1);
                // The road lights ahead of the message.
                cue(Sound::RoadGlow, at, trip.walker.0);
            }
            Some((false, _)) if trip.back => {
                cue(Sound::Letter, at, trip.walker.0)
            }
            Some(_) => {}
        }
    }
}

/// A chime where an agent begins to wait for the chief, and a low
/// note where one faults.
pub(super) fn state_cues(
    society: Res<Society>,
    drawn: Res<Drawn>,
    places: Query<&GlobalTransform>,
    mut known: Local<HashMap<AgentId, KState>>,
    mut cues: MessageWriter<Cue>,
) {
    known.retain(|id, _| society.agent(*id).is_some());
    for agent in &society.agents {
        let was = known.insert(agent.id, agent.state);
        let sound = match agent.state {
            KState::Waiting => Sound::Chime,
            KState::Fault => Sound::Fault,
            _ => continue,
        };
        // An agent first seen in a state did not just enter it.
        if was.is_none_or(|was| was == agent.state) {
            continue;
        }
        let at = drawn
            .houses
            .get(&agent.id)
            .and_then(|house| places.get(house.0).ok())
            .map_or(Vec3::ZERO, GlobalTransform::translation);
        cues.write(Cue {
            sound,
            at,
            turn: agent.id.0,
        });
    }
}

/// The place of an agent's house, once it is drawn.
fn home(
    drawn: &Drawn,
    places: &Query<&GlobalTransform>,
    agent: AgentId,
) -> Option<Vec3> {
    drawn
        .houses
        .get(&agent)
        .and_then(|house| places.get(house.0).ok())
        .map(GlobalTransform::translation)
}

/// The society as it last changed, and the agents whose houses have
/// yet to be heard coming: each with true if it came back, and the
/// clock time it was seen.
#[derive(Resource, Default)]
pub(super) struct Lives {
    seen: bool,
    agents: HashMap<AgentId, bool>,
    towns: HashMap<TownId, AgentId>,
    roads: HashMap<Pair, RoadLevel>,
    coming: HashMap<AgentId, (bool, f32)>,
}

/// Seconds an agent's coming waits for its house to rise.
const COMING: f32 = 20.0;

/// Sounds for what becomes of agents and hamlets: one removed, a
/// hamlet founded, a mayor changed, and a road laid or raised
/// between two that were there already.
pub(super) fn life_cues(
    clock: Res<Clock>,
    society: Res<Society>,
    drawn: Res<Drawn>,
    places: Query<&GlobalTransform>,
    mut lives: ResMut<Lives>,
    mut cues: MessageWriter<Cue>,
) {
    let lives = &mut *lives;
    // The things there when the world is first seen did not just
    // happen.
    let live =
        std::mem::replace(&mut lives.seen, true) && !clock.frozen;
    let mut cue = |sound: Sound, at: Vec3, turn: u32| {
        if live {
            cues.write(Cue { sound, at, turn });
        }
    };
    let mut left = false;
    let mut newcomers = Vec::new();
    lives.agents.retain(|id, _| society.agent(*id).is_some());
    for agent in &society.agents {
        match lives.agents.insert(agent.id, agent.archived) {
            None if !agent.archived && live => {
                lives.coming.insert(agent.id, (false, clock.t));
                newcomers.push(agent.id);
            }
            Some(true) if !agent.archived => {
                lives.coming.insert(agent.id, (true, clock.t));
                newcomers.push(agent.id);
            }
            Some(false) if agent.archived => {
                left = true;
                // One merged away is heard as its house goes over.
                if agent.into.is_none()
                    && let Some(at) = home(&drawn, &places, agent.id)
                {
                    cue(Sound::Gone, at, agent.id.0);
                }
            }
            _ => {}
        }
    }
    lives.coming.retain(|_, (_, at)| clock.t - *at < COMING);

    lives.towns.retain(|id, _| society.town(*id).is_some());
    for town in &society.towns {
        match lives.towns.insert(town.id, town.leader) {
            None => cue(Sound::Founded, Vec3::ZERO, town.id.0),
            Some(was) if was != town.leader => {
                cue(Sound::Mayor, Vec3::ZERO, town.leader.0)
            }
            Some(_) => {}
        }
    }

    lives.roads.retain(|pair, _| society.road(*pair).is_some());
    for road in &society.roads {
        let was = lives.roads.insert(road.pair, road.level);
        let better = was.is_none_or(|was| road.level > was);
        // A newcomer's road is part of its coming, and a merge
        // moves roads that were there.
        let old = !newcomers.contains(&road.pair.0)
            && !newcomers.contains(&road.pair.1);
        let ends = home(&drawn, &places, road.pair.0).zip(home(
            &drawn,
            &places,
            road.pair.1,
        ));
        if better
            && old
            && !left
            && let Some((a, b)) = ends
        {
            cue(Sound::RoadLaid, a.midpoint(b), road.pair.0.0);
        }
    }
}

/// A sound that is due at a clock time.
type Due = (f32, Sound, Vec3, u32);

/// Sounds for a house coming and going: its timber and its runes
/// as it rises, a thump as it settles and the agent's own notes
/// after; a rattle, a thud and embers as it falls; or its motes
/// going over to the house it was merged into.
#[allow(clippy::too_many_arguments)]
pub(super) fn house_cues(
    clock: Res<Clock>,
    motion: Res<HouseMotion>,
    magic: Res<MagicSettings>,
    society: Res<Society>,
    drawn: Res<Drawn>,
    houses: Query<(Entity, &House, &Shift, &GlobalTransform)>,
    places: Query<&GlobalTransform>,
    mut lives: ResMut<Lives>,
    mut heard: Local<EntityHashSet>,
    mut due: Local<Vec<Due>>,
    mut cues: MessageWriter<Cue>,
) {
    heard.retain(|house| houses.contains(*house));
    for (entity, house, shift, place) in &houses {
        let Some((began, _)) = shift.began() else {
            continue;
        };
        // A still shows the moment after, with no sound in the air.
        if !heard.insert(entity) || clock.frozen {
            continue;
        }
        let at = place.translation();
        let turn = house.agent.0;
        let mut now = |sound: Sound| {
            cues.write(Cue { sound, at, turn });
        };
        match shift.part() {
            Part::Rise => {
                now(Sound::HouseRise);
                now(Sound::Rune);
                let settled = began + motion.rise;
                due.push((settled, Sound::HouseSettle, at, turn));
                if let Some((back, _)) = lives
                    .coming
                    .remove(&house.agent)
                    .filter(|c| clock.t - c.1 < COMING)
                {
                    let sound = if back {
                        Sound::Restored
                    } else {
                        Sound::Born
                    };
                    let shown =
                        (began + motion.props_after).max(settled);
                    due.push((shown + 0.25, sound, at, turn));
                }
            }
            Part::Fall => {
                let host = society
                    .agent(house.agent)
                    .and_then(|gone| gone.into)
                    .and_then(|host| home(&drawn, &places, host));
                if let Some(host) = host {
                    now(Sound::MergeAway);
                    let lands = began + magic.flight_time;
                    due.push((lands, Sound::MergeLand, host, turn));
                } else {
                    now(Sound::HouseShudder);
                    let falls = began + motion.shudder;
                    due.push((falls, Sound::HouseFall, at, turn));
                    due.push((falls + 0.1, Sound::Embers, at, turn));
                }
            }
            Part::Clear | Part::Rest => {}
        }
    }
    due.retain(|&(when, sound, at, turn)| {
        if when > clock.t {
            return true;
        }
        cues.write(Cue { sound, at, turn });
        false
    });
}

/// Seconds without a ballot after which the next one opens a vote.
const VOTE_APART: f32 = 30.0;

/// A gavel as a vote opens, and a slip of paper for every ballot.
pub(super) fn vote_cues(
    clock: Res<Clock>,
    voting: Res<Voting>,
    drawn: Res<Drawn>,
    places: Query<&GlobalTransform>,
    mut cast: Local<HashMap<AgentId, f32>>,
    mut last: Local<Option<f32>>,
    mut cues: MessageWriter<Cue>,
) {
    cast.retain(|agent, _| voting.0.contains_key(agent));
    for (&agent, &when) in &voting.0 {
        if cast.insert(agent, when) == Some(when) {
            continue;
        }
        let at = home(&drawn, &places, agent).unwrap_or_default();
        if last.is_none_or(|last| clock.t - last > VOTE_APART) {
            cues.write(Cue {
                sound: Sound::VoteCalled,
                at,
                turn: agent.0,
            });
        }
        *last = Some(clock.t);
        cues.write(Cue {
            sound: Sound::Ballot,
            at,
            turn: agent.0 ^ when.to_bits(),
        });
    }
}

/// A note of glass as the light of a thinking agent appears.
pub(super) fn muse_cues(
    drawn: Res<Drawn>,
    lights: Query<&Musing, Added<Musing>>,
    places: Query<&GlobalTransform>,
    mut turn: Local<u32>,
    mut cues: MessageWriter<Cue>,
) {
    for musing in &lights {
        let Some(at) = home(&drawn, &places, musing.of()) else {
            continue;
        };
        *turn = turn.wrapping_add(1);
        cues.write(Cue {
            sound: Sound::Muse,
            at,
            turn: *turn,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::testing;

    fn heard(app: &mut App) -> Vec<Sound> {
        app.update();
        let cues = app.world().resource::<Messages<Cue>>();
        cues.iter_current_update_messages()
            .map(|cue| cue.sound)
            .collect::<Vec<_>>()
    }

    #[test]
    fn what_is_there_at_first_is_silent_and_what_comes_after_is_not()
    {
        let mut land = testing::land(1);
        let mut app = App::new();
        app.add_message::<Cue>()
            .init_resource::<Clock>()
            .init_resource::<Drawn>()
            .init_resource::<Lives>()
            .init_resource::<Society>()
            .add_systems(
                Update,
                life_cues.run_if(resource_changed::<Society>),
            );
        let mut society = app.world_mut().resource_mut::<Society>();
        let first = society.found_town(&mut land);
        let old = society.add_agent(&land, first).unwrap();
        assert_eq!(heard(&mut app), []);
        assert_eq!(heard(&mut app), []);

        let mut society = app.world_mut().resource_mut::<Society>();
        society.found_town(&mut land);
        let new = society.add_agent(&land, first).unwrap();
        assert_eq!(heard(&mut app), [Sound::Founded]);
        // A newcomer is heard when its house has risen, and one that
        // was there at first is never taken for a newcomer.
        let lives = app.world().resource::<Lives>();
        assert_eq!(lives.coming.get(&new).map(|c| c.0), Some(false));
        assert!(!lives.coming.contains_key(&old));

        let mut society = app.world_mut().resource_mut::<Society>();
        society.remove_agent(new);
        society.restore(&land, new);
        heard(&mut app);
        let lives = app.world().resource::<Lives>();
        assert_eq!(lives.coming.get(&new).map(|c| c.0), Some(false));
        let mut society = app.world_mut().resource_mut::<Society>();
        society.remove_agent(new);
        heard(&mut app);
        app.world_mut()
            .resource_mut::<Society>()
            .restore(&land, new);
        heard(&mut app);
        let lives = app.world().resource::<Lives>();
        assert_eq!(lives.coming.get(&new).map(|c| c.0), Some(true));
    }
}
