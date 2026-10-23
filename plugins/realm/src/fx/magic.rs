//! The realm's small magic: runes under a house that rises, embers
//! over one that falls, motes from one that goes over to another or
//! is set down elsewhere, and a light over a thinking agent's house
//! that can be clicked.

use std::collections::HashMap;

use glam::{Vec3, Vec3Swizzles};
use hamlet_engine_bevy::api::agents::Doing;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::{
    AlphaMode, Material, Shader, StandardMaterial,
};

use crate::build::frame::Frame;
use crate::build::mark::{self, Mark, Shape};
use crate::build::stock::glow;
use crate::build::upload::Draw;
use crate::folk::keeper::{Floor, KState};
use crate::fx::motes::{Cloud, Look, Motes, Way};
use crate::fx::ready;
use crate::places::shift::Part;
use crate::realm::{Clock, Now};
use crate::society::{AgentId, Society};
use crate::tuning::VfxTuning;
use crate::world::land::Land;
use crate::world::layout::plot;
use crate::world::palette::Palette;
use crate::world::sky::Quality;

pub(crate) fn plugin(app: &mut App) {
    app.add_message::<Shifted>()
        .add_message::<Nudged>()
        .add_systems(
            Update,
            (
                make.run_if(not(resource_exists::<Magic>)),
                (charm, cast, muse)
                    .chain()
                    .run_if(resource_exists::<Magic>),
            )
                .run_if(ready),
        );
}

/// The realm's small magic.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MagicSettings {
    /// The reach of a rune circle and of the embers' ground, and how
    /// far over the ground both lie.
    pub radius: f32,
    pub lift: f32,
    /// Seconds a rune circle lasts, and how bright it is.
    pub rune_time: f32,
    pub rune_light: f32,
    /// Number of embers a falling house gives off where the realm
    /// has no sparks, the seconds over which they set out, and the
    /// seconds each lasts.
    pub embers: u32,
    pub ember_spread: f32,
    pub ember_time: f32,
    /// An ember's climb, its stray, its size and its brightness.
    pub ember_rise: f32,
    pub ember_sway: f32,
    pub ember_size: f32,
    pub ember_light: f32,
    /// The motes of each house of a town that is set down somewhere
    /// else: how many, the seconds they are on their way, and how
    /// high their arc is for the distance.
    pub move_motes: u32,
    pub move_time: f32,
    pub move_arc: f32,
    /// The motes of a house that is merged away: how many, the
    /// seconds they take to cross, how high they arc and how far
    /// they spread on the way, how high over the other house they
    /// land, and their size.
    pub motes: u32,
    pub flight_time: f32,
    pub flight_arc: f32,
    pub flight_spread: f32,
    pub flight_height: f32,
    pub mote_size: f32,
    /// The light over a thinking agent's house: how high, how big,
    /// how bright, how far and how fast it bobs, and the seconds it
    /// takes to grow.
    pub musing_height: f32,
    pub musing_size: f32,
    pub musing_light: f32,
    pub musing_bob: f32,
    pub musing_rate: f32,
    pub musing_in: f32,
}

/// A house began its part in a change, at the clock time `began`,
/// standing at `from`.
#[derive(Message, Clone, Copy, Debug)]
pub struct Shifted {
    pub agent: AgentId,
    pub part: Part,
    pub began: f32,
    pub from: Frame,
}

/// The light over the house of `agent`, thinking, was clicked where
/// it shines, at `at`.
#[derive(Message, Clone, Copy, Debug)]
pub struct Nudged {
    pub agent: AgentId,
    pub at: Vec3,
}

/// The magic in play, and what it is drawn with.
#[derive(Resource)]
struct Magic {
    marks: Handle<Shader>,
    /// The light of a thought, and the room about it, which draws
    /// nothing.
    musing: Handle<Material>,
    room: Handle<Material>,
    circles: Vec<Circle>,
    clouds: Vec<Play>,
    muses: HashMap<AgentId, Muse>,
}

/// A circle of runes: a ring and a six-sided rune turning against it.
struct Circle {
    began: f32,
    at: Vec3,
    light: Vec3,
    runes: Vec<(Mark, f32)>,
}

/// Motes that set off at `began` and are gone at `ends`, once they
/// are in the scene.
struct Play {
    look: Look,
    at: Vec3,
    began: f32,
    ends: f32,
    cloud: Option<Cloud>,
}

/// The light over a thinking agent's house.
struct Muse {
    began: f32,
    entity: Entity,
    room: Entity,
    /// True while the pointer is on its room.
    over: bool,
}

fn make(tuning: Res<VfxTuning>, pal: Res<Palette>, mut draw: Draw) {
    let set = &tuning.magic;
    let made = (|| -> Result<Magic, String> {
        Ok(Magic {
            marks: mark::shader(&mut draw)?,
            musing: draw
                .material(glow(pal.rune, set.musing_light))?,
            room: draw.material(StandardMaterial {
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                ..StandardMaterial::colored((0.0, 0.0, 0.0, 0.0))
            })?,
            circles: Vec::new(),
            clouds: Vec::new(),
            muses: HashMap::new(),
        })
    })();
    match made {
        Ok(magic) => draw.commands.insert_resource(magic),
        Err(why) => {
            hamlet_engine_bevy::warn(&format!("magic: {why}"))
        }
    }
}

/// The place a house of `agent` stands on now.
fn home(
    land: &Land,
    society: &Society,
    agent: AgentId,
) -> Option<Vec3> {
    let agent =
        society.agent(agent).filter(|agent| !agent.archived)?;
    let town = society.town(agent.town)?;
    Some(land.on_ground(plot(land, town.site, agent.plot).at))
}

/// Gives each house that began to come or go its magic: runes under
/// one that rises, embers over one that falls, motes from one that
/// goes over to another or is set down somewhere else.
fn charm(
    mut shifted: MessageReader<Shifted>,
    vfx: Res<VfxTuning>,
    pal: Res<Palette>,
    quality: Res<Quality>,
    land: Res<Land>,
    society: Res<Society>,
    mut magic: ResMut<Magic>,
) {
    let set = &vfx.magic;
    let light = pal.rune * set.rune_light;
    let flying = |from: Vec3,
                  to: Vec3,
                  count: u32,
                  time: f32,
                  arc: f32,
                  began: f32| {
        let at = (from + to) * 0.5;
        Play {
            look: Look {
                colour: pal.rune * set.musing_light,
                way: Way::Flight,
                count,
                reach: from.distance(to) * 0.5
                    + arc
                    + set.radius
                    + set.flight_spread,
                salt: began.rem_euclid(1000.0),
                own: [
                    from.x - at.x,
                    from.y - at.y,
                    from.z - at.z,
                    set.radius,
                    to.x - at.x,
                    to.y - at.y,
                    to.z - at.z,
                    time,
                    arc,
                    set.flight_spread,
                    set.mote_size,
                    0.0,
                ],
                added: true,
            },
            at,
            began,
            ends: began + time,
            cloud: None,
        }
    };
    for shift in shifted.read() {
        let (began, ground) =
            (shift.began, shift.from.at + Vec3::Y * set.lift);
        match shift.part {
            Part::Rise => magic.circle(began, ground, light),
            Part::Fall => {
                // A house that is merged away goes over to the one
                // that takes it in, and a circle opens there as it
                // lands.
                let host = society
                    .agent(shift.agent)
                    .and_then(|gone| gone.into)
                    .and_then(|host| home(&land, &society, host));
                if let Some(host) = host {
                    let to = host + Vec3::Y * set.flight_height;
                    let play = flying(
                        ground,
                        to,
                        set.motes,
                        set.flight_time,
                        set.flight_arc,
                        began,
                    );
                    magic.clouds.push(play);
                    magic.circle(
                        began + set.flight_time,
                        host + Vec3::Y * set.lift,
                        light,
                    );
                    continue;
                }
                let salt = began.rem_euclid(1000.0);
                let play =
                    match vfx.sparks.embers(&pal, *quality, salt) {
                        Some(look) => Play {
                            look,
                            at: ground,
                            began,
                            ends: began + vfx.sparks.ember_life + 0.5,
                            cloud: None,
                        },
                        // A few plain embers stand in where the realm
                        // has no sparks.
                        None => Play {
                            look: Look {
                                colour: pal.ember * set.ember_light,
                                way: Way::Rise,
                                count: set.embers,
                                reach: set.radius
                                    + set.ember_rise
                                    + set.ember_sway,
                                salt,
                                own: [
                                    set.radius * 0.7,
                                    set.ember_spread,
                                    set.ember_time,
                                    set.ember_rise,
                                    set.ember_sway,
                                    set.ember_size,
                                    0.0,
                                    0.0,
                                    0.0,
                                    0.0,
                                    0.0,
                                    0.0,
                                ],
                                added: true,
                            },
                            at: ground,
                            began,
                            ends: began
                                + set.ember_spread
                                + set.ember_time,
                            cloud: None,
                        },
                    };
                magic.clouds.push(play);
            }
            Part::Clear => {
                // The house of a town that was set down somewhere
                // else goes over to where it now stands.
                let Some(to) = home(&land, &society, shift.agent)
                    .filter(|to| to.xz() != shift.from.at.xz())
                else {
                    continue;
                };
                let far = to.distance(ground);
                let play = flying(
                    ground,
                    to + Vec3::Y * set.flight_height,
                    set.move_motes,
                    set.move_time,
                    set.flight_arc.max(far * set.move_arc),
                    began,
                );
                magic.clouds.push(play);
            }
            Part::Rest => {}
        }
    }
}

impl Magic {
    /// A circle of runes that opens at `began` at `at`.
    fn circle(&mut self, began: f32, at: Vec3, light: Vec3) {
        self.circles.push(Circle {
            began,
            at,
            light,
            runes: Vec::new(),
        });
    }
}

/// Plays the runes and the motes, and clears each away at its end.
fn cast(
    vfx: Res<VfxTuning>,
    clock: Res<Clock>,
    land: Res<Land>,
    now: Res<Now>,
    motes: Res<Motes>,
    mut magic: ResMut<Magic>,
    mut draw: Draw,
) {
    let set = &vfx.magic;
    let t = clock.now;
    let magic = &mut *magic;
    let floor = Floor::new(&land, &now.lots);
    let marks = magic.marks.clone();
    magic.circles.retain_mut(|circle| {
        let u = (t - circle.began) / set.rune_time;
        if u >= 1.0 {
            for (rune, _) in circle.runes.drain(..) {
                rune.despawn(&mut draw);
            }
            return false;
        }
        if u < 0.0 {
            return true;
        }
        if circle.runes.is_empty() {
            for (shape, turn) in
                [(Shape::RING, 1.0), (Shape::HEX, -1.6)]
            {
                match Mark::make(0, &marks, shape, &mut draw) {
                    Ok(mut rune) => {
                        rune.settle(
                            circle.at,
                            set.radius * 1.1,
                            &floor,
                            &mut draw,
                        );
                        rune.show(true, &mut draw);
                        circle.runes.push((rune, turn));
                    }
                    Err(why) => {
                        hamlet_engine_bevy::warn(&format!(
                            "runes: {why}"
                        ));
                        return false;
                    }
                }
            }
        }
        // It opens fast, holds, and is gone as the house settles.
        let open = 1.0 - (1.0 - (u * 3.0).min(1.0)).powi(3);
        let fade = (1.0 - u).powf(1.5) * (u * 8.0).min(1.0);
        for (rune, turn) in &mut circle.runes {
            rune.hand_over(&mut draw);
            rune.lay(
                circle.at,
                set.radius * (0.55 + 0.45 * open),
                u * *turn,
                &mut draw,
            );
            rune.shine(circle.light, fade, &draw);
        }
        true
    });
    magic.clouds.retain_mut(|play| {
        if t >= play.ends {
            if let Some(cloud) = play.cloud.take() {
                draw.despawn(cloud.entity);
            }
            return false;
        }
        let age = t - play.began;
        if age < 0.0 {
            return true;
        }
        match &mut play.cloud {
            Some(cloud) => cloud.age(age, &draw),
            None => match motes
                .cloud(play.look, play.at, age, &mut draw)
            {
                Ok(cloud) => play.cloud = Some(cloud),
                Err(why) => {
                    hamlet_engine_bevy::warn(&format!(
                        "motes: {why}"
                    ));
                    return false;
                }
            },
        }
        true
    });
}

/// Keeps a small light over the house of every agent that waits on
/// its model, swelling under the pointer, and puts it out when the
/// thought is over. A click on it is told as [`Nudged`].
fn muse(
    vfx: Res<VfxTuning>,
    clock: Res<Clock>,
    land: Res<Land>,
    society: Res<Society>,
    motes: Res<Motes>,
    mut over: MessageReader<SceneOver>,
    mut out: MessageReader<SceneOut>,
    mut clicks: MessageReader<SceneClick>,
    mut nudged: MessageWriter<Nudged>,
    mut magic: ResMut<Magic>,
    mut draw: Draw,
) {
    let set = &vfx.magic;
    let magic = &mut *magic;
    for hit in over.read() {
        for muse in magic.muses.values_mut() {
            muse.over |= muse.room == hit.0.entity;
        }
    }
    for gone in out.read() {
        for muse in magic.muses.values_mut() {
            muse.over &= muse.room != gone.entity;
        }
    }
    let thinking = |agent: AgentId| {
        society.agent(agent).is_some_and(|agent| {
            !agent.archived
                && agent.work == Doing::Think
                && agent.state == KState::Working
        })
    };
    for click in clicks.read() {
        if let Some((agent, _)) = magic
            .muses
            .iter()
            .find(|(_, muse)| muse.room == click.0.entity)
        {
            nudged.write(Nudged {
                agent: *agent,
                at: click.0.position,
            });
        }
    }
    magic.muses.retain(|agent, muse| {
        let place = home(&land, &society, *agent)
            .filter(|_| thinking(*agent));
        let Some(place) = place else {
            draw.despawn(muse.entity);
            return false;
        };
        let t = clock.now - muse.began;
        let bob = (t * set.musing_rate).sin() * set.musing_bob;
        let breath = 1.0 + 0.18 * (t * set.musing_rate * 2.3).sin();
        let grown = (t / set.musing_in).clamp(0.0, 1.0);
        // It swells under the pointer, to say it can be clicked.
        let swell = if muse.over { 1.5 } else { 1.0 };
        draw.place(
            muse.entity,
            Frame::at(place + Vec3::Y * (set.musing_height + bob))
                .with_size(Vec3::splat(
                    set.musing_size * breath * grown * swell,
                )),
        );
        true
    });
    for agent in &society.agents {
        if !thinking(agent.id)
            || magic.muses.contains_key(&agent.id)
            || home(&land, &society, agent.id).is_none()
        {
            continue;
        }
        let entity = draw.spawn(None, 0);
        draw.place(
            entity,
            Frame::at(Vec3::ZERO).with_size(Vec3::ZERO),
        );
        draw.draw(entity, motes.ball(), &magic.musing);
        draw.shadows(entity, false, false);
        let room = draw.spawn(Some(entity), 0);
        draw.place(
            room,
            Frame::IDENTITY.with_size(Vec3::splat(vfx.nudge_room)),
        );
        draw.draw(room, motes.ball(), &magic.room);
        draw.shadows(room, false, false);
        draw.watch(room);
        magic.muses.insert(
            agent.id,
            Muse {
                began: clock.now,
                entity,
                room,
                over: false,
            },
        );
    }
}
