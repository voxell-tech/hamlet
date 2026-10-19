//! The limits of a hamlet where its chief changes them: the budget,
//! the models and the mayor's leave. Each control writes one value of
//! a `limits.toml`.

use std::path::{Path, PathBuf};

use bevy::app::{App, Update};
use bevy::ecs::change_detection::{DetectChanges, Ref};
use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, number_field, row,
    segmented,
};
use bevy_fynix::{ScopedExt, ViewExt, keyed, resource};
use hamlet_agents_api::agents::{Charter, Settlement};
use hamlet_base::HomeDir;
use hamlet_home::limits::{self, Limits, Period};
use hamlet_sdk::cards::heading;

use crate::icons::Glyph;
use crate::looks::{
    Fills, action, bare, doors, list_door, note, page_head, tick,
    well,
};
use crate::{AnyView, View, with_cx};

/// The width of the room of a control, so that the controls stand in
/// a line at the panel's right.
const CONTROLS: f32 = 150.0;

pub fn plugin(app: &mut App) {
    app.init_resource::<Book>()
        .init_resource::<Part>()
        .add_systems(Update, follow);
}

/// The chief's limits as the window shows them.
#[derive(Resource, Default)]
pub struct Book {
    /// The limits of the hamlet shown, with the realm's own rules
    /// as the home has them.
    limits: Limits,
    /// The folder of the hamlet shown. The home's until a hamlet's
    /// bylaws are opened.
    root: Option<PathBuf>,
    problem: Option<String>,
    /// The limits shown are those of `root`.
    read: bool,
}

/// A value of the limits that is the realm's, read from the home's
/// file for every hamlet.
fn of_the_realm(section: &str, key: &str) -> bool {
    matches!(section, "votes" | "hamlets")
        || (section, key) == ("road", "grows_over_days")
}

/// A hamlet's `own` limits with the rules of the `realm`.
fn with_realm(mut own: Limits, realm: Limits) -> Limits {
    own.votes = realm.votes;
    own.hamlets = realm.hamlets;
    own.road.grows_over_days = realm.road.grows_over_days;
    own
}

/// The limits that hold for the hamlet at `root` in the home at
/// `home`: its own, with the realm's rules from the home's.
fn limits_of(home: &Path, root: &Path) -> Result<Limits, String> {
    let read =
        |at: &Path| Limits::load(at).map_err(|e| format!("{e:#}"));
    let limits = read(root)?;
    if root == home {
        return Ok(limits);
    }
    Ok(with_realm(limits, read(home)?))
}

/// Shows the limits as the core last read them, when another
/// hamlet's are opened and when the hamlet's file or the home's was
/// read again.
pub fn follow(
    home: Res<HomeDir>,
    hamlets: Query<(&Settlement, Ref<Charter>)>,
    mut book: ResMut<Book>,
) {
    let turned = !book.read;
    let any = hamlets.iter().any(|(_, limits)| limits.is_changed());
    if !turned && !any {
        return;
    }
    let home = home.0.root.as_path();
    let root =
        book.root.clone().unwrap_or_else(|| home.to_path_buf());
    let of = |at: &Path| {
        hamlets
            .iter()
            .find(|(hamlet, _)| hamlet.record().root() == at)
            .map(|(_, limits)| limits)
    };
    let (own, realm) = (of(&root), of(home));
    let moved = [&own, &realm]
        .into_iter()
        .flatten()
        .any(|limits| limits.is_changed());
    if !turned && !moved {
        return;
    }
    let held = |limits: &Option<Ref<Charter>>| {
        limits.as_ref().and_then(|limits| limits.0.clone())
    };
    let limits = match (held(&own), held(&realm)) {
        (Some(own), Some(_)) if root == home => Ok(own),
        (Some(own), Some(realm)) => Ok(with_realm(own, realm)),
        // The file says why it cannot be read.
        _ => limits_of(home, &root),
    };
    match limits {
        Ok(limits) => {
            if book.limits != limits {
                book.limits = limits;
            }
            if book.problem.is_some() {
                book.problem = None;
            }
        }
        Err(e) => book.problem = Some(e),
    }
    if turned {
        book.read = true;
    }
}

/// Sets one value of the limits, or takes it out when `value` is
/// none, and shows the file as it then stands.
fn put(
    world: &mut World,
    section: &str,
    key: &str,
    value: Option<String>,
) {
    let home = world.resource::<HomeDir>().0.root.clone();
    let root = world
        .resource::<Book>()
        .root
        .clone()
        .unwrap_or_else(|| home.clone());
    // A rule of the realm is the home's, whichever hamlet is shown.
    let file = if of_the_realm(section, key) {
        &home
    } else {
        &root
    };
    let done = limits::set(file, section, key, value.as_deref());
    let mut book = world.resource_mut::<Book>();
    match done {
        Ok(()) => {
            if let Ok(limits) = limits_of(&home, &root) {
                book.limits = limits;
            }
            book.problem = None;
        }
        Err(e) => book.problem = Some(format!("{e:#}")),
    }
}

/// A row of the panel: what a control sets, and the control.
fn line(what: &'static str, control: AnyView) -> impl View {
    row((
        // The name gives way in a narrow panel and goes on to a
        // second line. The control keeps its width.
        row((note(what),)).fills_width(),
        row((control,)).width(px(CONTROLS)).shrink(0.0),
    ))
    .gap(10.0)
    .align(AlignItems::Center)
}

fn fine(text: &'static str) -> impl View {
    label(text).size(11.0).tone(Tone::Dim)
}

/// A whole number of the limits, at `key` of `section`.
fn whole(
    section: &'static str,
    key: &'static str,
    get: fn(&Limits) -> u64,
    most: u64,
) -> impl View {
    number_field(
        resource::<Book, _>(move |book| get(&book.limits)),
        move |world, value: u64| {
            put(world, section, key, Some(value.to_string()));
        },
    )
    .range(0u64, most)
    .width(px(110.0))
    .rules(well)
}

/// A cap on cost, in the provider's currency. 0 is no cap.
fn cost(
    section: &'static str,
    get: fn(&Limits) -> Option<f64>,
) -> impl View {
    number_field(
        resource::<Book, _>(move |book| {
            get(&book.limits).unwrap_or(0.0)
        }),
        move |world, value: f64| {
            let value = (value > 0.0).then(|| format!("{value:.2}"));
            put(world, section, "cost", value);
        },
    )
    .range(0.0, 1_000_000.0)
    .step(0.05)
    .precision(2)
    .width(px(110.0))
    .rules(well)
}

/// A yes or a no of the limits.
fn switch(
    section: &'static str,
    key: &'static str,
    get: fn(&Limits) -> bool,
) -> impl View {
    tick(resource::<Book, _>(move |book| get(&book.limits)))
        .on_change(move |world, on| {
            put(world, section, key, Some(on.to_string()));
        })
}

/// The budget of `hamlet` and the caps on one task.
fn budget(hamlet: Entity) -> impl View {
    let period = segmented(
        ["A DAY", "A MONTH"],
        resource::<Book, _>(|book| match book.limits.budget.period {
            Period::Day => 0,
            Period::Month => 1,
        }),
        |world, chosen| {
            let period = if chosen == 0 { "day" } else { "month" };
            put(
                world,
                "budget",
                "period",
                Some(format!("\"{period}\"")),
            );
        },
    )
    .boxed();
    column((
        heading(Glyph::Meter, "THE BUDGET"),
        fine(
            "The most model calls may cost in a period. 0 means no \
             limit.",
        ),
        line("It starts again every", period),
        line(
            "May spend, in dollars",
            cost("budget", |l| l.budget.cost).boxed(),
        ),
        row((
            action(
                Glyph::Restore,
                "START IT AGAIN NOW",
                move |world| {
                    crate::chat::decree_for(
                        world,
                        hamlet,
                        "/reset-budget".into(),
                    )
                },
            ),
            note("What was spent stays in the record."),
        ))
        .gap(10.0)
        .align(AlignItems::Center)
        .tagged(crate::platform::Wraps)
        .boxed(),
        heading(Glyph::Coins, "ONE TASK"),
        fine("Counted across every agent the task touches."),
        line(
            "May cost, in dollars",
            cost("task", |l| l.task.cost).boxed(),
        ),
        line(
            "Messages between agents",
            whole(
                "task",
                "messages",
                |l| u64::from(l.task.messages),
                10_000,
            )
            .boxed(),
        ),
        line(
            "Price of a road, in dollars",
            cost("road", |l| Some(l.road.cost)).boxed(),
        ),
    ))
    .gap(6.0)
}

/// The rules of votes.
fn votes() -> impl View {
    let pass = row((
        number_field(
            resource::<Book, _>(|book| book.limits.votes.pass.0),
            |world, yes: u32| {
                let of = world.resource::<Book>().limits.votes.pass.1;
                put(
                    world,
                    "votes",
                    "pass",
                    Some(format!("[{yes}, {of}]")),
                );
            },
        )
        .range(1u32, 100u32)
        .width(px(50.0))
        .rules(well),
        note("in"),
        number_field(
            resource::<Book, _>(|book| book.limits.votes.pass.1),
            |world, of: u32| {
                let yes =
                    world.resource::<Book>().limits.votes.pass.0;
                put(
                    world,
                    "votes",
                    "pass",
                    Some(format!("[{yes}, {of}]")),
                );
            },
        )
        .range(1u32, 100u32)
        .width(px(50.0))
        .rules(well),
    ))
    .gap(8.0)
    .align(AlignItems::Center)
    .boxed();
    column((
        heading(Glyph::Ballot, "VOTES"),
        fine(
            "Agents may vote to replace their mayor or found a \
             hamlet. You can overturn any vote.",
        ),
        line("It passes with yes from", pass),
        line(
            "Fewest agents to hold one",
            whole(
                "votes",
                "least_voters",
                |l| l.votes.least_voters as u64,
                100,
            )
            .boxed(),
        ),
        line(
            "Ballots hidden until it closes",
            switch("votes", "sealed", |l| l.votes.sealed).boxed(),
        ),
        line(
            "A yes needs grounds in its house",
            switch("votes", "grounded_only", |l| {
                l.votes.grounded_only
            })
            .boxed(),
        ),
        line(
            "Minutes to answer a ballot",
            whole(
                "votes",
                "ballot_minutes",
                |l| l.votes.ballot_minutes,
                600,
            )
            .boxed(),
        ),
        line(
            "Minutes a vote stays open",
            whole(
                "votes",
                "open_minutes",
                |l| l.votes.open_minutes,
                100_000,
            )
            .boxed(),
        ),
        line(
            "Minutes before calling again",
            whole(
                "votes",
                "retry_minutes",
                |l| l.votes.retry_minutes,
                100_000,
            )
            .boxed(),
        ),
    ))
    .gap(6.0)
}

/// The deeds a hamlet's mayor may do alone, and the most agents and
/// roads the hamlet may have.
fn mayor() -> impl View {
    column((
        heading(Glyph::Mayor, "THE MAYOR MAY, WITHOUT ASKING"),
        line(
            "Create agents",
            switch("mayor", "create_agents", |l| {
                l.mayor.create_agents
            })
            .boxed(),
        ),
        line(
            "Remove agents",
            switch("mayor", "remove_agents", |l| {
                l.mayor.remove_agents
            })
            .boxed(),
        ),
        line(
            "Merge agents",
            switch("mayor", "merge_agents", |l| l.mayor.merge_agents)
                .boxed(),
        ),
        line(
            "Build roads",
            switch("mayor", "build_roads", |l| l.mayor.build_roads)
                .boxed(),
        ),
        line(
            "Days of its decisions agents are told",
            whole("mayor", "told_days", |l| l.mayor.told_days, 3650)
                .boxed(),
        ),
        heading(Glyph::Signpost, "AGENTS AND ROADS"),
        line(
            "Most agents in this hamlet",
            whole("", "most_agents", |l| l.most_agents as u64, 1000)
                .boxed(),
        ),
        line(
            "Most roads the mayor builds",
            whole("", "most_roads", |l| l.most_roads as u64, 10_000)
                .boxed(),
        ),
    ))
    .gap(6.0)
}

/// The rules that hold for every hamlet of the realm, as a page of
/// the settings.
pub fn realm() -> impl View {
    let hamlets = column((
        heading(Glyph::Signpost, "HAMLETS AND THE ROADS BETWEEN"),
        line(
            "Most hamlets",
            whole("hamlets", "most", |l| l.hamlets.most as u64, 1000)
                .boxed(),
        ),
        line(
            "Agents of different hamlets may have roads",
            switch("hamlets", "roads_between", |l| {
                l.hamlets.roads_between
            })
            .boxed(),
        ),
        line(
            "Mayors it takes to ask for a hamlet's removal",
            whole(
                "hamlets",
                "removal_mayors",
                |l| l.hamlets.removal_mayors as u64,
                100,
            )
            .boxed(),
        ),
        line(
            "Days until an unused road grows over",
            whole(
                "road",
                "grows_over_days",
                |l| l.road.grows_over_days,
                3650,
            )
            .boxed(),
        ),
    ))
    .gap(6.0);
    column((fine("These apply to every hamlet."), hamlets, votes()))
        .gap(14.0)
}

/// The bylaws of `hamlet`, for its chief to change.
pub fn panel(hamlet: Entity) -> AnyView {
    let problem = keyed(
        resource::<Book, _>(|book| book.problem.clone()),
        |problem| match problem {
            Some(why) => label(why.clone())
                .size(12.0)
                .tone(Tone::Critical)
                .boxed(),
            None => column(()).boxed(),
        },
    )
    .within(bare());
    // The parts as rows of one list, each on a page of its own.
    let page =
        keyed(resource::<Part, _>(|part| *part), move |part| {
            if *part == Part::Front {
                let parts = Part::PAGES.map(|part| {
                    list_door(
                        part.glyph(),
                        part.title(),
                        part.holds(),
                        move |world| {
                            *world.resource_mut::<Part>() = part
                        },
                    )
                });
                let foot = fine(
                    "These apply to this hamlet only. Rules for \
                     every hamlet are under Realm in Settings.",
                )
                .boxed();
                let parts = parts.into();
                return with_cx(move |cx| doors(cx, parts, foot));
            }
            let body = match part {
                Part::Budget => budget(hamlet).boxed(),
                Part::Models => crate::models::of_hamlet(hamlet),
                Part::Front | Part::Mayor => mayor().boxed(),
            };
            column((
                page_head(
                    "BYLAWS",
                    |world| {
                        *world.resource_mut::<Part>() = Part::Front
                    },
                    part.glyph(),
                    part.title(),
                ),
                body,
            ))
            .gap(14.0)
            .boxed()
        })
        .within(bare());
    AnyView::new(move |cx: &mut crate::Cx| {
        // The bylaws shown are this hamlet's, read from its own
        // folder.
        let root =
            cx.world.get::<Settlement>(hamlet).map(|settlement| {
                settlement.record().root().to_path_buf()
            });
        let mut book = cx.world.resource_mut::<Book>();
        if book.root != root {
            book.root = root;
            book.read = false;
        }
        cx.build(column((problem, page)).gap(8.0))
    })
}

/// A part of a hamlet's bylaws, each on a page of its own.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Part {
    /// The list of the others.
    #[default]
    Front,
    Budget,
    Models,
    Mayor,
}

impl Part {
    const PAGES: [Part; 3] =
        [Part::Budget, Part::Models, Part::Mayor];

    fn title(self) -> &'static str {
        match self {
            Part::Front => "BYLAWS",
            Part::Budget => "BUDGET",
            Part::Models => "MODELS",
            Part::Mayor => "THE MAYOR AND LIMITS",
        }
    }

    fn glyph(self) -> Glyph {
        match self {
            Part::Front => Glyph::Book,
            Part::Budget => Glyph::Meter,
            Part::Models => Glyph::Chip,
            Part::Mayor => Glyph::Mayor,
        }
    }

    fn holds(self) -> &'static str {
        match self {
            Part::Front => "",
            Part::Budget => "The budget, and the caps on one task.",
            Part::Models => "Which models this hamlet may run.",
            Part::Mayor => {
                "What the mayor may do without asking, and the caps \
                 on agents and roads."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;

    #[test]
    fn a_control_writes_one_value_and_says_when_it_cannot() {
        let home = Scratch::new("ui-rules");
        let mut world = World::new();
        world.insert_resource(HomeDir(hamlet_home::Home {
            root: home.to_path_buf(),
            workspace: home.join("workspace"),
        }));
        world.init_resource::<Book>();
        put(&mut world, "votes", "sealed", Some("false".into()));
        put(&mut world, "budget", "tokens", None);
        let kept = Limits::load(&home).unwrap();
        assert!(!kept.votes.sealed);
        assert_eq!(kept.budget.tokens, None);
        // The window shows the file as it stands.
        let book = world.resource::<Book>();
        assert_eq!(book.limits, kept);
        assert_eq!(book.problem, None);

        // A value the file cannot hold is said, and nothing changes.
        put(&mut world, "votes", "pass", Some("[5, 3]".into()));
        assert!(world.resource::<Book>().problem.is_some());
        assert_eq!(Limits::load(&home).unwrap(), kept);
    }

    #[test]
    fn a_hamlet_s_bylaws_are_its_own_and_the_realm_s_rules_the_home_s()
     {
        let home = Scratch::new("ui-rules-hamlet");
        let other = home.join("hamlets").join("mill");
        std::fs::create_dir_all(&other).unwrap();
        let mut world = World::new();
        world.insert_resource(HomeDir(hamlet_home::Home {
            root: home.to_path_buf(),
            workspace: home.join("workspace"),
        }));
        world.insert_resource(Book {
            root: Some(other.clone()),
            ..Book::default()
        });
        // Its budget is written to its own file and the home's is
        // left as it was.
        put(&mut world, "budget", "cost", Some("9.00".into()));
        assert_eq!(
            Limits::load(&other).unwrap().budget.cost,
            Some(9.0)
        );
        assert_eq!(
            Limits::load(&home).unwrap().budget.cost,
            Some(5.0)
        );
        // A rule of the realm goes to the home's, and is shown with
        // the hamlet's own.
        put(&mut world, "hamlets", "most", Some("3".into()));
        assert_eq!(Limits::load(&home).unwrap().hamlets.most, 3);
        assert_eq!(Limits::load(&other).unwrap().hamlets.most, 6);
        let shown = &world.resource::<Book>().limits;
        assert_eq!(
            (shown.budget.cost, shown.hamlets.most),
            (Some(9.0), 3)
        );
    }
}
