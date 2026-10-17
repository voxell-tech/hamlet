//! The skills skills.sh offers as a resource: read from their file,
//! asked for again once they are a day old, and taken into the
//! library in the background.

use std::collections::BTreeMap;

use bevy_app::{App, Startup, Update};
use bevy_ecs::prelude::*;
use hamlet_model::log;
use hamlet_model::skills_sh::{self, Scroll, Scrolls, Unrolled};
use hamlet_rules::library::Library;

use crate::core::{
    HomeDir, Model, RefreshLists, Rounds, Tokio, answered,
};

/// What skills.sh lists, or the reason it could not be asked.
type Fetched = Result<Vec<Scroll>, String>;

/// The words searched for, and what skills.sh found.
type Found = (String, Result<Vec<Scroll>, String>);

/// A skill's id and its files.
type Read = (String, Result<Unrolled, String>);

/// A skill's id and how its taking goes.
type Going = (String, Taking);

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<SkillMarket>()
        .init_resource::<Rounds<Fetched>>()
        .init_resource::<Rounds<Found>>()
        .init_resource::<Rounds<Read>>()
        .init_resource::<Rounds<Going>>()
        .add_observer(fetch_scrolls)
        .add_observer(seek)
        .add_observer(unroll)
        .add_observer(take)
        .add_systems(Startup, open)
        .add_systems(
            Update,
            (
                stock.run_if(answered::<Fetched>),
                found.run_if(answered::<Found>),
                read.run_if(answered::<Read>),
                went.run_if(answered::<Going>),
            ),
        );
}

/// The skills skills.sh lists, and what is being asked of it.
#[derive(Resource, Default)]
pub struct SkillMarket {
    pub scrolls: Scrolls,
    /// skills.sh is being asked for its leaderboard.
    pub fetching: bool,
    /// The reason the leaderboard could not be asked for or kept,
    /// last time.
    pub problem: Option<String>,
    /// The words last searched for on skills.sh, and what it found.
    /// `None` while it is asked.
    pub found: Option<(String, Option<Result<Vec<Scroll>, String>>)>,
    /// The files of each skill opened, by its id. `None` while they
    /// are asked for.
    pub unrolled: BTreeMap<String, Option<Result<Unrolled, String>>>,
    /// The skill being taken to the library, or last taken, by its
    /// id, and how that goes.
    pub taking: Option<(String, Taking)>,
}

impl SkillMarket {
    /// A market that holds `scrolls`, as if their file had been read.
    pub fn of(scrolls: Scrolls) -> SkillMarket {
        SkillMarket {
            scrolls,
            ..SkillMarket::default()
        }
    }

    /// The skill of the id `id`, on the leaderboard or found by the
    /// last search.
    pub fn scroll(&self, id: &str) -> Option<&Scroll> {
        let found = match &self.found {
            Some((_, Some(Ok(found)))) => found.as_slice(),
            _ => &[],
        };
        self.scrolls
            .get(id)
            .or_else(|| found.iter().find(|scroll| scroll.id() == id))
    }

    /// True while a skill is being taken to the library.
    pub fn busy(&self) -> bool {
        matches!(
            self.taking,
            Some((_, Taking::Fetching | Taking::Shelving(_)))
        )
    }
}

/// How the taking of a skill to the library goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Taking {
    /// Its files are asked of skills.sh.
    Fetching,
    /// Its files, this many, are written on to the shelves.
    Shelving(usize),
    /// It is on the shelves, in the folder of this name.
    Shelved(String),
    Failed(String),
}

/// Asks skills.sh for its leaderboard in the background.
#[derive(Event)]
pub struct FetchScrolls;

/// Asks skills.sh for the skills these words name or describe.
#[derive(Event)]
pub struct SeekScrolls(pub String);

/// Asks skills.sh for the files of the skill of this id, unless they
/// are known already.
#[derive(Event)]
pub struct Unroll(pub String);

/// Takes the skill of the id `id` from skills.sh into the library, in
/// the background, for the chief `by`.
#[derive(Event)]
pub struct TakeSkill {
    pub id: String,
    pub by: String,
}

/// Reads the listing, and asks for it again when it is stale.
fn open(world: &mut World) {
    let home = world.resource::<HomeDir>().0.root.clone();
    let scrolls = Scrolls::load(&home);
    let stale = scrolls.stale(log::now_millis());
    world.resource_mut::<SkillMarket>().scrolls = scrolls;
    // Answers come from a recording, and nothing is asked of a
    // network.
    if stale && !world.resource::<Model>().replay {
        world.trigger(FetchScrolls);
    }
}

fn fetch_scrolls(
    _: On<FetchScrolls>,
    tokio: Res<Tokio>,
    mut rounds: ResMut<Rounds<Fetched>>,
    mut market: ResMut<SkillMarket>,
) {
    market.fetching = true;
    let tell = rounds.begin();
    tokio.0.spawn(async move {
        tell(skills_sh::fetch().await.map_err(|e| format!("{e:#}")));
    });
}

/// Keeps what skills.sh listed.
fn stock(
    home: Res<HomeDir>,
    mut rounds: ResMut<Rounds<Fetched>>,
    mut market: ResMut<SkillMarket>,
) {
    while let Some(fetched) = rounds.next() {
        market.fetching = false;
        market.problem = match fetched {
            Ok(scrolls) => {
                market.scrolls = Scrolls {
                    fetched: log::now_millis(),
                    scrolls,
                };
                market
                    .scrolls
                    .save(&home.0.root)
                    .err()
                    .map(|e| format!("{e:#}"))
            }
            Err(e) => Some(e),
        };
    }
}

fn seek(
    asked: On<SeekScrolls>,
    tokio: Res<Tokio>,
    mut rounds: ResMut<Rounds<Found>>,
    mut market: ResMut<SkillMarket>,
) {
    let words = asked.event().0.trim().to_string();
    market.found = Some((words.clone(), None));
    let tell = rounds.begin();
    tokio.0.spawn(async move {
        let found = skills_sh::search(&words).await;
        tell((words, found.map_err(|e| format!("{e:#}"))));
    });
}

fn found(
    mut rounds: ResMut<Rounds<Found>>,
    mut market: ResMut<SkillMarket>,
) {
    while let Some((words, found)) = rounds.next() {
        market.found = Some((words, Some(found)));
    }
}

fn unroll(
    asked: On<Unroll>,
    tokio: Res<Tokio>,
    mut rounds: ResMut<Rounds<Read>>,
    mut market: ResMut<SkillMarket>,
) {
    let id = asked.event().0.clone();
    if matches!(market.unrolled.get(&id), Some(Some(Ok(_)))) {
        return;
    }
    let Some(scroll) = market.scroll(&id).cloned() else {
        return;
    };
    // Only the last skill asked for is heard: one still asked would
    // wait forever.
    market.unrolled.retain(|_, files| files.is_some());
    market.unrolled.insert(id.clone(), None);
    let tell = rounds.begin();
    tokio.0.spawn(async move {
        let files = skills_sh::unroll(&scroll).await;
        tell((id, files.map_err(|e| format!("{e:#}"))));
    });
}

fn read(
    mut rounds: ResMut<Rounds<Read>>,
    mut market: ResMut<SkillMarket>,
) {
    while let Some((id, files)) = rounds.next() {
        market.unrolled.insert(id, Some(files));
    }
}

fn take(
    asked: On<TakeSkill>,
    tokio: Res<Tokio>,
    home: Res<HomeDir>,
    mut rounds: ResMut<Rounds<Going>>,
    mut market: ResMut<SkillMarket>,
) {
    let TakeSkill { id, by } = asked.event();
    if market.busy() {
        return;
    }
    let Some(scroll) = market.scroll(id).cloned() else {
        let gone = "skills.sh no longer lists it.".to_string();
        market.taking = Some((id.clone(), Taking::Failed(gone)));
        return;
    };
    let library = Library::of(&home.0.root);
    if library.root().join(&scroll.skill).exists() {
        let had = format!(
            "The library already has a skill named `{}`.",
            scroll.skill
        );
        market.taking = Some((id.clone(), Taking::Failed(had)));
        return;
    }
    let known = match market.unrolled.get(id) {
        Some(Some(Ok(files))) => Some(files.clone()),
        _ => None,
    };
    market.taking = Some((id.clone(), Taking::Fetching));
    let (id, by, tell) = (id.clone(), by.clone(), rounds.begin());
    tokio.0.spawn(async move {
        let files = match known {
            Some(files) => Ok(files),
            None => skills_sh::unroll(&scroll).await,
        };
        let files = match files {
            Ok(files) => files,
            Err(e) => {
                tell((id, Taking::Failed(format!("{e:#}"))));
                return;
            }
        };
        tell((id.clone(), Taking::Shelving(files.files.len())));
        let from = format!("skills.sh: {}", scroll.id());
        let went = match library.give(
            &scroll.skill,
            &files.files,
            &by,
            &from,
        ) {
            Ok(()) => Taking::Shelved(scroll.skill),
            Err(e) => Taking::Failed(format!("{e:#}")),
        };
        tell((id, went));
    });
}

/// Follows a skill on its way to the library, and reads the shelves
/// again once it is there.
fn went(
    mut rounds: ResMut<Rounds<Going>>,
    mut market: ResMut<SkillMarket>,
    mut commands: Commands,
) {
    while let Some((id, going)) = rounds.next() {
        if matches!(going, Taking::Shelved(_)) {
            commands.trigger(RefreshLists);
        }
        market.taking = Some((id, going));
    }
}

#[cfg(test)]
mod tests {
    use hamlet_sandbox::Unsandboxed;
    use tokio::runtime::Runtime;

    use super::*;
    use crate::core::Skills;
    use crate::testing::{self, place, until};

    #[test]
    fn a_skill_the_chief_takes_is_on_the_shelves_and_in_the_list() {
        let rt = Runtime::new().unwrap();
        let home = place("skill-market-take");
        let mut app = testing::app(&home, &rt, &Unsandboxed, vec![]);
        app.update();
        let scroll = Scroll {
            source: "mattpocock/skills".into(),
            skill: "tdd".into(),
            name: "tdd".into(),
            installs: 1_094_811,
            rank: Some(6),
            official: false,
        };
        let id = scroll.id();
        // Opened already, so nothing is asked of skills.sh.
        let files = Unrolled {
            files: vec![(
                "SKILL.md".into(),
                "---\nname: tdd\ndescription: Test first.\n---\n"
                    .into(),
            )],
            left: vec![],
        };
        let mut market =
            app.world_mut().resource_mut::<SkillMarket>();
        market.scrolls.scrolls.push(scroll);
        market.unrolled.insert(id.clone(), Some(Ok(files)));

        let take = |app: &mut App| {
            app.world_mut().trigger(TakeSkill {
                id: id.clone(),
                by: "chief:alice".into(),
            });
        };
        take(&mut app);
        until(&mut app, "the skill to be shelved", |app| {
            app.world().resource::<SkillMarket>().taking
                == Some((id.clone(), Taking::Shelved("tdd".into())))
        });
        app.update();
        let skills = &app.world().resource::<Skills>().0;
        assert!(skills.iter().any(|skill| skill.name == "tdd"));

        // Taken twice, it is refused and the first stays.
        take(&mut app);
        let market = app.world().resource::<SkillMarket>();
        assert!(matches!(
            &market.taking,
            Some((_, Taking::Failed(why))) if why.contains("already")
        ));
    }
}
