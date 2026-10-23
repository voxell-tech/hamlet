//! Renders every sound of the world and a few minutes of its music
//! to WAV files, and prints what they measure. Nothing is played:
//! no audio device is opened and no window.
//!
//! ```sh
//! cargo run --release --example sound_sheet            # target/sound-sheet
//! cargo run --release --example sound_sheet -- <dir> [minutes]
//! ```
//!
//! Each effect is written as made, at its own level. The music and
//! the scene are written at the level they play at with the default
//! settings, so the files can be compared by ear as they will sound.

#![expect(clippy::print_stdout, reason = "the sheet is printed")]

use std::path::{Path, PathBuf};

use hamlet_realm::DayPart;
use hamlet_realm::sound::music::{Composer, Moment, Mood, render};
use hamlet_realm::sound::{Family, RATE, Sound, SoundSettings};

fn db(level: f32) -> f32 {
    20.0 * level.max(1e-9).log10()
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn rms(samples: &[f32]) -> f32 {
    let sum = samples.iter().map(|s| s * s).sum::<f32>();
    (sum / samples.len().max(1) as f32).sqrt()
}

const FAMILIES: [Family; 11] = [
    Family::Ambience,
    Family::Steps,
    Family::Work,
    Family::Doors,
    Family::Alerts,
    Family::Interface,
    Family::Hand,
    Family::Magic,
    Family::Talk,
    Family::Lives,
    Family::Answers,
];

/// The level of the loudest stretch of `window` seconds: what a
/// short sound is heard as. A sound shorter than the window is
/// spread over all of it, as the ear spreads it.
fn loudest(samples: &[f32], channels: usize, window: f32) -> f32 {
    let n = ((window * RATE as f32) as usize * channels).max(1);
    samples
        .chunks(n)
        .map(|chunk| {
            let sum = chunk.iter().map(|s| s * s).sum::<f32>();
            (sum / n as f32).sqrt()
        })
        .fold(0.0f32, f32::max)
}

/// The largest step from one sample to the next of each channel.
fn jump(samples: &[f32], channels: usize) -> f32 {
    samples
        .windows(channels + 1)
        .map(|w| (w[channels] - w[0]).abs())
        .fold(0.0f32, f32::max)
}

/// The share of the power in each of four bands: below 250 Hz, to
/// 1 kHz, to 4 kHz, and above.
fn bands(samples: &[f32], channels: usize) -> [f32; 4] {
    let mut power = [0.0f32; 4];
    for channel in 0..channels {
        // Each crossover is four gentle low-passes in a row.
        let mut state = [[0.0f32; 4]; 3];
        for s in samples.iter().skip(channel).step_by(channels) {
            let mut lows = [0.0f32; 3];
            for (k, cut) in
                [250.0f32, 1000.0, 4000.0].iter().enumerate()
            {
                let a = 1.0
                    - (-std::f32::consts::TAU * cut / RATE as f32)
                        .exp();
                let mut x = *s;
                for y in &mut state[k] {
                    *y += a * (x - *y);
                    x = *y;
                }
                lows[k] = x;
            }
            let split = [
                lows[0],
                lows[1] - lows[0],
                lows[2] - lows[1],
                s - lows[2],
            ];
            for (p, x) in power.iter_mut().zip(split) {
                *p += x * x;
            }
        }
    }
    let total = power.iter().sum::<f32>().max(1e-12);
    power.map(|p| p / total)
}

fn write_wav(path: &Path, samples: &[f32], channels: u16) {
    let bytes = (samples.len() * 2) as u32;
    let mut wav = Vec::with_capacity(bytes as usize + 44);
    wav.extend(b"RIFF");
    wav.extend((36 + bytes).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(channels.to_le_bytes());
    wav.extend(RATE.to_le_bytes());
    wav.extend((RATE * 2 * channels as u32).to_le_bytes());
    wav.extend((2 * channels).to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(bytes.to_le_bytes());
    for s in samples {
        let s = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        wav.extend(s.to_le_bytes());
    }
    std::fs::write(path, wav).expect("the sound file");
}

/// Every effect by itself: a file each, and a line of measurements.
/// Gives how loud each is heard at the default settings.
fn effects(dir: &Path, set: &SoundSettings) -> Vec<(Sound, f32)> {
    println!(
        "{:<14} {:<9} {:>5} {:>6} {:>6} {:>6} {:>7} {:>6} {:>6}  \
         {:>4} {:>4} {:>4} {:>4}",
        "sound",
        "family",
        "sec",
        "peak",
        "rms",
        "loud",
        "dc",
        "jump",
        "heard",
        "<250",
        "<1k",
        "<4k",
        ">4k"
    );
    let mut heard = Vec::new();
    let mut warnings = Vec::new();
    for sound in Sound::ALL {
        let samples = sound.samples();
        write_wav(
            &dir.join(format!("{}.wav", sound.name())),
            &samples,
            1,
        );
        let family = sound.family();
        let gain = family.volume(set) * set.volume;
        let loud = loudest(&samples, 1, 0.125);
        let mean = samples.iter().sum::<f32>() / samples.len() as f32;
        let step = jump(&samples, 1);
        let share = bands(&samples, 1);
        println!(
            "{:<14} {:<9} {:>5.2} {:>6.1} {:>6.1} {:>6.1} {:>7.4} \
             {:>6.1} {:>6.1}  {:>4.0} {:>4.0} {:>4.0} {:>4.0}",
            sound.name(),
            format!("{family:?}").to_lowercase(),
            samples.len() as f32 / RATE as f32,
            db(peak(&samples)),
            db(rms(&samples)),
            db(loud),
            mean,
            db(step),
            db(loud * gain),
            share[0] * 100.0,
            share[1] * 100.0,
            share[2] * 100.0,
            share[3] * 100.0,
        );
        // A click is one step far larger than nearly all the rest.
        // Noise has many large steps, and is not one.
        let mut steps = samples
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .collect::<Vec<_>>();
        steps.sort_by(f32::total_cmp);
        let usual = steps[steps.len() * 995 / 1000];
        if step > 4.0 * usual {
            warnings.push(format!(
                "{}: a click, {:.1} times the usual step",
                sound.name(),
                step / usual
            ));
        }
        let (first, last) = (samples[0], samples[samples.len() - 1]);
        if !sound.is_ambient() && first.abs().max(last.abs()) > 0.01 {
            warnings.push(format!("{}: a hard edge", sound.name()));
        }
        if mean.abs() > 1e-3 {
            warnings.push(format!("{}: an offset", sound.name()));
        }
        heard.push((sound, loud * gain));
    }

    println!(
        "\nfamilies, as heard (loudest eighth of a second, dB):"
    );
    for family in FAMILIES {
        let mut of = heard
            .iter()
            .filter(|(s, _)| s.family() == family)
            .map(|&(s, level)| (s, db(level)))
            .collect::<Vec<_>>();
        of.sort_by(|a, b| a.1.total_cmp(&b.1));
        let middle = of[of.len() / 2].1;
        let (quiet, loud) = (of[0], of[of.len() - 1]);
        println!(
            "  {:<9} {:>2} sounds  median {:>6.1}  quietest {} \
             {:>6.1}  loudest {} {:>6.1}",
            format!("{family:?}").to_lowercase(),
            of.len(),
            middle,
            quiet.0.name(),
            quiet.1,
            loud.0.name(),
            loud.1
        );
        // The loops of the ambience are mixed by the hour and the
        // camera's height, not against each other.
        for (sound, level) in of {
            if level > middle + 3.0 && !sound.is_ambient() {
                warnings.push(format!(
                    "{}: {:.1} dB over its family",
                    sound.name(),
                    level - middle
                ));
            }
        }
    }
    println!("\nwarnings: {}", warnings.len());
    for warning in warnings {
        println!("  {warning}");
    }
    heard
}

/// Two channels, to lay breaths and effects onto.
struct Reel {
    samples: Vec<f32>,
    /// The samples each breath begins and ends at.
    edges: Vec<usize>,
}

impl Reel {
    fn new(seconds: f32) -> Self {
        Self {
            samples: vec![0.0; (seconds * RATE as f32) as usize * 2],
            edges: Vec::new(),
        }
    }

    /// Lays two channels of `block` in at `at` seconds.
    fn lay(&mut self, at: f32, block: &[f32], gain: f32) {
        let start = (at * RATE as f32) as usize * 2;
        self.edges.push(start);
        self.edges.push(start + block.len());
        for (i, s) in block.iter().enumerate() {
            if let Some(out) = self.samples.get_mut(start + i) {
                *out += s * gain;
            }
        }
    }

    /// Lays one channel of `clip` in the middle at `at` seconds.
    fn place(&mut self, at: f32, clip: &[f32], gain: f32) {
        let start = (at * RATE as f32) as usize * 2;
        for (i, s) in clip.iter().enumerate() {
            for side in 0..2 {
                if let Some(out) =
                    self.samples.get_mut(start + i * 2 + side)
                {
                    *out +=
                        s * gain * std::f32::consts::FRAC_1_SQRT_2;
                }
            }
        }
    }
}

/// The music for `seconds` under one mood, as the app plays it: a
/// breath at a time, each overlapping the last. A sound in
/// `moments` is answered as the app answers it.
fn score(
    seconds: f32,
    seed: u32,
    mood: impl Fn(f32) -> Mood,
    moments: &[(f32, Sound)],
    set: &SoundSettings,
) -> (Reel, usize) {
    let mut reel = Reel::new(seconds);
    let mut composer = Composer::new(seed);
    let mut asked = moments.iter().peekable();
    let mut answered = f32::MIN;
    let (mut at, mut notes) = (0.0f32, 0);
    while at < seconds {
        let breath = composer.breathe(&mood(at));
        notes += breath.notes.len();
        reel.lay(at, &render(&breath), 1.0);
        let next = at + breath.stride;
        while let Some(&&(when, sound)) = asked.peek() {
            if when >= next {
                break;
            }
            asked.next();
            let Some(moment) = Moment::of(sound) else {
                continue;
            };
            if when - answered > set.phrase_gap {
                answered = when;
                let phrase = composer.answer(moment, &mood(when));
                reel.lay(
                    when + set.phrase_after,
                    &render(&phrase),
                    1.0,
                );
            }
        }
        at = next;
    }
    (reel, notes)
}

/// Measures a reel of music: its level, how much of it is near
/// silence, and whether any breath begins or ends with a click.
fn measure(name: &str, reel: &Reel, notes: usize, gain: f32) -> f32 {
    let samples = &reel.samples;
    let seconds = samples.len() as f32 / 2.0 / RATE as f32;
    let level = rms(samples) * gain;
    let share = bands(samples, 2);
    let per = RATE as usize * 2;
    let quiet = samples
        .chunks(per)
        .filter(|second| rms(second) * gain < 0.001)
        .count() as f32
        / (samples.len() / per).max(1) as f32;
    // The largest step within a hundredth of a second of an edge,
    // against the largest anywhere.
    let near = (RATE as usize / 100) * 2;
    let at_edges = reel
        .edges
        .iter()
        .filter(|&&edge| edge < samples.len())
        .map(|&edge| {
            let from = edge.saturating_sub(near);
            let to = (edge + near).min(samples.len());
            jump(&samples[from..to], 2)
        })
        .fold(0.0f32, f32::max);
    let mean = samples.iter().sum::<f32>() / samples.len() as f32;
    println!(
        "{name:<16} {seconds:>5.0} s  peak {:>6.1}  rms {:>6.1}  \
         loud {:>6.1}  dc {:>8.5}  under -60 dB {:>3.0}%  notes/min \
         {:>4.1}",
        db(peak(samples) * gain),
        db(level),
        db(loudest(samples, 2, 1.0) * gain),
        mean * gain,
        quiet * 100.0,
        notes as f32 * 60.0 / seconds,
    );
    println!(
        "{:<16} step: anywhere {:>6.1} dB, at a breath's edge \
         {:>6.1} dB   bands {:.0}/{:.0}/{:.0}/{:.0}",
        "",
        db(jump(samples, 2) * gain),
        db(at_edges * gain),
        share[0] * 100.0,
        share[1] * 100.0,
        share[2] * 100.0,
        share[3] * 100.0,
    );
    level
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().map_or_else(
        || PathBuf::from("target/sound-sheet"),
        PathBuf::from,
    );
    let minutes = args
        .next()
        .and_then(|m| m.parse::<f32>().ok())
        .unwrap_or(3.0);
    std::fs::create_dir_all(&dir).expect("the folder");
    let set = SoundSettings::default();

    let heard = effects(&dir, &set);

    println!(
        "\nmusic, as heard at the default volumes (music {} x \
         master {}):",
        set.music, set.volume
    );
    let gain = set.music * set.volume;
    let steady = |part: DayPart, busy: f32| {
        let rest = set.music_rest;
        move |_: f32| Mood { part, busy, rest }
    };
    let seconds = minutes * 60.0;
    let moods = [
        ("day-busy", DayPart::Day, 1.0, 1.0),
        ("day-some", DayPart::Day, 0.4, 1.0),
        ("day-quiet", DayPart::Day, 0.0, 1.0),
        ("dusk-some", DayPart::Dusk, 0.4, 1.0),
        ("night-busy", DayPart::Night, 1.0, set.music_night),
        ("night-quiet", DayPart::Night, 0.0, set.music_night),
    ];
    let mut levels = Vec::new();
    for (k, (name, part, busy, dim)) in moods.into_iter().enumerate()
    {
        let (mut reel, notes) = score(
            seconds,
            40 + k as u32,
            steady(part, busy),
            &[],
            &set,
        );
        let level = measure(
            &format!("music-{name}"),
            &reel,
            notes,
            gain * dim,
        );
        levels.push(level);
        for s in &mut reel.samples {
            *s *= gain * dim;
        }
        write_wav(
            &dir.join(format!("music-{name}.wav")),
            &reel.samples,
            2,
        );
    }

    // A minute and a half of a day in the hamlet: the hand, the
    // panels, a house rising, a question and its answer, over the
    // music.
    let scene = [
        (4.0, Sound::Tick),
        (4.3, Sound::Tap),
        (4.35, Sound::PopOpen),
        (5.6, Sound::Tap),
        (5.65, Sound::PopClose),
        (8.0, Sound::Select),
        (9.2, Sound::Focus),
        (12.0, Sound::Poke),
        (12.6, Sound::Poke),
        (13.1, Sound::Annoy),
        (16.0, Sound::Pat),
        (19.0, Sound::Send),
        (21.0, Sound::Speak),
        (22.5, Sound::Muse),
        (26.0, Sound::HouseRise),
        (26.0, Sound::Rune),
        (26.6, Sound::HouseSettle),
        (26.85, Sound::Born),
        (31.0, Sound::Door),
        (31.0, Sound::RoadGlow),
        (33.0, Sound::Hammer),
        (33.95, Sound::Hammer),
        (34.9, Sound::Hammer),
        (38.0, Sound::Think),
        (41.0, Sound::Chime),
        (46.0, Sound::Tap),
        (46.02, Sound::Allowed),
        (50.0, Sound::Lift),
        (51.2, Sound::Drop),
        (54.0, Sound::Tab),
        (55.0, Sound::Fold),
        (58.0, Sound::Gift),
        (64.0, Sound::VoteCalled),
        (64.3, Sound::Ballot),
        (65.1, Sound::Ballot),
        (68.0, Sound::MergeAway),
        (69.5, Sound::MergeLand),
        (74.0, Sound::HouseShudder),
        (74.35, Sound::HouseFall),
        (74.45, Sound::Embers),
        (79.0, Sound::Dusk),
        (84.0, Sound::Stopped),
    ];
    let mood = |at: f32| Mood {
        part: if at < 79.0 {
            DayPart::Day
        } else {
            DayPart::Dusk
        },
        busy: 0.5,
        rest: set.music_rest,
    };
    let (mut reel, notes) = score(95.0, 77, mood, &scene, &set);
    measure("scene (music)", &reel, notes, gain);
    for s in &mut reel.samples {
        *s *= gain;
    }
    for (k, &(at, sound)) in scene.iter().enumerate() {
        let level = sound.family().volume(&set) * set.volume;
        let speed = sound.speed(&set, k as u32);
        let clip = sound.samples();
        // Played faster or slower, as the app varies it.
        let count = ((clip.len() - 1) as f32 / speed) as usize;
        let varied = (0..count)
            .map(|i| {
                let x = i as f32 * speed;
                let (a, part) = (x as usize, x.fract());
                clip[a] * (1.0 - part) + clip[a + 1] * part
            })
            .collect::<Vec<_>>();
        reel.place(at, &varied, level);
    }
    println!(
        "{:<16} with its effects: peak {:>6.1} dB",
        "",
        db(peak(&reel.samples))
    );
    write_wav(&dir.join("scene.wav"), &reel.samples, 2);

    // The music against the effects.
    let mut one_shots = heard
        .iter()
        .filter(|(s, _)| !s.is_ambient())
        .map(|&(_, level)| db(level))
        .collect::<Vec<_>>();
    one_shots.sort_by(f32::total_cmp);
    let middle = one_shots[one_shots.len() / 2];
    let quietest = one_shots[0];
    let loudest_music =
        levels.iter().map(|&l| db(l)).fold(f32::MIN, f32::max);
    println!(
        "\nmusic rms at its fullest {loudest_music:.1} dB; effects \
         as heard: median {middle:.1} dB, quietest {quietest:.1} dB"
    );
    println!(
        "the music sits {:.1} dB under the median effect",
        middle - loudest_music
    );
    println!("wrote {}", dir.display());
}
