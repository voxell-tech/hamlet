//! The links in the chat (docs/chat.md): the place each leads, the
//! folders a path is looked for in, and the click that follows one.

mod open;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::Remove;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemCondition};
use bevy::ecs::system::{Local, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::picking::events::{Click, Out, Over, Pointer, Press};
use bevy::picking::pointer::PointerButton;
use hamlet_core::core::HomeDir;
use hamlet_core::session::{JobFinished, Seat};
use hamlet_home::Home;
use hamlet_rules::hamlet::Hamlet;
use hamlet_world::sound::{Cue, Sound};

use crate::inspector::house_of;
use crate::me::Me;
use crate::roll::Roll;
use crate::sounds;

/// The place a span of text leads when it is clicked.
#[derive(Component, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum Anchor {
    Web(String),
    /// A file or a folder, with every link on the way to it followed.
    File(PathBuf),
}

/// The folders a path written in the chat is looked for in, the
/// first looked in first. A file outside all of them is no link.
#[derive(Resource, Default, Clone, PartialEq, Eq, Debug)]
pub struct Roots {
    folders: Vec<PathBuf>,
    /// The session whose agent they are the folders of.
    of: Option<Entity>,
}

impl Roots {
    /// Those of `folders` that are there.
    pub fn new(folders: impl IntoIterator<Item = PathBuf>) -> Self {
        let mut roots = Roots::default();
        for folder in folders {
            if let Ok(folder) = folder.canonicalize()
                && !roots.folders.contains(&folder)
            {
                roots.folders.push(folder);
            }
        }
        roots
    }

    /// The folder the hamlet of whoever sits at `seat` works on, then
    /// that agent's workspace and its house.
    fn of(home: &Home, session: Entity, seat: Option<&Seat>) -> Self {
        let hamlet = match seat {
            Some(seat) => Hamlet::at(&home.root, &seat.hamlet),
            None => Hamlet::new(home.root.clone()),
        };
        let house = house_of(home, seat);
        Roots {
            of: Some(session),
            ..Roots::new(
                hamlet
                    .work()
                    .folder
                    .into_iter()
                    .chain([house.workspace, house.root]),
            )
        }
    }
}

/// The most paths whose files are remembered.
const REMEMBERED: usize = 4096;

/// The file each path written in the chat names under [`Roots`], as
/// it was last looked for.
#[derive(Resource, Default)]
struct Found(HashMap<String, Option<PathBuf>>);

/// Sets [`Roots`] for the session of the chat, if they are those of
/// another.
fn turn(world: &mut World) {
    let (Some(home), Some(me)) =
        (world.get_resource::<HomeDir>(), world.get_resource::<Me>())
    else {
        return;
    };
    let kept = world.get_resource::<Roots>();
    if kept.is_some_and(|roots| roots.of == Some(me.session)) {
        return;
    }
    let seat = world.get::<Seat>(me.session);
    let roots = Roots::of(&home.0, me.session, seat);
    world.insert_resource(roots);
    world.get_resource_or_init::<Found>().0.clear();
}

/// The file `written` names, looked for on the disk once and
/// remembered.
pub(crate) fn seek(
    world: &mut World,
    written: &str,
) -> Option<PathBuf> {
    // The chat is drawn for another agent before the system that
    // keeps the folders has seen it turn.
    turn(world);
    let known = world
        .get_resource::<Found>()
        .and_then(|found| found.0.get(written));
    if let Some(known) = known {
        return known.clone();
    }
    let roots = world.get_resource::<Roots>()?;
    let file = open::locate(written, &roots.folders);
    let mut found = world.get_resource_or_init::<Found>();
    if found.0.len() >= REMEMBERED {
        found.0.clear();
    }
    found.0.insert(written.to_string(), file.clone());
    file
}

/// The link under the pointer.
#[derive(Resource, Default, PartialEq, Eq, Debug)]
pub struct Aimed {
    span: Option<Entity>,
    /// The place it leads, in full. Empty when the pointer is on no
    /// link.
    pub address: String,
}

impl Aimed {
    /// A pointer that rests on a link to `address`.
    pub fn at(address: &str) -> Self {
        Aimed {
            span: None,
            address: address.to_string(),
        }
    }
}

fn aim(
    over: On<Pointer<Over>>,
    anchors: Query<&Anchor>,
    mut aimed: ResMut<Aimed>,
) {
    let Ok(anchor) = anchors.get(over.entity) else {
        return;
    };
    let address = match anchor {
        Anchor::Web(address) => address.clone(),
        Anchor::File(path) => match open::way(path) {
            open::Way::Open => path.display().to_string(),
            open::Way::Reveal => {
                format!("{} (shown in its folder)", path.display())
            }
        },
    };
    *aimed = Aimed {
        span: Some(over.entity),
        address,
    };
}

fn leave(aimed: &mut ResMut<Aimed>, span: Entity) {
    if aimed.span == Some(span) {
        **aimed = Aimed::default();
    }
}

fn aim_off(out: On<Pointer<Out>>, mut aimed: ResMut<Aimed>) {
    leave(&mut aimed, out.entity);
}

/// A link that is drawn again under the pointer goes with no word of
/// the pointer leaving it.
fn aim_gone(gone: On<Remove, Anchor>, mut aimed: ResMut<Aimed>) {
    leave(&mut aimed, gone.entity);
}

fn tap(
    press: On<Pointer<Press>>,
    anchors: Query<(), With<Anchor>>,
    mut cues: MessageWriter<Cue>,
    mut turn: Local<u32>,
) {
    if press.button == PointerButton::Primary
        && anchors.contains(press.entity)
    {
        sounds::write(&mut cues, &mut turn, Sound::Tap);
    }
}

/// Opens the link that was clicked: an address in the browser, a
/// file by the system's program for it.
fn follow(
    click: On<Pointer<Click>>,
    anchors: Query<&Anchor>,
    roots: Res<Roots>,
    mut cues: MessageWriter<Cue>,
    mut turn: Local<u32>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok(anchor) = anchors.get(click.entity) else {
        return;
    };
    let went = match anchor {
        Anchor::Web(address) => open::browse(address),
        Anchor::File(path) => open::show(path, &roots.folders),
    };
    if !went {
        sounds::write(&mut cues, &mut turn, Sound::Deny);
    }
}

/// Opens the file `path` by the system's program for it. False when
/// nothing could.
pub(crate) fn open_file(path: &Path) -> bool {
    let folder =
        path.parent().and_then(|folder| folder.canonicalize().ok());
    folder.is_some_and(|folder| open::show(path, &[folder]))
}

/// True when a hamlet's record has moved since it was last asked.
fn hamlets_turned(
    roll: Res<Roll>,
    mut seen: Local<Option<u64>>,
) -> bool {
    let now = Some(roll.hamlets());
    std::mem::replace(&mut *seen, now) != now
}

/// Keeps [`Roots`] as those of the agent the chat is with.
fn keep_roots(
    home: Option<Res<HomeDir>>,
    me: Res<Me>,
    seats: Query<&Seat>,
    mut roots: ResMut<Roots>,
    mut found: ResMut<Found>,
) {
    let Some(home) = home else {
        return;
    };
    let seat = seats.get(me.session).ok();
    let now = Roots::of(&home.0, me.session, seat);
    if *roots != now {
        *roots = now;
        found.0.clear();
    }
}

/// Forgets the files found when a turn has ended, which is when an
/// agent has written some.
fn forget(_: On<JobFinished>, mut found: ResMut<Found>) {
    if !found.0.is_empty() {
        found.0.clear();
    }
}

/// Follows the pointer onto the links of the chat and opens the one
/// that is clicked.
pub fn plugin(app: &mut App) {
    app.init_resource::<Roots>()
        .init_resource::<Found>()
        .init_resource::<Aimed>()
        .add_observer(aim)
        .add_observer(aim_off)
        .add_observer(aim_gone)
        .add_observer(tap)
        .add_observer(follow)
        .add_observer(forget)
        .add_systems(
            Update,
            keep_roots.run_if(
                resource_changed::<Me>.or_eager(hamlets_turned),
            ),
        );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_looked_for_once_until_a_turn_ends() {
        let home = hamlet_testing::Scratch::new("links-seek");
        let file = home.join("plan.md");
        let mut world = World::new();
        world.insert_resource(Roots::new([home.to_path_buf()]));
        world.init_resource::<Found>();
        world.add_observer(forget);

        assert_eq!(seek(&mut world, "plan.md"), None);
        std::fs::write(&file, "").unwrap();
        assert_eq!(seek(&mut world, "plan.md"), None);
        world.trigger(JobFinished {
            session: Entity::PLACEHOLDER,
        });
        assert_eq!(seek(&mut world, "plan.md"), Some(file));
    }
}
