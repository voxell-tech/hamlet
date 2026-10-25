//! The Skills stall: what skills.sh lists, searched past its
//! leaderboard, and a skill taken into the library, its files fetched
//! first.

pub(crate) mod skills_sh;

use std::cmp::Ordering;
use std::collections::BTreeMap;

use bevy::app::App;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::Commands;
use bevy::ecs::world::World;
use hamlet_client::messages::Response;
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::Skills;
use hamlet_protocol::client as protocol;

use crate::bazaar::errand::{self, Errand, Errands, Fetched, json};
use crate::bazaar::shelf::{self, Listing, Stock, now};
use crate::bazaar::skills::skills_sh::{Scroll, Unrolled};

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins(shelf::plugin::<Scroll>)
        .init_resource::<Board>()
        .init_resource::<Desk>()
        .add_observer(answered);
}

/// A skill by skills.sh's id of it, `owner/repo/skill`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub(crate) struct SkillId(pub String);

/// The order skills are shown in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum Order {
    #[default]
    Installs,
    Name,
}

impl Listing for Scroll {
    type Id = SkillId;
    type Filter = ();
    type Order = Order;

    const ORDERS: &'static [(Order, &'static str)] =
        &[(Order::Installs, "Most installed"), (Order::Name, "Name")];

    fn id(&self) -> SkillId {
        SkillId(self.path())
    }

    fn told(&self) -> String {
        format!("{} {} {}", self.name, self.source, self.skill)
    }

    fn order(&self, other: &Scroll, order: Order) -> Ordering {
        let by_name = || {
            self.name.to_lowercase().cmp(&other.name.to_lowercase())
        };
        match order {
            Order::Installs => {
                other.installs.cmp(&self.installs).then_with(by_name)
            }
            Order::Name => {
                by_name().then_with(|| self.source.cmp(&other.source))
            }
        }
    }
}

/// The leaderboard come so far, while the pages after are asked for.
#[derive(Resource, Default)]
struct Board(Vec<Scroll>);

/// The progress of a skill being taken into the library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Taking {
    /// Its files are asked of skills.sh.
    Fetching,
    /// Its files, this many, are handed to the library by the
    /// request.
    Giving(usize, protocol::RequestId),
    /// It is in the library, under this name.
    Taken(String),
    Failed(String),
}

/// The stall's work besides its shelf: a search, the files of the
/// skills opened, and the skill taken into the library.
#[derive(Resource, Default)]
pub(crate) struct Desk {
    /// The words last searched for on skills.sh, and how many more
    /// it found or why it could not. `None` while it is asked.
    pub found: Option<(String, Option<Result<usize, String>>)>,
    /// The files of each skill opened. `None` while they are asked.
    pub unrolled: BTreeMap<SkillId, Option<Result<Unrolled, String>>>,
    /// The skill being taken into the library, or last taken.
    pub taking: Option<(SkillId, Taking)>,
}

impl Desk {
    /// True while a skill is on its way to the library.
    pub fn busy(&self) -> bool {
        matches!(
            self.taking,
            Some((_, Taking::Fetching | Taking::Giving(..)))
        )
    }

    fn take(&mut self, id: &SkillId, now: Taking) {
        self.taking = Some((id.clone(), now));
    }
}

/// True when the library has a skill of the name of `scroll`.
pub(crate) fn shelved(world: &World, scroll: &Scroll) -> bool {
    world.resource::<Skills>().0.iter().any(|skill| {
        skill.name.eq_ignore_ascii_case(&scroll.skill)
            || skill.name.eq_ignore_ascii_case(&scroll.name)
    })
}

/// Asks skills.sh for its leaderboard again, from the first page.
pub(crate) fn fetch_board(world: &mut World) {
    if world.resource::<Stock<Scroll>>().asking {
        return;
    }
    world.resource_mut::<Board>().0.clear();
    if errand::send(world, skills_sh::board(0), Errand::Board(0)) {
        world.resource_mut::<Stock<Scroll>>().ask();
    }
}

/// Takes in the page `page` of the leaderboard, and asks for the next
/// while there are more.
pub(crate) fn board(world: &mut World, page: u32, answer: Fetched) {
    let body = match json(answer, "skills.sh") {
        Ok(body) => body,
        Err(why) => {
            return world
                .resource_mut::<Stock<Scroll>>()
                .came(Err(why), now());
        }
    };
    world
        .resource_mut::<Board>()
        .0
        .extend(skills_sh::read(&body, Some(page)));
    let next = page + 1;
    if skills_sh::has_more(&body)
        && next < skills_sh::PAGES
        && errand::send(
            world,
            skills_sh::board(next),
            Errand::Board(next),
        )
    {
        return;
    }
    let board = std::mem::take(&mut world.resource_mut::<Board>().0);
    let stock = world.resource::<Stock<Scroll>>();
    // What a search found stays, past the board.
    let found = stock
        .items
        .iter()
        .filter(|s| s.rank.is_none())
        .filter(|s| !board.iter().any(|b| b.path() == s.path()))
        .cloned()
        .collect::<Vec<_>>();
    let all = board.into_iter().chain(found).collect();
    world.resource_mut::<Stock<Scroll>>().came(Ok(all), now());
}

/// Asks skills.sh for what `words` name, past its leaderboard.
pub(crate) fn seek(world: &mut World, words: &str) {
    let words = words.trim().to_string();
    if words.chars().count() < 2 {
        return;
    }
    if errand::send(
        world,
        skills_sh::search(&words),
        Errand::Search(words.clone()),
    ) {
        world.resource_mut::<Desk>().found = Some((words, None));
    }
}

/// Puts what a search for `words` found on the shelf, past the
/// leaderboard, unless the chief has searched for others since.
pub(crate) fn found(
    world: &mut World,
    words: String,
    answer: Fetched,
) {
    let desk = world.resource::<Desk>();
    if desk.found.as_ref().is_none_or(|(asked, _)| *asked != words) {
        return;
    }
    let found = match json(answer, "skills.sh") {
        Ok(body) => skills_sh::read(&body, None),
        Err(why) => {
            world.resource_mut::<Desk>().found =
                Some((words, Some(Err(why))));
            return;
        }
    };
    let mut stock = world.resource_mut::<Stock<Scroll>>();
    let more = found
        .into_iter()
        .filter(|scroll| stock.get(&scroll.id()).is_none())
        .collect::<Vec<_>>();
    let count = more.len();
    if count > 0 {
        let mut all = std::mem::take(&mut stock.items);
        all.extend(more);
        stock.stock(all);
    }
    world.resource_mut::<Desk>().found =
        Some((words, Some(Ok(count))));
}

/// Asks skills.sh for the files of the skill `id`, unless they are
/// known or asked for already.
pub(crate) fn unroll(world: &mut World, id: &SkillId) {
    if matches!(
        world.resource::<Desk>().unrolled.get(id),
        Some(Some(Ok(_)) | None)
    ) {
        return;
    }
    let Some(scroll) =
        world.resource::<Stock<Scroll>>().get(id).cloned()
    else {
        return;
    };
    if !skills_sh::askable(&scroll) {
        let why =
            format!("`{}` is no skill of a GitHub repository", id.0);
        world
            .resource_mut::<Desk>()
            .unrolled
            .insert(id.clone(), Some(Err(why)));
        return;
    }
    let errand = Errand::Files(id.clone());
    if world.resource::<Errands>().waiting(&errand) {
        return;
    }
    if errand::send(
        world,
        skills_sh::download(&scroll.path()),
        errand,
    ) {
        world
            .resource_mut::<Desk>()
            .unrolled
            .insert(id.clone(), None);
    }
}

/// Takes in the files of the skill `id`, and hands them to the
/// library when it is being taken.
pub(crate) fn unrolled(
    world: &mut World,
    id: SkillId,
    answer: Fetched,
) {
    let files = json(answer, "skills.sh")
        .and_then(|body| skills_sh::read_files(&body));
    let mut desk = world.resource_mut::<Desk>();
    let taking = desk.taking == Some((id.clone(), Taking::Fetching));
    desk.unrolled.insert(id.clone(), Some(files.clone()));
    match (taking, files) {
        (false, _) => {}
        (true, Ok(files)) => give(world, &id, &files),
        (true, Err(why)) => {
            world
                .resource_mut::<Desk>()
                .take(&id, Taking::Failed(why));
        }
    }
}

/// Takes the skill `id` from skills.sh into the library: its files
/// fetched where they are not known, then handed over.
pub(crate) fn take(world: &mut World, id: &SkillId) {
    if world.resource::<Desk>().busy() {
        return;
    }
    let failed = |world: &mut World, why: String| {
        world.resource_mut::<Desk>().take(id, Taking::Failed(why));
    };
    let Some(scroll) =
        world.resource::<Stock<Scroll>>().get(id).cloned()
    else {
        return failed(world, "skills.sh no longer lists it".into());
    };
    if shelved(world, &scroll) {
        let had = format!(
            "the library already has a skill named `{}`",
            scroll.skill
        );
        return failed(world, had);
    }
    let known = match world.resource::<Desk>().unrolled.get(id) {
        Some(Some(Ok(files))) => Some(files.clone()),
        _ => None,
    };
    if let Some(files) = known {
        return give(world, id, &files);
    }
    world.resource_mut::<Desk>().unrolled.remove(id);
    unroll(world, id);
    match world.resource::<Desk>().unrolled.get(id) {
        Some(None) => {
            world.resource_mut::<Desk>().take(id, Taking::Fetching);
        }
        _ => failed(world, "skills.sh cannot be asked now".into()),
    }
}

/// Hands the skill `id`, made of `files`, to the library.
fn give(world: &mut World, id: &SkillId, files: &Unrolled) {
    let Some(scroll) =
        world.resource::<Stock<Scroll>>().get(id).cloned()
    else {
        return;
    };
    let request = world.resource_mut::<Outbox>().send(|request| {
        protocol::ClientMessage::TakeSkill(protocol::TakeSkill {
            files: files
                .files
                .iter()
                .map(|(path, text)| protocol::SkillText {
                    path: path.clone(),
                    text: text.clone(),
                })
                .collect(),
            from: format!("skills.sh/{}", scroll.path()),
            id: request,
            name: scroll.skill.clone(),
        })
    });
    world
        .resource_mut::<Desk>()
        .take(id, Taking::Giving(files.files.len(), request));
}

/// Takes in the server's answer to a skill handed to the library.
fn answered(response: On<Response>, mut commands: Commands) {
    let response = response.event().clone();
    commands.queue(move |world: &mut World| {
        let Some((id, Taking::Giving(_, request))) =
            world.resource::<Desk>().taking.clone()
        else {
            return;
        };
        if request != response.to {
            return;
        }
        let now = match &response.message {
            protocol::ServerMessage::Fault(fault) => {
                Taking::Failed(fault.text.clone())
            }
            protocol::ServerMessage::Reply(reply)
                if reply.kind == protocol::ReplyKind::Error =>
            {
                Taking::Failed(reply.text.clone())
            }
            _ => {
                crate::bazaar::traded(world);
                let name =
                    id.0.rsplit('/').next().unwrap_or_default();
                Taking::Taken(name.to_string())
            }
        };
        world.resource_mut::<Desk>().take(&id, now);
    });
}
