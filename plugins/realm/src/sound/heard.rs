//! The cues of what the realm hears happen of itself: the turn of the
//! day, the calls for the chief, the lives of agents and hamlets, and
//! what follows a house rising or falling and a ballot cast.

use std::collections::HashMap;

use glam::Vec3;
use hamlet_engine_bevy::api::agents::{AgentsEvent, Choice};
use hamlet_engine_bevy::prelude::*;

use crate::bridge::Bridge;
use crate::folk::keeper::KState;
use crate::folk::sounds::{FolkCue, FolkSound};
use crate::fx::magic::Nudged;
use crate::realm::Clock;
use crate::society::{AgentId, Pair, RoadLevel, Society, TownId};
use crate::sound::kinds::Sound;
use crate::sound::{Cue, SoundSettings};
use crate::tuning::WorldTuning;
use crate::world::land::Land;
use crate::world::layout::plot;
use crate::world::sky::{Sky, SkySettings};

/// Seconds an agent's coming waits for its house to rise.
const COMING: f32 = 20.0;

/// Seconds without a ballot after which the next one opens a vote.
const VOTE_APART: f32 = 30.0;

/// The part of the day the sky shows.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DayPart {
    Dawn,
    #[default]
    Day,
    Dusk,
    Night,
}

impl DayPart {
    fn at(set: &SkySettings, hour: f32) -> Self {
        let half = set.twilight * 0.5;
        if !set.moving {
            if set.night(hour) {
                Self::Night
            } else {
                Self::Day
            }
        } else if (hour - set.sunrise).abs() < half {
            Self::Dawn
        } else if (hour - set.sunset).abs() < half {
            Self::Dusk
        } else if set.night(hour) {
            Self::Night
        } else {
            Self::Day
        }
    }
}

/// The society as it last changed, and the agents whose houses have
/// yet to be heard coming: each with true if it came back, and the
/// clock time it was seen.
#[derive(Resource, Default)]
pub struct Lives {
    seen: bool,
    agents: HashMap<AgentId, bool>,
    towns: HashMap<TownId, AgentId>,
    roads: HashMap<Pair, RoadLevel>,
    coming: HashMap<AgentId, (bool, f32)>,
}

/// The cues due at a later clock time, and the time of the last
/// ballot.
#[derive(Resource, Default)]
pub struct Due {
    cues: Vec<(f32, Cue)>,
    ballot: Option<f32>,
}

impl Due {
    /// Takes the cues due by `now`.
    pub fn take(&mut self, now: f32) -> Vec<Cue> {
        let mut taken = Vec::new();
        self.cues.retain(|&(when, cue)| {
            if when > now {
                return true;
            }
            taken.push(cue);
            false
        });
        taken
    }

    fn push(&mut self, when: f32, sound: Sound, at: Vec3, turn: u32) {
        self.cues.push((when, Cue { sound, at, turn }));
    }
}

/// The place of an agent's house.
fn home(
    land: &Land,
    society: &Society,
    agent: AgentId,
) -> Option<Vec3> {
    let agent = society.agent(agent)?;
    let town = society.town(agent.town)?;
    Some(land.on_ground(plot(land, town.site, agent.plot).at))
}

/// A few small bars as the day breaks and as night falls.
pub fn turn_of_day(
    tuning: Res<WorldTuning>,
    sky: Res<Sky>,
    mut part: ResMut<DayPart>,
    mut cues: MessageWriter<Cue>,
) {
    let now = DayPart::at(&tuning.sky, sky.hour);
    if *part == now {
        return;
    }
    let was = std::mem::replace(&mut *part, now);
    let sound = match (was, now) {
        (DayPart::Night, DayPart::Dawn | DayPart::Day) => Sound::Dawn,
        (DayPart::Day, DayPart::Dusk | DayPart::Night) => Sound::Dusk,
        _ => return,
    };
    cues.write(Cue {
        sound,
        at: Vec3::ZERO,
        turn: 0,
    });
}

/// A chime where an agent begins to wait for the chief, and a low
/// note where one faults.
pub fn state_cues(
    land: Res<Land>,
    society: Res<Society>,
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
        cues.write(Cue {
            sound,
            at: home(&land, &society, agent.id).unwrap_or(Vec3::ZERO),
            turn: agent.id.0,
        });
    }
}

/// Sounds for what becomes of agents and hamlets: one removed, a
/// hamlet founded, a mayor changed, and a road laid or raised
/// between two that were there already.
pub fn life_cues(
    clock: Res<Clock>,
    land: Res<Land>,
    society: Res<Society>,
    mut lives: ResMut<Lives>,
    mut cues: MessageWriter<Cue>,
) {
    let lives = &mut *lives;
    // The things there when the realm is first seen did not just
    // happen.
    let live = std::mem::replace(&mut lives.seen, true);
    let mut cue = |sound: Sound, at: Vec3, turn: u32| {
        if live {
            cues.write(Cue { sound, at, turn });
        }
    };
    let home = |agent: AgentId| home(&land, &society, agent);
    let mut left = false;
    let mut newcomers = Vec::new();
    lives.agents.retain(|id, _| society.agent(*id).is_some());
    for agent in &society.agents {
        match lives.agents.insert(agent.id, agent.archived) {
            None if !agent.archived && live => {
                lives.coming.insert(agent.id, (false, clock.now));
                newcomers.push(agent.id);
            }
            Some(true) if !agent.archived => {
                lives.coming.insert(agent.id, (true, clock.now));
                newcomers.push(agent.id);
            }
            Some(false) if agent.archived => {
                left = true;
                // One merged away is heard as its house goes over.
                if agent.into.is_none()
                    && let Some(at) = home(agent.id)
                {
                    cue(Sound::Gone, at, agent.id.0);
                }
            }
            _ => {}
        }
    }
    lives.coming.retain(|_, (_, at)| clock.now - *at < COMING);

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
        let ends = home(road.pair.0).zip(home(road.pair.1));
        if better
            && old
            && !left
            && let Some((a, b)) = ends
        {
            cue(Sound::RoadLaid, a.midpoint(b), road.pair.0.0);
        }
    }
}

/// What follows the cues of the frame: a house that rises opens its
/// runes, settles and, for a newcomer, plays the agent's notes; one
/// that falls comes down in embers; and a merged one's motes land on
/// the house it joins.
pub fn follow(
    clock: Res<Clock>,
    tuning: Res<WorldTuning>,
    set: Res<SoundSettings>,
    land: Res<Land>,
    society: Res<Society>,
    mut lives: ResMut<Lives>,
    mut due: ResMut<Due>,
    mut cues: MessageReader<Cue>,
) {
    let now = clock.now;
    let motion = &tuning.house_motion;
    for &Cue { sound, at, turn } in cues.read() {
        let agent = AgentId(turn);
        match sound {
            Sound::HouseRise => {
                due.push(now, Sound::Rune, at, turn);
                let settled = now + motion.rise;
                due.push(settled, Sound::HouseSettle, at, turn);
                if let Some((back, _)) = lives
                    .coming
                    .remove(&agent)
                    .filter(|coming| now - coming.1 < COMING)
                {
                    let sound = if back {
                        Sound::Restored
                    } else {
                        Sound::Born
                    };
                    let shown =
                        (now + motion.props_after).max(settled);
                    due.push(shown + 0.25, sound, at, turn);
                }
            }
            Sound::HouseShudder => {
                let falls = now + motion.shudder;
                due.push(falls, Sound::HouseFall, at, turn);
                due.push(falls + 0.1, Sound::Embers, at, turn);
            }
            Sound::MergeAway => {
                let host = society
                    .agent(agent)
                    .and_then(|gone| gone.into)
                    .and_then(|host| home(&land, &society, host));
                if let Some(host) = host {
                    let lands = now + set.flight_time;
                    due.push(lands, Sound::MergeLand, host, turn);
                }
            }
            _ => {}
        }
    }
}

/// The sounds of the robots and of the chief's hand, as they are
/// made.
pub fn hear_folk(
    clock: Res<Clock>,
    mut folk: MessageReader<FolkCue>,
    mut due: ResMut<Due>,
) {
    for cue in folk.read() {
        let sound = match cue.sound {
            FolkSound::Step => Sound::Step,
            FolkSound::RoadStep => Sound::RoadStep,
            FolkSound::Hammer => Sound::Hammer,
            FolkSound::Wheel => Sound::Wheel,
            FolkSound::Steam => Sound::Steam,
            FolkSound::Quill => Sound::Quill,
            FolkSound::Kite => Sound::Kite,
            FolkSound::Door => Sound::Door,
            FolkSound::Letter => Sound::Letter,
            FolkSound::Poke => Sound::Poke,
            FolkSound::Annoy => Sound::Annoy,
            FolkSound::Pat => Sound::Pat,
            FolkSound::Lift => Sound::Lift,
            FolkSound::Drop => Sound::Drop,
            FolkSound::Wave => Sound::Wave,
            FolkSound::Cheer => Sound::Cheer,
            FolkSound::Introduce => Sound::Introduce,
            FolkSound::WickOut => Sound::WickOut,
            FolkSound::WickIn => Sound::WickIn,
        };
        due.push(clock.now, sound, cue.at, cue.turn);
    }
}

/// A glass note as a thinking light is nudged.
pub fn hear_nudges(
    clock: Res<Clock>,
    mut nudged: MessageReader<Nudged>,
    mut due: ResMut<Due>,
) {
    for nudge in nudged.read() {
        due.push(clock.now, Sound::Nudge, nudge.at, nudge.agent.0);
    }
}

/// A gavel as a vote opens and a slip of paper for every ballot, a
/// message setting out on a road, and the chief's answers and stops.
pub fn hear_agents(
    clock: Res<Clock>,
    land: Res<Land>,
    society: Res<Society>,
    bridge: Res<Bridge>,
    mut events: MessageReader<AgentsEventReceived>,
    mut due: ResMut<Due>,
) {
    let now = clock.now;
    for event in events.read() {
        match &event.event {
            AgentsEvent::Voted { agent } => {
                let Some(agent) = bridge.housed(agent) else {
                    continue;
                };
                let at =
                    home(&land, &society, agent).unwrap_or_default();
                if due
                    .ballot
                    .is_none_or(|last| now - last > VOTE_APART)
                {
                    due.push(now, Sound::VoteCalled, at, agent.0);
                }
                due.ballot = Some(now);
                let turn = agent.0 ^ now.to_bits();
                due.push(now, Sound::Ballot, at, turn);
            }
            AgentsEvent::Answered { choice, .. } => {
                let sound = match choice {
                    Choice::Allow
                    | Choice::Always
                    | Choice::GoAhead => Sound::Allowed,
                    Choice::Deny | Choice::Withdraw => Sound::Denied,
                    // A reply in words and a dismissal keep the sound
                    // of their button.
                    Choice::Reply | Choice::Dismiss => continue,
                };
                due.push(now, sound, Vec3::ZERO, 0);
            }
            // A door as the message sets out, and the road lighting
            // ahead of it.
            AgentsEvent::Carried { from, .. } => {
                let Some(agent) = bridge.housed(from) else {
                    continue;
                };
                let Some(at) = home(&land, &society, agent) else {
                    continue;
                };
                due.push(now, Sound::Door, at, agent.0 + 1);
                due.push(now, Sound::RoadGlow, at, agent.0);
            }
            AgentsEvent::Vetoed { .. } => {
                due.push(now, Sound::Stopped, Vec3::ZERO, 0);
            }
            _ => {}
        }
    }
}
