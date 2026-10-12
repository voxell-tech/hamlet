//! The notes, the character and the skills an agent keeps in its
//! house, read for its pages.

use std::path::PathBuf;

use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, Res, ResMut};
use bevy::ecs::world::World;
use hamlet_core::core::HomeDir;
use hamlet_core::session::{JobFinished, Seat};
use hamlet_home::Home;
use hamlet_rules::hamlet::Hamlet;

use crate::me::Me;

/// The most of the notes or the character the panel shows.
const NOTES_SHOWN: usize = 6000;

/// The house of the selected agent: its root and its workspace.
pub(crate) fn house(world: &World) -> Home {
    let home = &world.resource::<HomeDir>().0;
    let me = world.resource::<Me>();
    house_of(home, world.get::<Seat>(me.session))
}

/// The house of whoever sits at `seat` in `home`.
pub(crate) fn house_of(home: &Home, seat: Option<&Seat>) -> Home {
    let root = seat.and_then(|seat| {
        Hamlet::at(&home.root, &seat.hamlet).house_dir(&seat.name)
    });
    match root {
        Some(root) => Home {
            workspace: root.join("workspace"),
            root,
        },
        // The first mayor's house is the home itself.
        None => Home {
            root: home.root.clone(),
            workspace: home.workspace.clone(),
        },
    }
}

/// The notes, the character and the skills the selected agent keeps
/// in its house, as its pages show them.
#[derive(Resource, Default, PartialEq)]
pub struct Papers {
    pub(super) notes: String,
    pub(super) character: String,
    /// Each skill of its own by its name, with what it is for.
    pub(super) skills: Vec<(String, String)>,
}

fn read_papers(
    home: Option<&HomeDir>,
    me: &Me,
    seats: &Query<&Seat>,
    papers: &mut ResMut<Papers>,
) {
    let Some(home) = home else {
        return;
    };
    let house = house_of(&home.0, seats.get(me.session).ok());
    let shown = |file: PathBuf| {
        std::fs::read_to_string(file)
            .unwrap_or_default()
            .trim()
            .chars()
            .take(NOTES_SHOWN)
            .collect::<String>()
    };
    let now = Papers {
        notes: shown(house.notes()),
        character: shown(house.character()),
        skills: house
            .skill_index()
            .into_iter()
            .map(|skill| (skill.name, skill.description))
            .collect(),
    };
    if **papers != now {
        **papers = now;
    }
}

/// Reads the selected agent's papers again.
pub fn keep_papers(
    home: Option<Res<HomeDir>>,
    me: Res<Me>,
    seats: Query<&Seat>,
    mut papers: ResMut<Papers>,
) {
    read_papers(home.as_deref(), &me, &seats, &mut papers);
}

/// Reads them again when a turn has ended, which is when an agent
/// has written them.
pub(super) fn papers_on_job(
    _: On<JobFinished>,
    home: Option<Res<HomeDir>>,
    me: Res<Me>,
    seats: Query<&Seat>,
    mut papers: ResMut<Papers>,
) {
    read_papers(home.as_deref(), &me, &seats, &mut papers);
}
