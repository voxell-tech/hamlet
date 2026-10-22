//! A robot at work about its own house (docs/working-alone.md): it
//! walks to the station of its chore and plays the chore there, and
//! a passage it plays once, on its way into the realm or out of it,
//! holds its work meanwhile.

use std::f32::consts::PI;

use glam::{EulerRot, Quat, Vec3, Vec3Swizzles};
use hamlet_engine_sdk::api::agents::Doing;

use crate::build::frame::Frame;
use crate::folk::keeper::acts::Act;
use crate::folk::keeper::lot::{Lot, footing};
use crate::folk::keeper::rig::Rig;
use crate::folk::keeper::trip::{Stride, Trip};
use crate::folk::keeper::work::{Chore, routine};
use crate::folk::keeper::{KState, Motion};
use crate::places::yard::Station;
use crate::world::land::Land;

/// One chore: the walk to its station, then the chore itself.
#[derive(Debug)]
struct Job {
    chore: Chore,
    /// Clock time it set off.
    since: f32,
    /// Clock time the step that asked for it began.
    began: f32,
    leg: Trip,
    /// The spot it stands at for the chore, and the way it faces.
    to: (Vec3, f32),
}

impl Job {
    fn new(
        chore: Chore,
        since: f32,
        way: Vec<Vec3>,
        to: (Vec3, f32),
        motion: &Motion,
    ) -> Self {
        let mut leg = Trip::new(way, since, motion);
        leg.speed = motion.amble;
        Self {
            chore,
            since,
            began: since,
            leg,
            to,
        }
    }

    /// Clock time it reaches its station.
    fn arrive(&self) -> f32 {
        if self.leg.length() < 0.05 {
            self.since
        } else {
            self.since + self.leg.duration()
        }
    }

    /// True when the chore has been seen for long enough to leave.
    fn shown(&self, now: f32) -> bool {
        let least = self.chore.least();
        least == 0.0 || now >= self.arrive() + least
    }

    /// True when it is real work, to be rounded off when it ends.
    fn works(&self) -> bool {
        !matches!(
            self.chore,
            Chore::Home | Chore::Stand | Chore::Done
        )
    }
}

/// A motion a robot plays once, whatever its work: on its way out of
/// the realm or back into it.
#[derive(Clone, Copy, Debug)]
pub(super) struct Passage {
    act: Act,
    /// Clock time it begins.
    from: f32,
    /// True once it has begun.
    begun: bool,
}

impl Passage {
    /// `act`, begun at clock time `from`.
    pub fn new(act: Act, from: f32) -> Self {
        Self {
            act,
            from,
            begun: false,
        }
    }
}

/// An agent's robot: how it is drawn and what it does.
pub(super) struct Robot {
    pub rig: Rig,
    pub lot: Lot,
    pub state: KState,
    pub work: Doing,
    /// What the house it stands by was built from: when it changes,
    /// the robot is made anew.
    pub sig: (usize, glam::Vec2, u32, u8, bool),
    pub passage: Option<Passage>,
    /// Its doing about its house. None before it has been placed.
    job: Option<Job>,
    /// The motion last asked of it, and the clock time it began.
    playing: Option<(Act, f32)>,
    /// Steps of its agent's work seen, and when the last began.
    step: u32,
    step_at: f32,
    /// A tool it has yet to be seen using.
    queued: Option<Chore>,
    /// True when it has been placed once.
    placed: bool,
    /// The speed of its next walk about its house, when it is in a
    /// hurry.
    haste: Option<f32>,
}

impl Robot {
    pub fn new(
        rig: Rig,
        lot: Lot,
        state: KState,
        work: Doing,
        sig: (usize, glam::Vec2, u32, u8, bool),
    ) -> Self {
        Self {
            rig,
            lot,
            state,
            work,
            sig,
            passage: None,
            job: None,
            playing: None,
            step: 0,
            step_at: 0.0,
            queued: None,
            placed: false,
            haste: None,
        }
    }

    /// Takes up what its agent does now.
    pub fn follow(&mut self, state: KState, work: Doing, now: f32) {
        if state == KState::Working && work != self.work {
            // A new step: a tool is seen in use even if it is over
            // at once.
            self.step += 1;
            self.step_at = now;
            let chore = Chore::of(work);
            if self.placed && chore != Chore::Ponder {
                self.queued = Some(chore);
            }
        }
        if state != KState::Working {
            self.queued = None;
        }
        self.state = state;
        self.work = work;
    }

    /// Forgets the chore it was at, so that it takes its work up
    /// again from wherever it stands.
    fn put_down(&mut self, haste: Option<f32>) {
        self.job = None;
        self.playing = None;
        self.haste = haste;
    }

    /// True while it waits, unseen, for a passage that brings it into
    /// the realm.
    pub fn folded(&self, now: f32) -> bool {
        self.passage.is_some_and(|passage| {
            passage.act == Act::Appear && now < passage.from
        })
    }

    /// Plays its passage. True when it is over and the robot is gone.
    pub fn pass(&mut self, now: f32) -> bool {
        let Some(passage) = &mut self.passage else {
            return false;
        };
        if now < passage.from {
            return false;
        }
        if !passage.begun {
            passage.begun = true;
            self.rig.play(passage.act, passage.from);
        }
        let (act, from) = (passage.act, passage.from);
        if act.length().is_some_and(|l| now - from < l) {
            return false;
        }
        if act.vanishes() {
            return true;
        }
        self.passage = None;
        // Its chore went on without it meanwhile. A way it was still
        // on is taken up again at the speed it had.
        let under_way = self
            .job
            .as_ref()
            .filter(|job| now < job.arrive())
            .map(|job| job.leg.speed);
        let haste = self.haste.or(under_way);
        self.put_down(haste);
        false
    }

    /// The chore it should be at, and the clock time the step behind
    /// it began.
    fn wanted(
        &self,
        seed: u32,
        now: f32,
        live: bool,
    ) -> (Chore, f32) {
        match self.state {
            KState::Working => {
                if self.work == Doing::Unknown {
                    routine(seed, now)
                } else if live && let Some(chore) = self.queued {
                    (chore, self.step_at)
                } else {
                    (Chore::of(self.work), self.step_at)
                }
            }
            KState::Rest => {
                if live && self.job.as_ref().is_some_and(Job::works) {
                    (Chore::Done, now)
                } else {
                    (Chore::Home, now)
                }
            }
            KState::Waiting | KState::Fault | KState::Failed => {
                (Chore::Stand, now)
            }
        }
    }

    /// A job as if the robot had been at it all along, decided by the
    /// clock and the agent alone.
    fn settled(
        &self,
        land: &Land,
        (want, began): (Chore, f32),
        seed: u32,
        motion: &Motion,
    ) -> Job {
        let lot = &self.lot;
        let stand = |chore: Chore, beat: f32| {
            // A chore done wherever the robot is needs a place.
            let station = match chore {
                Chore::Done => Station::Desk,
                Chore::Stand => Station::Porch,
                chore => chore.station(),
            };
            lot.stand(land, station, beat)
                .unwrap_or((lot.porch, lot.yaw))
        };
        let to = stand(want, first_beat(want, seed));
        let by_the_clock = self.state == KState::Working
            && self.work == Doing::Unknown;
        let way = if by_the_clock {
            // The round says what came before, so it walks from
            // there.
            let (before, since) = routine(seed, began - 0.01);
            let spent = (began - since - 2.0).max(0.0);
            let beat = before
                .bit(spent, 0.0, seed, 0)
                .beat
                .map_or(0.0, |b| b.at);
            lot.way(land, stand(before, beat).0, to.0)
        } else {
            vec![to.0]
        };
        Job::new(want, began, way, to, motion)
    }

    /// Has the robot do its chore about its house a frame `dt`
    /// seconds on, at clock time `now`.
    pub fn work(
        &mut self,
        land: &Land,
        motion: &Motion,
        now: f32,
        dt: f32,
    ) {
        if self.passage.is_some() {
            return;
        }
        let seed = self.rig.who().seed;
        // A robot's first moment shows the settled state.
        let live = self.placed;
        self.placed = true;
        if let Some(job) = &self.job
            && now >= job.arrive()
            && job.shown(now)
            && self.queued == Some(job.chore)
        {
            self.queued = None;
        }
        let (want, began) = self.wanted(seed, now, live);
        if !live {
            self.job =
                Some(self.settled(land, (want, began), seed, motion));
        } else if self.job.as_ref().is_none_or(|job| {
            want != job.chore && (want.breaks_in() || job.shown(now))
        }) {
            let from = self.rig.at.at;
            let facing = self.rig.at.turn.to_euler(EulerRot::YXZ).0;
            let lot = self.lot;
            let job = match lot.stand(
                land,
                want.station(),
                first_beat(want, seed),
            ) {
                Some(to) if lot.away(land, from) => {
                    // From far off, as after its town was carried, it
                    // is simply at its door again.
                    self.rig.place(
                        Frame::at(lot.porch).with_turn(
                            Quat::from_rotation_y(lot.yaw),
                        ),
                    );
                    Job::new(
                        want,
                        now,
                        lot.way(land, lot.porch, to.0),
                        to,
                        motion,
                    )
                }
                Some(to) => {
                    let mut job = Job::new(
                        want,
                        now,
                        lot.way(land, from, to.0),
                        to,
                        motion,
                    );
                    if let Some(haste) = self.haste.take() {
                        job.leg.speed = haste;
                    }
                    job
                }
                None => Job::new(
                    want,
                    now,
                    vec![from],
                    (from, facing),
                    motion,
                ),
            };
            self.job = Some(job);
        }
        let Some(job) = self.job.as_mut() else {
            return;
        };
        if want == job.chore {
            job.began = began;
        }
        let job = &*job;
        let arrive = job.arrive();
        let lot = &self.lot;
        let ease = live.then_some(now);
        let (p, yaw) = if job.chore == Chore::Stand {
            // Its state picks the motion.
            self.rig.state = self.state;
            self.rig.stride(Some(Stride::default()));
            self.playing = None;
            job.to
        } else {
            let (p, yaw, act, since, stride) = if now < arrive {
                let s = job.leg.at(now);
                let act = if job.chore == Chore::Home {
                    Act::Walk
                } else {
                    Act::Amble
                };
                // It keeps to the ground, or to the paving of a yard
                // above it.
                let ground = footing(land, s.at.xz());
                let paved = ground + lot.yard.shape.terrace.max(0.0);
                let p = s.at.with_y(s.at.y.clamp(ground, paved));
                (p, s.yaw, act, job.since, s.stride())
            } else {
                let spent = now - job.began.max(arrive);
                let bit = job.chore.bit(
                    now - arrive,
                    spent,
                    seed,
                    self.step,
                );
                let (p, yaw) = match bit.beat {
                    Some(b) => {
                        let (p, out) = lot
                            .stand(land, Station::Beat, b.at)
                            .unwrap_or(job.to);
                        let along = lot.yaw + lot.yard.along();
                        let yaw = if b.heading > 0.0 {
                            along
                        } else if b.heading < 0.0 {
                            along + PI
                        } else {
                            out
                        };
                        (p, yaw)
                    }
                    None => job.to,
                };
                let stride = bit
                    .beat
                    .map_or(Stride::default(), |b| b.stride());
                (p, yaw, bit.act, arrive + bit.since, stride)
            };
            self.rig.stride(Some(stride));
            if self.playing.is_none_or(|(a, s)| {
                a != act || (s - since).abs() > 0.02
            }) {
                self.rig.take_up(act, since, ease);
                self.playing = Some((act, since));
            }
            (p, yaw)
        };
        let to = Quat::from_rotation_y(yaw);
        // A turn takes a moment.
        let turn = if live {
            self.rig
                .at
                .turn
                .slerp(to, 1.0 - (-dt * motion.turn_rate).exp())
        } else {
            to
        };
        self.rig.place(Frame::at(p).with_turn(turn));
    }
}

/// The place on its beat a chore starts at.
fn first_beat(chore: Chore, seed: u32) -> f32 {
    chore.bit(0.0, 0.0, seed, 0).beat.map_or(0.0, |b| b.at)
}
