//! The voices of the music: a kalimba, a music box, a small bell, a
//! bass and a pad, each laid onto two channels, and the room they
//! play in.

use std::f32::consts::TAU;

use super::synth::{DT, RATE, pitch};

/// Two channels of sound.
pub struct Tape {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}

impl Tape {
    pub fn new(seconds: f32) -> Self {
        let n = (seconds * RATE as f32) as usize;
        Self {
            left: vec![0.0; n],
            right: vec![0.0; n],
        }
    }

    pub fn len(&self) -> usize {
        self.left.len()
    }

    pub fn is_empty(&self) -> bool {
        self.left.is_empty()
    }

    /// Adds `other`, `gain` times as loud.
    pub fn mix(&mut self, other: &Tape, gain: f32) {
        let both = self.left.iter_mut().zip(self.right.iter_mut());
        for ((l, r), (a, b)) in
            both.zip(other.left.iter().zip(&other.right))
        {
            *l += a * gain;
            *r += b * gain;
        }
    }

    /// The two channels as one run of samples: left, right, left.
    pub(crate) fn woven(&self) -> Vec<f32> {
        self.left
            .iter()
            .zip(&self.right)
            .flat_map(|(l, r)| [*l, *r])
            .collect::<Vec<_>>()
    }
}

/// Loudness on each side of a sound placed `pan` from left (-1) to
/// right (1).
fn sides(pan: f32) -> (f32, f32) {
    let angle = (pan.clamp(-1.0, 1.0) + 1.0) * TAU / 8.0;
    (angle.cos(), angle.sin())
}

/// An instrument of the music.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Voice {
    /// A plucked tine with a short ring.
    Kalimba,
    /// A music box: it rings longer and a little brighter.
    Box,
    /// A small bell, with the partials of a struck bar.
    Bell,
    /// A low round note.
    Bass,
}

impl Voice {
    /// Seconds a note of `hz` is heard for.
    fn rings(self, hz: f32) -> f32 {
        match self {
            Self::Kalimba => 4.2 * (440.0 / hz).powf(0.35),
            Self::Box => 5.5 * (520.0 / hz).powf(0.45),
            Self::Bell => 6.0,
            Self::Bass => 4.5,
        }
        .min(6.0)
    }

    /// One sample, `t` seconds into a note of `hz`.
    fn sample(self, hz: f32, t: f32) -> f32 {
        let w = TAU * hz * t;
        match self {
            Self::Kalimba => {
                let ring = 0.62 * (440.0 / hz).powf(0.35);
                (t / 0.003).min(1.0)
                    * (w.sin() * (-t / ring).exp()
                        + 0.26
                            * (2.0 * w).sin()
                            * (-t / (ring * 0.5)).exp()
                        + 0.14 * (5.4 * w).sin() * (-t / 0.045).exp())
            }
            Self::Box => {
                let ring = 1.1 * (520.0 / hz).powf(0.45);
                (t / 0.003).min(1.0)
                    * (w.sin() * (-t / ring).exp()
                        + 0.3
                            * (2.0 * w).sin()
                            * (-t / (ring * 0.55)).exp()
                        + 0.12
                            * (3.01 * w).sin()
                            * (-t / (ring * 0.3)).exp()
                        + 0.12 * (5.43 * w).sin() * (-t / 0.06).exp())
            }
            Self::Bell => {
                (t / 0.002).min(1.0)
                    * (w.sin() * (-t / 1.1).exp()
                        + 0.42 * (2.756 * w).sin() * (-t / 0.6).exp()
                        + 0.16
                            * (5.404 * w).sin()
                            * (-t / 0.22).exp())
            }
            Self::Bass => {
                (t / 0.03).min(1.0)
                    * (-t / 0.9).exp()
                    * (w.sin() + 0.25 * (2.0 * w).sin())
            }
        }
    }
}

/// Lays one note onto `tape` at `at` seconds.
pub fn play(
    tape: &mut Tape,
    voice: Voice,
    at: f32,
    note: f32,
    gain: f32,
    pan: f32,
) {
    let hz = pitch(note);
    let (l, r) = sides(pan);
    let first = (at.max(0.0) * RATE as f32) as usize;
    let count = (voice.rings(hz) * RATE as f32) as usize;
    let last = (first + count).min(tape.len());
    for i in first..last {
        let x = gain * voice.sample(hz, (i - first) as f32 * DT);
        tape.left[i] += x * l;
        tape.right[i] += x * r;
    }
}

/// Lays a chord held as a soft pad onto `tape`: it swells for
/// `rise` seconds, holds until `hold`, and takes `fall` to go.
/// Each side is a few cents off the other, so it is wider than it
/// is loud.
pub fn pad(
    tape: &mut Tape,
    at: f32,
    (rise, hold, fall): (f32, f32, f32),
    notes: &[(f32, f32)],
) {
    let first = (at.max(0.0) * RATE as f32) as usize;
    let count = ((hold + fall) * RATE as f32) as usize;
    let last = (first + count).min(tape.len());
    for (k, &(note, gain)) in notes.iter().enumerate() {
        let f = pitch(note);
        let sides = [f * 1.0028, f / 1.0028];
        let drift = 0.11 + 0.04 * k as f32;
        for i in first..last {
            let t = (i - first) as f32 * DT;
            let swell = (t / rise).min(1.0);
            let swell = swell * swell * (3.0 - 2.0 * swell);
            let held = if t < hold {
                1.0
            } else {
                let u = (1.0 - (t - hold) / fall).max(0.0);
                u * u * (3.0 - 2.0 * u)
            };
            let level = gain * swell * held;
            let voice = |f: f32, side: f32| {
                let x = TAU * f * t;
                let breath =
                    1.0 + 0.12 * (TAU * drift * t + side).sin();
                breath
                    * (x.sin()
                        + 0.22 * (2.0 * x).sin()
                        + 0.06 * (3.0 * x).sin())
            };
            tape.left[i] += level * voice(sides[0], 0.0);
            tape.right[i] += level * voice(sides[1], 1.7);
        }
    }
}

/// Echoes of `dry` that bounce from one side to the other, `gap`
/// seconds apart.
pub fn bounce(dry: &Tape, gap: f32, feedback: f32) -> Tape {
    let mut wet = Tape::new(dry.len() as f32 * DT);
    let d = ((gap * RATE as f32) as usize).max(1);
    for i in d..wet.len() {
        let mono = 0.5 * (dry.left[i - d] + dry.right[i - d]);
        wet.left[i] = mono + feedback * wet.right[i - d];
        wet.right[i] = feedback * wet.left[i - d];
    }
    wet
}

/// The room around `dry`: its echoes alone, a little different on
/// each side.
pub fn room(dry: &Tape) -> Tape {
    let mut wet = Tape::new(dry.len() as f32 * DT);
    let combs = [1125usize, 1067, 1369, 1501, 1245, 1625];
    for (side, spread) in [(0, 0), (1, 25)] {
        let (input, output) = if side == 0 {
            (&dry.left, &mut wet.left)
        } else {
            (&dry.right, &mut wet.right)
        };
        // Each line holds its samples and what the loop has damped.
        let mut lines = combs
            .iter()
            .map(|&n| (vec![0.0f32; n + spread], 0.0f32))
            .collect::<Vec<_>>();
        let mut smears =
            [vec![0.0f32; 231 + spread], vec![0.0f32; 75 + spread]];
        for (i, s) in input.iter().enumerate() {
            let mut rung = 0.0;
            for (line, damp) in &mut lines {
                let at = i % line.len();
                let back = line[at];
                *damp = back * 0.6 + *damp * 0.4;
                line[at] = s + *damp * 0.86;
                rung += back;
            }
            let mut x = rung / combs.len() as f32;
            for smear in &mut smears {
                let at = i % smear.len();
                let held = smear[at];
                let out = held - 0.5 * x;
                smear[at] = x + 0.5 * out;
                x = out;
            }
            output[i] = x;
        }
    }
    wet
}

/// Takes the top off `tape`, above `cut` hertz, and fades its two
/// ends so that it begins and ends in silence.
pub(crate) fn soften(
    tape: &mut Tape,
    cut: f32,
    (fade_in, fade_out): (f32, f32),
) {
    let k = 1.0 - (-TAU * cut * DT).exp();
    let n = tape.len();
    let (rise, fall) =
        (fade_in * RATE as f32, fade_out * RATE as f32);
    for side in [&mut tape.left, &mut tape.right] {
        let (mut a, mut b) = (0.0f32, 0.0f32);
        for (i, s) in side.iter_mut().enumerate() {
            a += k * (*s - a);
            b += k * (a - b);
            let level = (i as f32 / rise).min(1.0)
                * ((n - 1 - i) as f32 / fall).min(1.0);
            *s = b * level;
        }
    }
}
