//! Stills of a chief's place, its mascot and a Wickstep, drawn
//! without a window (docs/chief.md).
//!
//! Every still: `cargo run --example chief_sheet`
//! Into a folder: `cargo run --example chief_sheet -- --out <dir>`

// Bevy systems take many parameters and long query types by nature.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

use std::path::PathBuf;
use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use hamlet_core::agents::{
    Citizen, Lived, LivesIn, Mayor, Settlement, Treasury,
};
use hamlet_home::limits::Caps;
use hamlet_offscreen::{self as offscreen, Target};
use hamlet_realm::chief::mascot::Doing;
use hamlet_realm::chief::{
    self, Beside, ChiefSettings, Cloak, Hat, Holds, Light, Look,
    Mascot, Phase, Place, Settled,
};
use hamlet_realm::generator::{Loading, Robot};
use hamlet_realm::hamlet::Housed;
use hamlet_realm::library::{Librarian, Library, Stood};
use hamlet_realm::pick::{Centre, Glide, Orbit};
use hamlet_realm::sky::{CameraSettings, SkySettings};
use hamlet_realm::{
    Clock, Hour, RealmPlugin, Stage, WorldCamera, WorldSystems,
};
use hamlet_rules::budget::{Amount, Report, Standing};
use hamlet_rules::hamlet::HOME;

const SIZE: UVec2 = UVec2::new(1280, 800);

/// The time a frame takes.
const FRAME: Duration = Duration::from_nanos(16_666_667);

/// The camera's subject.
#[derive(Clone, Copy)]
enum On {
    Hearth,
    Mascot,
    /// The agent the chief talks to.
    Agent,
    Library,
    Librarian,
}

/// One thing the sheet does.
#[derive(Clone, Copy)]
enum Do {
    /// Lets this many frames go by.
    Wait(u32),
    Night(bool),
    Wear(Look),
    /// Talks to the first agent, or lets it go.
    Talk(bool),
    /// Waits until the mascot is this far into coming in, or into
    /// going out.
    In(f32),
    Out(f32),
    /// Waits until the mascot does this at home, hurrying it from
    /// spot to spot meanwhile.
    Until(Doing),
    /// Looks at a thing from this far, turned this much from the
    /// way its door faces, this many radians under the usual tilt.
    See(On, f32, f32, f32),
    /// Sets the library down a little way off.
    MoveLibrary,
    /// Has an agent call on the librarian.
    Consult,
    Shot(&'static str),
}

fn look(cloak: Cloak, light: Light, hat: Hat, holds: Holds) -> Look {
    Look {
        cloak,
        light,
        hat,
        holds,
    }
}

fn script() -> Vec<Do> {
    use Do::*;
    let mut steps = vec![
        Night(false),
        See(On::Hearth, 44.0, 0.45, -0.12),
        Wait(40),
        Shot("01-hearth-day"),
        See(On::Hearth, 24.0, -0.3, -0.3),
        Wait(20),
        Shot("02-hearth-near"),
        See(On::Mascot, 6.5, 0.6, -0.4),
        Wait(20),
        Shot("03-mascot-at-home"),
    ];
    let looks = [
        look(Cloak::Moss, Light::Leaf, Hat::Hood, Holds::Book),
        look(Cloak::River, Light::Moon, Hat::Crown, Holds::Teacup),
        look(Cloak::Plum, Light::Rosy, Hat::Wreath, Holds::Nothing),
        look(Cloak::Mustard, Light::Rune, Hat::Bare, Holds::Quill),
        look(
            Cloak::Cream,
            Light::Candle,
            Hat::Pointed,
            Holds::Teacup,
        ),
    ];
    let names = [
        "04-look-moss-hood",
        "05-look-river-crown",
        "06-look-plum-wreath",
        "07-look-mustard-bare",
        "08-look-cream-pointed",
    ];
    for (worn, name) in looks.into_iter().zip(names) {
        steps.extend([Wear(worn), Wait(12), Shot(name)]);
    }
    steps.push(Wear(Look::default()));
    for (doing, name) in [
        (Doing::Read, "09-reads-a-letter"),
        (Doing::Tend, "10-tends-the-bed"),
        (Doing::Gaze, "11-looks-at-the-sky"),
    ] {
        steps.extend([Until(doing), Wait(55), Shot(name)]);
    }
    steps.push(Until(Doing::Idle));
    steps.extend([
        Wear(looks[1]),
        See(On::Hearth, 44.0, 0.45, -0.12),
        Wait(12),
        Shot("12-hearth-crown-and-moon"),
        Wear(Look::default()),
        Night(true),
        Wait(40),
        Shot("13-hearth-night"),
        // A Wickstep to an agent, seen from close by.
        Night(false),
        See(On::Agent, 11.0, 0.3, -0.3),
        Wait(30),
        Shot("14-before"),
        Talk(true),
        // It goes out at home, out of sight, and comes in here.
        In(0.06),
        Shot("15-in-a-twentieth"),
        In(0.22),
        Shot("16-in-a-fifth"),
        In(0.42),
        Shot("17-in-two-fifths"),
        In(0.62),
        Shot("18-in-three-fifths"),
        In(0.9),
        Shot("19-in-nearly-there"),
        Wait(40),
        Shot("20-the-agent-waves"),
        Wait(120),
        Shot("21-beside-the-agent"),
        Talk(false),
        Out(0.15),
        Shot("22-out-a-sixth"),
        Out(0.5),
        Shot("23-out-half"),
        Out(0.8),
        Shot("24-out-four-fifths"),
        Wait(90),
        // The same from where the camera starts.
        See(On::Agent, 72.0, 0.0, 0.0),
        Wait(20),
        Talk(true),
        In(0.35),
        Shot("25-in-from-afar"),
        Wait(70),
        Shot("26-beside-from-afar"),
        Talk(false),
        Out(0.5),
        Shot("27-out-from-afar"),
        Wait(90),
        Night(true),
        Wear(look(
            Cloak::River,
            Light::Rune,
            Hat::Pointed,
            Holds::Quill,
        )),
        See(On::Agent, 16.0, 0.3, -0.3),
        Wait(40),
        Talk(true),
        In(0.35),
        Shot("28-in-by-night"),
        Wait(70),
        Shot("29-beside-by-night"),
        Talk(false),
        Wait(90),
        See(On::Hearth, 30.0, 0.45, -0.2),
        Wait(40),
        Shot("30-hearth-by-night-rune"),
        // The library, by night and by day.
        See(On::Library, 44.0, 0.55, -0.1),
        Wait(60),
        Shot("31-the-library-by-night"),
        Night(false),
        Wait(60),
        Shot("32-the-library"),
        See(On::Librarian, 9.5, 0.3, -0.5),
        Wait(60),
        Shot("33-the-librarian"),
        See(On::Library, 44.0, 0.55, -0.1),
        Night(true),
        Wait(60),
        Consult,
        Wait(40),
        Shot("33-called-on-by-night"),
        Night(false),
        MoveLibrary,
        See(On::Library, 44.0, 0.55, -0.1),
        Wait(90),
        Shot("34-the-library-set-down-elsewhere"),
        // The last file is given time to be written.
        Wait(40),
    ]);
    steps
}

#[derive(Resource)]
struct Sheet {
    out: PathBuf,
    steps: Vec<Do>,
    at: usize,
    /// Frames left of a wait.
    wait: u32,
    /// Frames the world has stood finished before the first step.
    settled: u32,
    /// The camera's subject and its distance, turn and tilt.
    seen: Option<(On, f32, f32, f32)>,
}

/// The agent the sheet's chief talks to.
#[derive(Resource)]
struct Friend(Entity);

fn main() {
    let out = offscreen::word("--out").map_or_else(
        || {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("examples/chief_sheet/out")
        },
        PathBuf::from,
    );
    std::fs::create_dir_all(&out).expect("output folder");
    let mut app = App::new();
    app.add_plugins((offscreen::plugins(SIZE), RealmPlugin));
    Target::insert(&mut app, SIZE, 1.0);
    app.insert_resource(Sheet {
        out,
        steps: script(),
        at: 0,
        wait: 0,
        settled: 0,
        seen: None,
    })
    .insert_resource(TimeUpdateStrategy::ManualDuration(
        Duration::from_secs_f64(1.0 / 60.0),
    ))
    .add_systems(Startup, people)
    .add_systems(PostStartup, offscreen)
    .add_systems(Update, (direct, aim).chain().before(WorldSystems));
    app.run();
}

/// `spent` of a budget of a thousand.
fn standing(spent: u64) -> Standing {
    Standing {
        spent: Amount {
            tokens: spent,
            ..Amount::default()
        },
        caps: Caps {
            tokens: Some(1000),
            ..Caps::default()
        },
    }
}

/// A hamlet of a mayor and four agents, and a chief. A third of the
/// day has gone and over half the budget, so its meter shows it
/// spending fast.
fn people(world: &mut World) {
    let hamlet = world
        .spawn((
            Name::new("Pear Tree Hollow"),
            Settlement {
                home: PathBuf::new(),
                name: HOME.to_string(),
            },
            Treasury(Report {
                period: "today".into(),
                gone: 0.33,
                hamlet: standing(560),
                ..Report::default()
            }),
        ))
        .id();
    world.spawn((
        Name::new("founder"),
        Citizen(0),
        Mayor,
        LivesIn(hamlet),
        Lived::default(),
    ));
    let mut friend = None;
    for (number, name) in
        [(1, "Juniper"), (2, "Sorrel"), (3, "Bramble"), (4, "Tansy")]
    {
        let agent = world
            .spawn((
                Name::new(name),
                Citizen(number),
                LivesIn(hamlet),
                Lived {
                    since: number,
                    until: None,
                },
            ))
            .id();
        friend.get_or_insert(agent);
    }
    world.insert_resource(Friend(friend.unwrap()));
    chief::arrive(world, "wren", Look::default());
}

fn offscreen(
    mut commands: Commands,
    target: Res<Target>,
    camera: Single<Entity, With<WorldCamera>>,
) {
    commands
        .entity(*camera)
        .insert((target.drawn(), IsDefaultUiCamera));
}

/// Plays the sheet: one step after another, once the world stands.
fn direct(
    mut commands: Commands,
    mut sheet: ResMut<Sheet>,
    target: Res<Target>,
    friend: Res<Friend>,
    loading: Option<Res<Loading>>,
    sky: Res<SkySettings>,
    clock: Res<Clock>,
    mascots: Query<&Mascot>,
    mut hour: ResMut<Hour>,
    mut stage: ResMut<Stage>,
    mut chiefs: Query<(&mut Look, &mut Beside)>,
    mut set: ResMut<ChiefSettings>,
    mut pace: ResMut<TimeUpdateStrategy>,
    mut exit: MessageWriter<AppExit>,
) {
    // Time stands still while the world is built, so that a still
    // does not depend on how long the building took.
    let built = loading.is_some_and(|loading| loading.done());
    let frame = if built { FRAME } else { Duration::ZERO };
    *pace = TimeUpdateStrategy::ManualDuration(frame);
    if !built {
        return;
    }
    if sheet.settled < 90 {
        sheet.settled += 1;
        return;
    }
    if sheet.wait > 0 {
        sheet.wait -= 1;
        return;
    }
    let Some(step) = sheet.steps.get(sheet.at).copied() else {
        exit.write(AppExit::Success);
        return;
    };
    // The part of its Wickstep the mascot has done, of one.
    let into = |arriving: bool| {
        mascots.iter().find_map(|mascot| match mascot.phase {
            Phase::In { began, long } if arriving => {
                Some((clock.t - began) / long)
            }
            Phase::Out { began, long } if !arriving => {
                Some((clock.t - began) / long)
            }
            _ => None,
        })
    };
    match step {
        Do::In(far) if into(true).is_none_or(|u| u < far) => return,
        Do::Out(far) if into(false).is_none_or(|u| u < far) => return,
        Do::Until(doing)
            if mascots.iter().all(|mascot| mascot.doing != doing) =>
        {
            if set.linger != (1.5, 2.0) {
                set.linger = (1.5, 2.0);
            }
            return;
        }
        Do::Until(_) => *set = ChiefSettings::default(),
        _ => {}
    }
    sheet.at += 1;
    match step {
        Do::Wait(frames) => sheet.wait = frames,
        Do::In(_) | Do::Out(_) | Do::Until(_) => {}
        Do::Night(night) => {
            hour.0 = Some(sky.held(night));
            stage.night = night;
        }
        Do::Wear(worn) => {
            for (mut look, _) in &mut chiefs {
                *look = worn;
            }
        }
        Do::Talk(on) => {
            for (_, mut beside) in &mut chiefs {
                beside.0 = on.then_some(friend.0);
            }
        }
        Do::Consult => {
            commands.trigger(hamlet_core::agents::Consulted {
                agent: None,
            });
        }
        Do::MoveLibrary => {
            commands.queue(|world: &mut World| {
                let at = world
                    .query::<&Library>()
                    .iter(world)
                    .next()
                    .map(|library| library.held);
                if let Some(at) = at {
                    world.resource_mut::<Stood>().0 =
                        Some(at + Vec2::new(24.0, 6.0));
                }
            });
        }
        Do::See(on, far, turn, tilt) => {
            sheet.seen = Some((on, far, turn, tilt));
        }
        Do::Shot(name) => {
            let path = sheet.out.join(format!("{name}.png"));
            target.shoot(&mut commands, path);
            // The next still is not asked for in the same frame.
            sheet.wait = 1;
        }
    }
}

/// Keeps the camera on what the sheet looks at.
fn aim(
    sheet: Res<Sheet>,
    friend: Res<Friend>,
    centre: Res<Centre>,
    camera: Res<CameraSettings>,
    chiefs: Query<&Settled>,
    places: Query<&Transform, With<Place>>,
    mascots: Query<&Transform, With<Mascot>>,
    libraries: Query<&Transform, With<Library>>,
    librarians: Query<&GlobalTransform, With<Librarian>>,
    agents: Query<&Housed>,
    robots: Query<(&Robot, &Transform)>,
    mut orbit: ResMut<Orbit>,
    mut glide: ResMut<Glide>,
) {
    let Some((on, far, turn, tilt)) = sheet.seen else {
        return;
    };
    let Some(settled) = chiefs.iter().next() else {
        return;
    };
    let Ok(place) = places.get(settled.place) else {
        return;
    };
    let library = libraries.iter().next();
    let front = match (on, library) {
        (On::Library | On::Librarian, Some(library)) => {
            library.rotation * Vec3::Z
        }
        _ => place.rotation * Vec3::Z,
    };
    let at = match on {
        On::Library => library.map(|at| at.translation),
        On::Librarian => {
            librarians.iter().next().map(|at| at.translation())
        }
        On::Hearth => Some(place.translation),
        On::Mascot => {
            mascots.get(settled.mascot).ok().map(|at| at.translation)
        }
        On::Agent => agents.get(friend.0).ok().and_then(|housed| {
            robots
                .iter()
                .find(|(robot, _)| robot.agent == housed.0)
                .map(|(_, at)| at.translation)
        }),
    };
    let Some(at) = at else {
        return;
    };
    let next = Orbit {
        pan: at.xz() - centre.0,
        zoom: far / camera.distance,
        yaw: front.x.atan2(front.z) + turn,
        pitch: tilt,
    };
    if *orbit != next {
        *orbit = next;
    }
    if glide.0 != next {
        glide.0 = next;
    }
}
