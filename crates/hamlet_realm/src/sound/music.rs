//! The music, made a breath at a time: one chord held as a pad with a
//! few plucked notes over it, in D pentatonic. Each breath begins and
//! ends in silence and overlaps the one before.

pub(super) mod band;
#[cfg(test)]
mod tests;

use super::Sound;
use super::synth::Hiss;
use super::voices::{Tape, Voice, bounce, pad, play, room, soften};
use crate::DayPart;
use crate::meshes::lerp;

/// The hour and the bustle the music is played to.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Mood {
    pub part: DayPart,
    /// Share of the hamlet at work, 0 to 1.
    pub busy: f32,
    /// Seconds of rest added after every breath.
    pub rest: f32,
}

impl Mood {
    /// Amount of daylight, 0 to 1.
    fn bright(&self) -> f32 {
        match self.part {
            DayPart::Day => 1.0,
            DayPart::Dawn => 0.6,
            DayPart::Dusk => 0.4,
            DayPart::Night => 0.0,
        }
    }
}

/// Something the music answers with a phrase.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Moment {
    /// A house has risen and settled.
    Rise,
    /// An agent has come back.
    Return,
    /// Something waits for the chief.
    Ask,
    /// The chief said yes.
    Answer,
    /// A house fell, or the chief said no.
    Fall,
    /// A merge has landed.
    Join,
    /// A hamlet was founded.
    Found,
    Dawn,
    Dusk,
}

impl Moment {
    #[cfg(test)]
    pub const ALL: [Self; 9] = [
        Self::Rise,
        Self::Return,
        Self::Ask,
        Self::Answer,
        Self::Fall,
        Self::Join,
        Self::Found,
        Self::Dawn,
        Self::Dusk,
    ];

    /// The moment a sound of the world marks, if it marks one.
    pub fn of(sound: Sound) -> Option<Self> {
        Some(match sound {
            Sound::HouseSettle => Self::Rise,
            Sound::Restored => Self::Return,
            Sound::Chime => Self::Ask,
            Sound::Allowed => Self::Answer,
            Sound::HouseFall | Sound::Denied => Self::Fall,
            Sound::MergeLand => Self::Join,
            Sound::Founded => Self::Found,
            Sound::Dawn => Self::Dawn,
            Sound::Dusk => Self::Dusk,
            _ => return None,
        })
    }
}

/// A chord: its notes low to high, its bass, and how well it suits
/// the day and the night.
struct Chord {
    tones: &'static [f32],
    root: f32,
    day: f32,
    night: f32,
}

/// The chords of the two films. The first is home.
const CHORDS: [Chord; 9] = [
    Chord {
        tones: &[50.0, 57.0, 64.0, 66.0],
        root: 38.0,
        day: 1.0,
        night: 0.5,
    },
    Chord {
        tones: &[50.0, 57.0, 62.0, 66.0, 69.0],
        root: 38.0,
        day: 0.8,
        night: 0.1,
    },
    Chord {
        tones: &[55.0, 59.0, 62.0, 66.0],
        root: 43.0,
        day: 1.0,
        night: 0.5,
    },
    Chord {
        tones: &[57.0, 64.0, 66.0, 71.0],
        root: 45.0,
        day: 0.9,
        night: 0.1,
    },
    Chord {
        tones: &[59.0, 62.0, 66.0, 69.0],
        root: 47.0,
        day: 0.6,
        night: 0.6,
    },
    Chord {
        tones: &[55.0, 59.0, 62.0, 69.0],
        root: 43.0,
        day: 0.7,
        night: 0.2,
    },
    Chord {
        tones: &[57.0, 62.0, 64.0, 69.0],
        root: 45.0,
        day: 0.7,
        night: 0.2,
    },
    // The minor turn of the story's quiet road.
    Chord {
        tones: &[47.0, 54.0, 59.0, 62.0],
        root: 35.0,
        day: 0.1,
        night: 1.0,
    },
    Chord {
        tones: &[52.0, 55.0, 59.0, 64.0],
        root: 40.0,
        day: 0.1,
        night: 0.7,
    },
];

/// The note `i` steps up the pentatonic scale from the D above
/// middle C.
fn degree(i: i32) -> f32 {
    let steps = [0.0, 2.0, 4.0, 7.0, 9.0];
    62.0 + 12.0 * i.div_euclid(5) as f32
        + steps[i.rem_euclid(5) as usize]
}

/// One note of a breath.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Note {
    pub voice: Voice,
    /// Seconds from the start of the breath.
    pub at: f32,
    /// MIDI number.
    pub note: f32,
    pub gain: f32,
    pub pan: f32,
}

/// A few seconds of music as written.
#[derive(Clone, PartialEq, Debug)]
pub struct Breath {
    /// The notes of the pad and how loud each is.
    pub pads: Vec<(f32, f32)>,
    /// Seconds the pad takes to swell, the second it begins to go,
    /// and the seconds it takes to go.
    pub shape: (f32, f32, f32),
    pub notes: Vec<Note>,
    /// Seconds of sound, with every tail.
    pub long: f32,
    /// Seconds from its start to the start of the next.
    pub stride: f32,
    /// Hertz above which it is dulled.
    pub cut: f32,
}

/// Writes the music, one breath after another. The same seed and
/// the same moods give the same music.
pub struct Composer {
    hiss: Hiss,
    chord: usize,
    /// The tune's place on the scale.
    at: i32,
    breaths: u32,
    /// Breaths since the theme was last hinted at, and since the
    /// harmony was last home.
    since_theme: u32,
    since_home: u32,
}

impl Composer {
    pub fn new(seed: u32) -> Self {
        Self {
            hiss: Hiss(seed | 1),
            chord: 0,
            at: 6,
            breaths: 0,
            since_theme: 4,
            since_home: 0,
        }
    }

    fn unit(&mut self) -> f32 {
        self.hiss.unit()
    }

    /// The chord after this one: another, by how well each suits
    /// the light.
    fn turn(&mut self, bright: f32) -> usize {
        let now = self.chord;
        let weight = |k: usize| {
            if k == now {
                0.0
            } else {
                lerp(CHORDS[k].night, CHORDS[k].day, bright)
            }
        };
        let total = (0..CHORDS.len()).map(weight).sum::<f32>();
        let mut pick = self.unit() * total;
        for k in 0..CHORDS.len() {
            pick -= weight(k);
            if pick <= 0.0 {
                return k;
            }
        }
        0
    }

    /// Moves the tune a step or two, and keeps it near the middle
    /// of its range for the light.
    fn step(&mut self, bright: f32) {
        let middle = lerp(4.0, 8.0, bright) as i32;
        let steps = [-2, -1, -1, 1, 1, 2];
        let mut by = steps[(self.unit() * 6.0) as usize % 6];
        if (self.at + by - middle).abs() > 3 {
            by = -by;
        }
        self.at += by;
    }

    /// Moves the tune onto the nearest note of the chord.
    fn settle(&mut self) {
        let chord = &CHORDS[self.chord];
        let fits = |i: i32| {
            chord
                .tones
                .iter()
                .any(|tone| (degree(i) - tone).rem_euclid(12.0) < 0.5)
        };
        if let Some(by) =
            [0, -1, 1].into_iter().find(|by| fits(self.at + by))
        {
            self.at += by;
        }
    }

    /// The next breath.
    pub fn breathe(&mut self, mood: &Mood) -> Breath {
        let bright = mood.bright();
        let busy = mood.busy.clamp(0.0, 1.0);
        self.breaths += 1;
        self.since_theme += 1;
        let theme = self.since_theme >= 9 && self.unit() < 0.2;
        self.chord = if theme || self.since_home >= 4 {
            0
        } else {
            self.turn(bright)
        };
        self.since_home = if self.chord == 0 {
            0
        } else {
            self.since_home + 1
        };
        let chord = &CHORDS[self.chord];

        let hold = lerp(14.0, 10.0, bright) + 3.0 * self.unit();
        let rise = 3.0 + self.unit();
        let fall = 4.0 + 1.5 * self.unit();
        // The quieter the hamlet and the darker the hour, the more
        // silence between two breaths.
        let rest = mood.rest
            + lerp(8.0, 2.5, bright)
            + (1.0 - busy) * lerp(12.0, 8.0, bright) * self.unit();
        let mut breath = Breath {
            pads: Vec::new(),
            shape: (rise, hold, fall),
            notes: Vec::new(),
            long: hold + 6.5,
            stride: hold - 1.5 + rest,
            cut: lerp(2400.0, 7000.0, bright),
        };

        let level =
            0.09 * lerp(0.7, 1.0, bright) / chord.tones.len() as f32;
        breath.pads.extend(chord.tones.iter().map(|&t| (t, level)));
        if bright < 0.5 {
            breath.pads.push((chord.root, 0.02));
        }

        if theme {
            self.since_theme = 0;
            self.hint(&mut breath, bright, hold);
        } else {
            self.tune(&mut breath, bright, busy, hold);
            self.ripple(&mut breath, bright, busy, hold);
        }

        let grounded = bright > 0.3 && busy > 0.15;
        if grounded || self.unit() < 0.25 {
            breath.notes.push(Note {
                voice: Voice::Bass,
                at: 0.6,
                note: chord.root,
                gain: 0.06,
                pan: 0.0,
            });
        }
        if self.unit() < lerp(0.3, 0.12, bright) {
            // The top of the chord, in the octave of small bells.
            let top = chord.tones[chord.tones.len() - 1];
            let note = top + 12.0 * ((93.0 - top) / 12.0).floor();
            let at = 2.0 + self.unit() * (hold - 5.0);
            let pan = 0.7 * self.unit() - 0.35;
            breath.notes.push(Note {
                voice: Voice::Bell,
                at,
                note,
                gain: 0.035,
                pan,
            });
        }
        breath
    }

    /// A few gestures of one to three plucked notes, with space
    /// between them.
    fn tune(
        &mut self,
        breath: &mut Breath,
        bright: f32,
        busy: f32,
        hold: f32,
    ) {
        let expect =
            lerp(0.5, 1.0, bright) + busy * lerp(0.4, 1.1, bright);
        let count = (expect + self.unit()) as usize;
        let (from, to) = (1.5, hold - 2.5);
        let slot = (to - from) / count.max(1) as f32;
        for g in 0..count {
            let mut at =
                from + slot * (g as f32 + 0.15 + 0.5 * self.unit());
            let notes = (1
                + (self.unit() * (1.4 + 1.2 * busy)) as usize)
                .min(3);
            for k in 0..notes {
                self.step(bright);
                if k + 1 == notes {
                    self.settle();
                }
                let hit = 0.45 + 0.3 * self.unit();
                // The tune wanders a little from side to side.
                let turn = self.breaths as usize * 8 + g * 3 + k;
                let pan = 0.45 * (turn as f32 * 1.9).sin();
                let note = degree(self.at);
                // The lydian fourth, now and then, leaning on the
                // fifth it leads to.
                let fifth = note.rem_euclid(12.0) == 9.0;
                if fifth && bright > 0.3 && self.unit() < 0.2 {
                    breath.notes.push(Note {
                        voice: Voice::Kalimba,
                        at: at - 0.25,
                        note: note - 1.0,
                        gain: 0.07 * hit,
                        pan,
                    });
                }
                breath.notes.push(Note {
                    voice: Voice::Kalimba,
                    at: at + 0.012 * self.unit(),
                    note,
                    gain: 0.14 * hit,
                    pan,
                });
                if bright > 0.5 && hit > 0.68 {
                    breath.notes.push(Note {
                        voice: Voice::Kalimba,
                        at,
                        note: note - 12.0,
                        gain: 0.045,
                        pan: -pan,
                    });
                }
                at += 0.5 * (1.0 + (self.unit() * 2.0).floor());
            }
        }
    }

    /// A little motion while the hamlet is at work: the chord
    /// broken high on the music box, coming and going.
    fn ripple(
        &mut self,
        breath: &mut Breath,
        bright: f32,
        busy: f32,
        hold: f32,
    ) {
        if busy <= 0.3
            || self.unit() >= busy * lerp(0.12, 0.3, bright)
        {
            return;
        }
        let tones = CHORDS[self.chord].tones;
        let start = 2.0 + self.unit() * (hold - 9.0).max(0.0);
        let count = 4 + (self.unit() * 3.0) as usize;
        for k in 0..count {
            let tone =
                tones[[1, 2, 3, 2][k % 4].min(tones.len() - 1)];
            // It comes in from nothing and goes back to it.
            let swell = ((k as f32 + 0.5) / count as f32
                * std::f32::consts::PI)
                .sin();
            breath.notes.push(Note {
                voice: Voice::Box,
                at: start + k as f32 * 0.5 + 0.25,
                note: tone + 12.0,
                gain: 0.035 * swell,
                pan: if k % 2 == 0 { 0.55 } else { -0.55 },
            });
        }
    }

    /// The opening of the story's theme, alone: three notes by
    /// day, and at night only the two that ask.
    fn hint(&mut self, breath: &mut Breath, bright: f32, hold: f32) {
        let start = 2.5 + self.unit() * (hold - 7.0).max(0.0);
        let notes: &[(f32, f32, f32)] = if bright > 0.5 {
            &[(0.0, 78.0, 0.12), (0.5, 81.0, 0.12), (1.0, 86.0, 0.13)]
        } else {
            &[(0.0, 78.0, 0.1), (1.0, 81.0, 0.1)]
        };
        for &(at, note, gain) in notes {
            breath.notes.push(Note {
                voice: Voice::Box,
                at: start + at,
                note,
                gain,
                pan: -0.18,
            });
        }
        self.at = 8;
    }

    /// A short phrase for a moment, in the chord of the breath
    /// that is playing.
    pub fn answer(&mut self, moment: Moment, mood: &Mood) -> Breath {
        use Voice::{Bell, Box as MusicBox, Kalimba};
        let tones = CHORDS[self.chord].tones;
        let up = |k: usize| tones[k.min(tones.len() - 1)] + 12.0;
        let notes: Vec<(Voice, f32, f32, f32)> = match moment {
            Moment::Rise => vec![
                (Kalimba, 0.0, up(1), 0.16),
                (Kalimba, 0.22, up(2), 0.17),
                (Kalimba, 0.44, up(3), 0.19),
            ],
            Moment::Return => vec![
                (MusicBox, 0.0, 81.0, 0.17),
                (MusicBox, 0.25, 78.0, 0.16),
                (MusicBox, 0.5, 74.0, 0.19),
            ],
            // The theme's first two notes, asked and not answered.
            Moment::Ask => {
                vec![
                    (MusicBox, 0.0, 78.0, 0.17),
                    (MusicBox, 0.5, 81.0, 0.17),
                ]
            }
            Moment::Answer => vec![
                (MusicBox, 0.0, 81.0, 0.17),
                (MusicBox, 0.3, 86.0, 0.19),
                (Kalimba, 0.32, 74.0, 0.13),
            ],
            Moment::Fall => {
                vec![
                    (Kalimba, 0.0, 74.0, 0.14),
                    (Kalimba, 0.4, 71.0, 0.13),
                ]
            }
            Moment::Join => vec![
                (Kalimba, 0.0, 69.0, 0.14),
                (Kalimba, 0.02, 78.0, 0.14),
                (Kalimba, 0.36, 74.0, 0.17),
            ],
            Moment::Found => vec![
                (Kalimba, 0.0, 62.0, 0.16),
                (Kalimba, 0.09, 66.0, 0.16),
                (Kalimba, 0.18, 69.0, 0.16),
                (Kalimba, 0.27, 74.0, 0.18),
                (Bell, 0.42, 86.0, 0.06),
            ],
            Moment::Dawn => vec![
                (MusicBox, 0.0, 74.0, 0.15),
                (MusicBox, 0.3, 78.0, 0.15),
                (MusicBox, 0.6, 81.0, 0.16),
                (Bell, 0.9, 88.0, 0.05),
            ],
            Moment::Dusk => vec![
                (MusicBox, 0.0, 81.0, 0.15),
                (MusicBox, 0.35, 78.0, 0.14),
                (MusicBox, 0.7, 76.0, 0.14),
                (Kalimba, 1.05, 71.0, 0.14),
            ],
        };
        let side = 0.6 * self.unit() - 0.3;
        Breath {
            pads: Vec::new(),
            shape: (1.0, 1.0, 1.0),
            notes: notes
                .into_iter()
                .map(|(voice, at, note, gain)| Note {
                    voice,
                    at: at + 0.05,
                    note,
                    gain: gain * 0.7,
                    pan: side,
                })
                .collect::<Vec<_>>(),
            long: 7.0,
            stride: 0.0,
            cut: lerp(2400.0, 7000.0, mood.bright()),
        }
    }
}

/// Plays a breath: the samples of its two channels, left first.
pub fn render(breath: &Breath) -> Vec<f32> {
    let mut dry = Tape::new(breath.long);
    let mut wet = Tape::new(breath.long);
    if !breath.pads.is_empty() {
        let mut pads = Tape::new(breath.long);
        pad(&mut pads, 0.0, breath.shape, &breath.pads);
        dry.mix(&pads, 1.0);
        wet.mix(&pads, 0.35);
    }
    let mut plucks = Tape::new(breath.long);
    let mut bells = Tape::new(breath.long);
    for n in &breath.notes {
        let tape = match n.voice {
            Voice::Kalimba | Voice::Box => &mut plucks,
            Voice::Bell => &mut bells,
            Voice::Bass => &mut dry,
        };
        play(tape, n.voice, n.at, n.note, n.gain, n.pan);
    }
    let echo = bounce(&plucks, 0.75, 0.32);
    dry.mix(&plucks, 1.0);
    dry.mix(&echo, 0.16);
    wet.mix(&plucks, 0.5);
    wet.mix(&echo, 0.3);
    dry.mix(&bells, 1.0);
    wet.mix(&bells, 0.7);
    dry.mix(&room(&wet), 0.55);
    soften(&mut dry, breath.cut, (0.02, 0.8));
    dry.woven()
}
