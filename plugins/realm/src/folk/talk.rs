//! The balloon over a robot with its agent's words or thought.

use std::collections::HashMap;

use glam::Vec3;
use hamlet_engine_bevy::api::agents::{AgentsEvent, Doing, Paper};
use hamlet_engine_bevy::prelude::*;

use crate::bridge::Bridge;
use crate::build::frame::Frame;
use crate::build::upload::Draw;
use crate::folk::hand::Hand;
use crate::folk::keeper::Robots;
use crate::realm::Clock;
use crate::society::AgentId;
use crate::sound::{Cue, Sound};
use crate::tuning::WorldTuning;

/// The balloons over robots.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
pub struct BalloonSettings {
    /// The most letters it shows, counted from the end of what was
    /// said, and the most of the gist of a thought.
    pub most: usize,
    pub most_thought: usize,
    /// Height it floats over a robot's head.
    pub lift: f32,
    /// Camera distance beyond which balloons are not shown.
    pub far: f32,
    /// Seconds a balloon stays after its agent's last word, when the
    /// agent has finished and while it is still at work.
    pub linger: f32,
    pub linger_busy: f32,
    /// Seconds its words stay before a thought with no words yet
    /// takes their place.
    pub hold: f32,
}

/// Pixels between a robot's head and the point of its balloon.
const GAP: f32 = 2.0;

/// What an agent said or thought last, and its balloon.
struct Speech {
    /// What the balloon hangs from, over the robot's head.
    anchor: Option<Entity>,
    said: String,
    thought: bool,
    /// The words were told since the balloon was last kept.
    fresh: bool,
    /// Seconds since its agent last said anything.
    quiet: f32,
    /// The words made of what was said and the deed they were made
    /// with.
    made: Option<(String, Option<Doing>)>,
    /// The words the balloon shows, whether they are a thought, and
    /// the seconds since they last changed.
    shown: String,
    shows_thought: bool,
    steady: f32,
    /// What the balloon on the anchor says now, once it is up.
    up: Option<SceneBalloon>,
}

/// What the agents say or think, and where it is shown.
#[derive(Resource, Default)]
pub struct Talk {
    speakers: HashMap<AgentId, Speech>,
}

/// The end of `text`, cut at a word, as long as fits a balloon.
fn tail(text: &str, most: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let count = text.chars().count();
    if count <= most {
        return text;
    }
    let cut = text
        .char_indices()
        .nth(count - most)
        .map_or(0, |(at, _)| at);
    let rest = &text[cut..];
    let rest = rest.split_once(' ').map_or(rest, |(_, rest)| rest);
    format!("...{rest}")
}

/// The gist of a thought: its first sentence, cut at a word if it
/// runs long. Nothing until there is that much of it.
fn gist(text: &str, most: usize) -> Option<String> {
    let plain = text
        .chars()
        .filter(|c| !matches!(c, '*' | '#' | '`' | '_' | '>'))
        .collect::<String>();
    let plain = plain.trim_start();
    let end = plain
        .char_indices()
        .find(|(at, c)| {
            let rest = &plain[at + c.len_utf8()..];
            *c == '\n'
                || (matches!(c, '.' | '?' | '!')
                    && rest.starts_with([' ', '\n']))
        })
        .map(|(at, _)| at);
    let whole = end.is_some();
    let first = plain[..end.unwrap_or(plain.len())].trim();
    let first =
        first.split_whitespace().collect::<Vec<_>>().join(" ");
    let count = first.chars().count();
    if count <= most {
        return (whole && count > 0).then_some(first);
    }
    let cut = first
        .char_indices()
        .nth(most)
        .map_or(first.len(), |(at, _)| at);
    let kept = &first[..cut];
    let kept = kept.rsplit_once(' ').map_or(kept, |(kept, _)| kept);
    Some(format!("{kept}..."))
}

/// The deed of an agent at work, for a thought with no words.
fn deed(doing: Doing) -> Option<&'static str> {
    Some(match doing {
        Doing::Unknown | Doing::Think => return None,
        Doing::Read(Paper::File) => "Reading a file",
        Doing::Read(Paper::Notes) => "Reading its notes",
        Doing::Read(Paper::Skill) => "Reading a skill",
        Doing::Write(Paper::File) => "Writing a file",
        Doing::Write(Paper::Notes) => "Writing its notes",
        Doing::Write(Paper::Skill) => "Writing a skill",
        Doing::Run => "Running a command",
        Doing::Fetch => "Reaching the network",
        Doing::CustomTool => "Using a tool",
        Doing::Await => "Waiting for an answer",
    })
}

/// Takes in what the agents say or think as their turns go.
pub fn hear_speech(
    mut events: MessageReader<AgentsEventReceived>,
    bridge: Res<Bridge>,
    mut talk: ResMut<Talk>,
) {
    for event in events.read() {
        let AgentsEvent::Saying {
            agent,
            text,
            thought,
        } = &event.event
        else {
            continue;
        };
        let Some(agent) = bridge.drawn(agent) else {
            continue;
        };
        let speech =
            talk.speakers.entry(agent).or_insert_with(|| Speech {
                anchor: None,
                said: String::new(),
                thought: false,
                fresh: false,
                quiet: 0.0,
                made: None,
                shown: String::new(),
                shows_thought: false,
                steady: 0.0,
                up: None,
            });
        speech.said.clone_from(text);
        speech.thought = *thought;
        speech.fresh = true;
    }
}

/// Keeps a balloon over every robot whose agent is saying or
/// thinking something, and lets it go a while after the agent falls
/// silent.
pub fn keep_balloons(
    tuning: Res<WorldTuning>,
    clock: Res<Clock>,
    bridge: Res<Bridge>,
    robots: Res<Robots>,
    hand: Res<Hand>,
    mut talk: ResMut<Talk>,
    mut cues: MessageWriter<Cue>,
    mut draw: Draw,
) {
    let set = &tuning.bubbles;
    let dt = clock.dt;
    talk.speakers.retain(|agent, speech| {
        let Some(who) = bridge.agent(*agent).filter(|who| !who.gone)
        else {
            if let Some(anchor) = speech.anchor {
                draw.despawn(anchor);
            }
            return false;
        };
        let pinned = hand.pinned(*agent);
        speech.quiet = if speech.fresh || pinned {
            0.0
        } else {
            speech.quiet + dt
        };
        let linger = if who.busy {
            set.linger_busy
        } else {
            set.linger
        };
        let head = robots.at(*agent).map(|at| {
            at + Vec3::Y * (tuning.pick.robot_box.y + set.lift)
        });
        let Some(head) = head.filter(|_| speech.quiet <= linger)
        else {
            speech.fresh = false;
            if speech.up.take().is_some()
                && let Some(anchor) = speech.anchor
            {
                draw.commands
                    .entity(anchor)
                    .try_remove::<SceneBalloon>();
            }
            return true;
        };
        // Speech is shown as it is said. A thought shows only its
        // subject, or the agent's doing meanwhile. The words are
        // made again only when one of those changed.
        let doing = who.busy.then_some(who.doing);
        let kept = speech
            .made
            .as_ref()
            .is_some_and(|(_, with)| *with == doing && !speech.fresh);
        if !kept {
            let words = if speech.thought {
                gist(&speech.said, set.most_thought)
                    .or_else(|| deed(doing?).map(String::from))
                    .unwrap_or_default()
            } else {
                tail(&speech.said, set.most)
            };
            speech.made = Some((words, doing));
        }
        let made = speech
            .made
            .as_ref()
            .map_or("", |(words, _)| words.as_str());
        // Words stay their while when what follows has none yet, and
        // give way at once to other words.
        speech.steady += dt;
        let held = made.is_empty()
            && !speech.shown.is_empty()
            && speech.steady < set.hold;
        if !held {
            if speech.shown != made {
                speech.shown = made.to_owned();
                speech.steady = 0.0;
            }
            speech.shows_thought = speech.thought;
        }
        speech.fresh = false;
        let anchor =
            *speech.anchor.get_or_insert_with(|| draw.spawn(None, 0));
        draw.place(anchor, Frame::at(head));
        // A pinned thought is held open at any distance.
        let far = if pinned { 0.0 } else { set.far };
        let balloon = SceneBalloon {
            text: speech.shown.clone(),
            thought: speech.shows_thought,
            gap: GAP,
            far,
        };
        if speech.up.as_ref() != Some(&balloon) {
            if speech.up.is_none() {
                cues.write(Cue {
                    sound: if balloon.thought {
                        Sound::Think
                    } else {
                        Sound::Speak
                    },
                    at: head,
                    turn: agent.0,
                });
            }
            draw.commands.entity(anchor).insert(balloon.clone());
            speech.up = Some(balloon);
        }
        true
    });
}
