//! The links in the chat (docs/chat.md): the place each leads, the
//! folders a path is looked for in, and the click that follows one.

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
use hamlet_client::messages::JobFinished;
use hamlet_client::realm::{LivesIn, Works};
use hamlet_desktop::open;
use hamlet_world::sound::{Cue, Sound};

use crate::me::Me;
use crate::roll::Roll;
use crate::sounds;

/// The most paths whose files are remembered.
const REMEMBERED: usize = 4096;

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
            turn.run_if(
                resource_changed::<Me>.or_eager(hamlets_turned),
            ),
        );
}

/// The place a span of text leads when it is clicked.
#[derive(Component, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum Anchor {
    Web(String),
    /// A file or a folder, with every link on the way to it followed.
    File(PathBuf),
}

/// The local folders used to resolve relative paths in the chat.
#[derive(Resource, Default, Clone, PartialEq, Eq, Debug)]
pub struct Roots {
    folders: Vec<PathBuf>,
    /// The session whose agent they are the folders of.
    of: Option<Entity>,
    folder: Option<String>,
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
}

/// The file each path written in the chat names under [`Roots`], as
/// it was last looked for.
#[derive(Resource, Default)]
struct Found(HashMap<String, Option<PathBuf>>);

/// The local work folder of the selected agent's hamlet.
fn turn(world: &mut World) {
    let Some(me) = world.get_resource::<Me>() else {
        return;
    };
    let session = me.session;
    let folder = world
        .get::<LivesIn>(me.agent)
        .and_then(|lives| world.get::<Works>(lives.0))
        .and_then(|works| works.folder.clone());
    if world.get_resource::<Roots>().is_some_and(|roots| {
        roots.of == Some(session) && roots.folder == folder
    }) {
        return;
    }
    let roots = Roots {
        of: Some(session),
        folder: folder.clone(),
        ..Roots::new(folder.into_iter().map(PathBuf::from))
    };
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
    let path = Path::new(written);
    let file = if path.is_absolute() {
        path.canonicalize().ok()
    } else {
        world
            .get_resource::<Roots>()
            .and_then(|roots| open::locate(written, &roots.folders))
    };
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
        Anchor::File(path) => open_file(path),
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

fn hamlets_turned(
    roll: Option<Res<Roll>>,
    mut seen: Local<Option<u64>>,
) -> bool {
    let now = roll.map(|roll| roll.hamlets());
    std::mem::replace(&mut *seen, now) != now
}

/// Forgets the files found when a turn has ended, which is when an
/// agent has written some.
fn forget(_: On<JobFinished>, mut found: ResMut<Found>) {
    if !found.0.is_empty() {
        found.0.clear();
    }
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
            chat: Entity::PLACEHOLDER,
        });
        assert_eq!(seek(&mut world, "plan.md"), Some(file));
    }
}
