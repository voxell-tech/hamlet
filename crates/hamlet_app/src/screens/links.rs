//! The links in the chat (docs/chat.md).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::app::{App, PreUpdate};
use bevy::ecs::entity::Entity;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Changed, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemCondition};
use bevy::ecs::system::{Local, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::picking::events::{PointerClick, PointerPress};
use bevy::picking::pointer::PointerButton;
use hamlet_client::chats::TalksTo;
use hamlet_client::messages::JobFinished;
use hamlet_client::realm::{LivesIn, RealmFolder, Works};
use hamlet_core::panels::PanelSystems;
use hamlet_core::roll::{self, Roll};
use hamlet_core::sound::{Cue, Sound};
use hamlet_desktop::open;
use hamlet_ui::markdown::{Anchor, PathLinks};

use crate::convo::Convo;
use crate::sounds;

/// The most paths whose files are remembered.
const REMEMBERED: usize = 4096;

/// Opens the link of the chat that is clicked.
pub fn plugin(app: &mut App) {
    app.init_resource::<Roots>()
        .init_resource::<Found>()
        .insert_resource(PathLinks {
            resolve: seek,
            generation: 0,
        })
        .add_observer(tap)
        .add_observer(follow)
        .add_observer(forget)
        .add_systems(
            PreUpdate,
            (
                turn.run_if(chats_turned.or_eager(hamlets_turned)),
                roots_moved.run_if(resource_changed::<Roots>),
            )
                .chain()
                .after(roll::turn_roll)
                .in_set(PanelSystems::Bind),
        );
}

/// Has the documents find their links again under the new folders.
fn roots_moved(mut links: ResMut<PathLinks>) {
    links.generation = links.generation.wrapping_add(1);
}

/// The local folders used to resolve relative paths in the Hubs'
/// chats.
#[derive(Resource, Default, Clone, PartialEq, Eq, Debug)]
pub struct Roots {
    folders: Vec<PathBuf>,
    /// The work folders of the hamlets of the Hubs' chat agents, as
    /// the realm names them.
    named: Vec<String>,
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

/// The file each path in a chat names, by the work folder it was
/// looked for under, else [`Roots`], once looked for; and the realm's
/// folder, whose files are never linked.
#[derive(Resource, Default)]
struct Found(
    HashMap<(Option<String>, String), Option<PathBuf>>,
    Option<PathBuf>,
);

/// The work folder of the hamlet of the agent `hub`'s chat is with,
/// as the realm names it.
fn work_folder(world: &World, hub: Entity) -> Option<String> {
    folder_of(world, crate::convo::agent_of(world, hub))
}

/// The local work folder of `agent`'s hamlet.
fn folder_of(world: &World, agent: Entity) -> Option<String> {
    let hamlet = world.get::<LivesIn>(agent)?.0;
    world.get::<Works>(hamlet)?.folder().cloned()
}

/// The work folder a path drawn `under` is of: a chat's own, or that
/// of the chat in the Hub holding the node. `None` outside both.
fn base(world: &World, under: Entity) -> Option<Option<String>> {
    if let Some(to) = world.get::<TalksTo>(under) {
        return Some(folder_of(world, to.0));
    }
    let hub = crate::hub::holding(world, under)?;
    Some(work_folder(world, hub))
}

/// The local work folders of the hamlets of the agents the Hubs'
/// chats are with, changed only when those folders do, so a chat
/// that moves to another agent of the same hamlets draws no link
/// again.
fn turn(world: &mut World) {
    let mut named = world
        .try_query::<&Convo>()
        .map(|mut convos| {
            convos
                .iter(world)
                .filter_map(|convo| {
                    let hamlet = world.get::<LivesIn>(convo.agent)?.0;
                    world.get::<Works>(hamlet)?.folder().cloned()
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    named.sort();
    named.dedup();
    if world
        .get_resource::<Roots>()
        .is_some_and(|roots| roots.named == named)
    {
        return;
    }
    let roots = Roots {
        named: named.clone(),
        ..Roots::new(named.into_iter().map(PathBuf::from))
    };
    world.insert_resource(roots);
    world.get_resource_or_init::<Found>().0.clear();
}

/// The file `written` names, drawn `under` a node or for a chat,
/// looked for once and remembered. For a chat, or in a Hub, a
/// relative path is of that chat's work folder; elsewhere, of any
/// Hub's.
pub(crate) fn seek(
    world: &mut World,
    under: Option<Entity>,
    written: &str,
) -> Option<PathBuf> {
    // The chat may be drawn for another agent before `turn` runs.
    turn(world);
    let base = under.and_then(|node| base(world, node));
    // A chat with no work folder links no relative path.
    if base.as_ref().is_some_and(Option::is_none)
        && !Path::new(written).is_absolute()
    {
        return None;
    }
    let own = base.flatten();
    let key = (own.clone(), written.to_string());
    let kept = world
        .get_resource::<RealmFolder>()
        .map(|folder| PathBuf::from(&folder.0))
        .map(|path| path.canonicalize().unwrap_or(path));
    let mut found = world.get_resource_or_init::<Found>();
    if found.1 != kept {
        found.0.clear();
        found.1 = kept.clone();
    }
    let known = world
        .get_resource::<Found>()
        .and_then(|found| found.0.get(&key));
    if let Some(known) = known {
        return known.clone();
    }
    let path = Path::new(written);
    let file = if path.is_absolute() {
        path.canonicalize().ok()
    } else if let Some(own) = &own {
        let folders = Roots::new([PathBuf::from(own)]).folders;
        open::locate(written, &folders)
    } else {
        world
            .get_resource::<Roots>()
            .and_then(|roots| open::locate(written, &roots.folders))
    };
    let file = file.filter(|file| {
        !kept.as_ref().is_some_and(|kept| file.starts_with(kept))
    });
    let mut found = world.get_resource_or_init::<Found>();
    if found.0.len() >= REMEMBERED {
        found.0.clear();
    }
    found.0.insert(key, file.clone());
    file
}

fn tap(
    press: On<PointerPress>,
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

/// Opens the link that was clicked.
fn follow(
    click: On<PointerClick>,
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
    let launch = match anchor {
        Anchor::Web(address) => open::browse(address),
        Anchor::File(path) => open::show_file(path),
    };
    if launch == open::Launch::Failed {
        sounds::write(&mut cues, &mut turn, Sound::Deny);
    }
}

/// Whether a Hub's chat changed.
fn chats_turned(convos: Query<(), Changed<Convo>>) -> bool {
    !convos.is_empty()
}

fn hamlets_turned(
    roll: Option<Res<Roll>>,
    mut seen: Local<Option<u64>>,
) -> bool {
    let now = roll.map(|roll| roll.hamlets());
    std::mem::replace(&mut *seen, now) != now
}

/// Forgets the files found when a turn has ended.
fn forget(_: On<JobFinished>, mut found: ResMut<Found>) {
    if !found.0.is_empty() {
        found.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_received_realm_folder_stays_plain_even_through_work_folder_links()
     {
        let home = hamlet_testing::Scratch::new("house-links");
        let houses = home.join("houses");
        std::fs::create_dir_all(houses.join("home/1")).unwrap();
        let private = houses.join("home/1/plan.md");
        let public = home.join("public.md");
        std::fs::write(&private, "private").unwrap();
        std::fs::write(&public, "public").unwrap();
        let mut app = App::new();
        app.add_plugins(hamlet_client::ClientPlugin { server: None });
        app.world_mut()
            .insert_resource(Roots::new([home.to_path_buf()]));
        assert!(
            seek(app.world_mut(), None, private.to_str().unwrap())
                .is_some()
        );
        let mut state =
            serde_json::to_value(hamlet_testing::realm::state())
                .unwrap();
        state["realm"] = serde_json::json!(houses);
        hamlet_client::receive(
            app.world_mut(),
            hamlet_protocol::client::ServerMessage::State(
                serde_json::from_value(state).unwrap(),
            ),
        );
        assert_eq!(
            seek(app.world_mut(), None, private.to_str().unwrap()),
            None
        );
        assert_eq!(
            seek(app.world_mut(), None, "houses/home/1/plan.md"),
            None
        );
        assert!(seek(app.world_mut(), None, "public.md").is_some());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                &houses,
                home.join("shortcut"),
            )
            .unwrap();
            assert_eq!(
                seek(
                    app.world_mut(),
                    None,
                    "shortcut/home/1/plan.md"
                ),
                None
            );
        }
    }
}
