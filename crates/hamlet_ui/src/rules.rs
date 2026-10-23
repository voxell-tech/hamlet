//! A hamlet's received limits and the chief's edits through the
//! server.

use std::collections::BTreeMap;

use bevy::app::{App, Update};
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, number_field, row,
    segmented,
};
use bevy_fynix::{ScopedExt, ViewExt, keyed, resource};
use hamlet_client::messages::Response;
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::{Bylaws, HOME, Settlement};
use hamlet_protocol::client::{self as protocol, Limits, Period};
use serde_json::Value;

use crate::icons::Glyph;
use crate::looks::{
    Fills, action, bare, doors, list_door, note, page_head, tick,
    well,
};
use crate::overview::heading;
use crate::{AnyView, View, with_cx};

const CONTROLS: f32 = 150.0;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Book>()
        .init_resource::<Part>()
        .add_systems(Update, follow)
        .add_observer(answered);
}

/// The received limits of the hamlet shown.
#[derive(Resource, Default)]
pub struct Book {
    hamlet: Option<Entity>,
    limits: Option<Limits>,
    fields: BTreeMap<(&'static str, &'static str), Field>,
}

#[derive(Default)]
struct Field {
    pending: Option<protocol::RequestId>,
    problem: Option<String>,
}

/// The displayed limits kept in step with the received bylaws.
pub fn follow(hamlets: Query<&Bylaws>, mut book: ResMut<Book>) {
    let limits = book
        .hamlet
        .and_then(|hamlet| hamlets.get(hamlet).ok())
        .map(|limits| &limits.0);
    if book.limits.as_ref() != limits {
        book.limits = limits.cloned();
    }
}

fn put(
    world: &mut World,
    section: &'static str,
    key: &'static str,
    value: Value,
) {
    let Some(hamlet) = world.resource::<Book>().hamlet else {
        return;
    };
    let Some(name) = world
        .get::<Settlement>(hamlet)
        .map(|hamlet| hamlet.name.clone())
    else {
        return;
    };
    let Some(limits) = world.get::<Bylaws>(hamlet) else {
        return;
    };
    let whole = if section.is_empty() {
        let mut whole =
            serde_json::to_value(&limits.0).expect("limits JSON");
        whole[key] = value.clone();
        Some(serde_json::from_value(whole).expect("limits document"))
    } else {
        None
    };
    let id = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Edit(protocol::Edit::Limits(
            match whole {
                Some(whole) => protocol::EditLimits::Whole {
                    id,
                    hamlet: name,
                    reason: None,
                    whole,
                },
                None => protocol::EditLimits::Set {
                    id,
                    hamlet: name,
                    reason: None,
                    set: protocol::EditLimitsSetSet {
                        section: section.into(),
                        key: key.into(),
                        value,
                    },
                },
            },
        ))
    });
    let mut book = world.resource_mut::<Book>();
    let field = book.fields.entry((section, key)).or_default();
    field.pending = Some(id);
    field.problem = None;
}

fn answered(response: On<Response>, mut book: ResMut<Book>) {
    let Some(field) = book
        .fields
        .values_mut()
        .find(|field| field.pending.as_ref() == Some(&response.to))
    else {
        return;
    };
    let problem = match &response.message {
        protocol::ServerMessage::Reply(reply) => (reply.kind
            == protocol::ReplyKind::Error)
            .then(|| reply.text.clone()),
        protocol::ServerMessage::Fault(fault) => {
            Some(fault.text.clone())
        }
        _ => return,
    };
    field.pending = None;
    field.problem = problem;
}

fn control(
    section: &'static str,
    key: &'static str,
    make: impl Fn() -> AnyView + Send + Sync + 'static,
) -> AnyView {
    keyed(
        resource::<Book, _>(move |book| {
            book.fields
                .get(&(section, key))
                .and_then(|field| field.problem.clone())
        }),
        move |problem| {
            let mut parts = vec![make()];
            if let Some(problem) = problem {
                parts.push(
                    label(problem.clone())
                        .size(12.0)
                        .tone(Tone::Critical)
                        .boxed(),
                );
            }
            column(parts).gap(4.0).boxed()
        },
    )
    .within(bare())
    .boxed()
}

fn line(what: &'static str, control: AnyView) -> impl View {
    row((
        row((note(what),)).fills_width(),
        row((control,)).width(px(CONTROLS)).shrink(0.0),
    ))
    .gap(10.0)
    .align(AlignItems::Center)
}

fn fine(text: &'static str) -> impl View {
    label(text).size(11.0).tone(Tone::Dim)
}

fn whole(
    section: &'static str,
    key: &'static str,
    get: fn(&Limits) -> i64,
    most: i64,
) -> impl View {
    control(section, key, move || {
        number_field(
            resource::<Book, _>(move |book| {
                book.limits.as_ref().map_or(0, get)
            }),
            move |world, value: i64| {
                put(world, section, key, value.into());
            },
        )
        .range(0, most)
        .width(px(110.0))
        .rules(well)
        .boxed()
    })
}

fn cost(
    section: &'static str,
    get: fn(&Limits) -> Option<f64>,
) -> impl View {
    control(section, "cost", move || {
        number_field(
            resource::<Book, _>(move |book| {
                book.limits.as_ref().and_then(get).unwrap_or(0.0)
            }),
            move |world, value: f64| {
                put(
                    world,
                    section,
                    "cost",
                    serde_json::json!(
                        (value > 0.0 || section == "road")
                            .then_some(value)
                    ),
                );
            },
        )
        .range(0.0, 1_000_000.0)
        .step(0.05)
        .precision(2)
        .width(px(110.0))
        .rules(well)
        .boxed()
    })
}

fn switch(
    section: &'static str,
    key: &'static str,
    get: fn(&Limits) -> bool,
) -> impl View {
    control(section, key, move || {
        tick(resource::<Book, _>(move |book| {
            book.limits.as_ref().is_some_and(get)
        }))
        .on_change(move |world, on| {
            put(world, section, key, on.into());
        })
        .boxed()
    })
}

/// The budget of `hamlet` and the caps on one task.
fn budget(hamlet: Entity) -> impl View {
    let period = control("budget", "period", || {
        segmented(
            ["A DAY", "A MONTH"],
            resource::<Book, _>(|book| {
                match book
                    .limits
                    .as_ref()
                    .map(|limits| &limits.budget.period)
                {
                    Some(Period::Day) | None => 0,
                    Some(Period::Month) => 1,
                }
            }),
            |world, chosen| {
                let period =
                    if chosen == 0 { "day" } else { "month" };
                put(
                    world,
                    "budget",
                    "period",
                    serde_json::json!(period),
                );
            },
        )
        .boxed()
    });
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
                    crate::overview::tell(
                        world,
                        hamlet,
                        "/reset-budget".into(),
                    );
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
            whole("task", "messages", |l| l.task.messages, 10_000)
                .boxed(),
        ),
        line(
            "Price of a road, in dollars",
            cost("road", |l| Some(l.road.cost)).boxed(),
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
            switch("mayor", "createAgents", |l| {
                l.mayor.create_agents
            })
            .boxed(),
        ),
        line(
            "Remove agents",
            switch("mayor", "removeAgents", |l| {
                l.mayor.remove_agents
            })
            .boxed(),
        ),
        line(
            "Merge agents",
            switch("mayor", "mergeAgents", |l| l.mayor.merge_agents)
                .boxed(),
        ),
        line(
            "Build roads",
            switch("mayor", "buildRoads", |l| l.mayor.build_roads)
                .boxed(),
        ),
        line(
            "Days of its decisions agents are told",
            whole("mayor", "toldDays", |l| l.mayor.told_days, 3650)
                .boxed(),
        ),
        heading(Glyph::Signpost, "AGENTS AND ROADS"),
        line(
            "Most agents in this hamlet",
            whole("", "mostAgents", |l| l.most_agents, 1000).boxed(),
        ),
        line(
            "Most roads the mayor builds",
            whole("", "mostRoads", |l| l.most_roads, 10_000).boxed(),
        ),
    ))
    .gap(6.0)
}

fn votes() -> impl View {
    column((
        heading(Glyph::Ballot, "THE VOTES"),
        line(
            "Yes votes needed",
            whole("votes", "passYes", |l| l.votes.pass_yes, 1000)
                .boxed(),
        ),
        line(
            "Of every",
            whole("votes", "passOf", |l| l.votes.pass_of, 1000)
                .boxed(),
        ),
        line(
            "Least voters",
            whole(
                "votes",
                "leastVoters",
                |l| l.votes.least_voters,
                1000,
            )
            .boxed(),
        ),
        line(
            "Hide earlier ballots",
            switch("votes", "sealed", |l| l.votes.sealed).boxed(),
        ),
        line(
            "Count only grounded yes votes",
            switch("votes", "groundedOnly", |l| {
                l.votes.grounded_only
            })
            .boxed(),
        ),
        line(
            "Minutes for a ballot",
            whole(
                "votes",
                "ballotMinutes",
                |l| l.votes.ballot_minutes,
                10000,
            )
            .boxed(),
        ),
        line(
            "Minutes a vote stays open",
            whole(
                "votes",
                "openMinutes",
                |l| l.votes.open_minutes,
                10000,
            )
            .boxed(),
        ),
        line(
            "Minutes before trying again",
            whole(
                "votes",
                "retryMinutes",
                |l| l.votes.retry_minutes,
                10000,
            )
            .boxed(),
        ),
    ))
    .gap(6.0)
}

fn realm_rules() -> impl View {
    column((
        heading(Glyph::Globe, "THE REALM"),
        fine("These apply to every hamlet, from home's bylaws."),
        line(
            "Most hamlets",
            whole("hamlets", "most", |l| l.hamlets.most, 1000)
                .boxed(),
        ),
        line(
            "Roads between hamlets",
            switch("hamlets", "roadsBetween", |l| {
                l.hamlets.roads_between
            })
            .boxed(),
        ),
        line(
            "Mayors asking for removal",
            whole(
                "hamlets",
                "removalMayors",
                |l| l.hamlets.removal_mayors,
                1000,
            )
            .boxed(),
        ),
    ))
    .gap(6.0)
}

/// The way to home's realm bylaws.
pub fn realm() -> AnyView {
    column((
        fine("The realm's rules live in home's bylaws."),
        action(Glyph::Book, "OPEN HOME'S BYLAWS", |world| {
            let home = world
                .query::<(Entity, &Settlement)>()
                .iter(world)
                .find(|(_, h)| h.name == HOME)
                .map(|(e, _)| e);
            let Some(home) = home else { return };
            *world.resource_mut::<Part>() = Part::Realm;
            *world.resource_mut::<crate::overview::Page>() =
                crate::overview::Page::Bylaws;
            hamlet_realm::pick::select(
                world,
                Some(hamlet_realm::pick::Thing::Common(home)),
            );
            crate::dock::show_tab(world, "agent");
        }),
    ))
    .gap(8.0)
    .boxed()
}

/// The bylaws of `hamlet`, for its chief to change.
pub fn panel(hamlet: Entity) -> AnyView {
    let page =
        keyed(resource::<Part, _>(|part| *part), move |part| {
            if *part == Part::Front {
                return AnyView::new(move |cx: &mut crate::Cx| {
                    let home = cx
                        .world
                        .get::<Settlement>(hamlet)
                        .is_some_and(|h| h.name == HOME);
                    let parts = Part::PAGES
                        .into_iter()
                        .filter(|part| *part != Part::Realm || home)
                        .map(|part| {
                            list_door(
                                part.glyph(),
                                part.title(),
                                part.holds(),
                                move |world| {
                                    *world.resource_mut::<Part>() =
                                        part;
                                },
                            )
                        })
                        .collect::<Vec<_>>();
                    let foot = fine(if home {
                        "The realm's rules apply to every hamlet."
                    } else {
                        "These apply to this hamlet only."
                    })
                    .boxed();
                    cx.build(with_cx(move |cx| {
                        doors(cx, parts, foot)
                    }))
                });
            }
            let body = match part {
                Part::Budget => budget(hamlet).boxed(),
                Part::Models => crate::models::of_hamlet(hamlet),
                Part::Votes => votes().boxed(),
                Part::Realm => realm_rules().boxed(),
                Part::Front | Part::Mayor => mayor().boxed(),
            };
            column((
                page_head(
                    "BYLAWS",
                    |world| {
                        *world.resource_mut::<Part>() = Part::Front;
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
        if cx
            .world
            .get::<Settlement>(hamlet)
            .is_some_and(|h| h.name != HOME)
            && *cx.world.resource::<Part>() == Part::Realm
        {
            *cx.world.resource_mut::<Part>() = Part::Front;
        }
        let limits = cx
            .world
            .get::<Bylaws>(hamlet)
            .map(|limits| limits.0.clone());
        let mut book = cx.world.resource_mut::<Book>();
        if book.hamlet != Some(hamlet) {
            book.fields.clear();
            book.hamlet = Some(hamlet);
        }
        book.limits = limits;
        cx.build(page)
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
    Votes,
    Realm,
}

impl Part {
    const PAGES: [Part; 5] = [
        Part::Budget,
        Part::Models,
        Part::Mayor,
        Part::Votes,
        Part::Realm,
    ];

    fn title(self) -> &'static str {
        match self {
            Part::Front => "BYLAWS",
            Part::Budget => "BUDGET",
            Part::Models => "MODELS",
            Part::Mayor => "THE MAYOR AND LIMITS",
            Part::Votes => "THE VOTES",
            Part::Realm => "THE REALM",
        }
    }

    fn glyph(self) -> Glyph {
        match self {
            Part::Front => Glyph::Book,
            Part::Budget => Glyph::Meter,
            Part::Models => Glyph::Chip,
            Part::Mayor => Glyph::Mayor,
            Part::Votes => Glyph::Ballot,
            Part::Realm => Glyph::Globe,
        }
    }

    fn holds(self) -> &'static str {
        match self {
            Part::Front => "",
            Part::Votes => {
                "The ballots, the threshold and the time to answer."
            }
            Part::Realm => "The hamlets and their dealings.",
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
    use bevy::ecs::hierarchy::{ChildOf, Children};
    use bevy::input::InputPlugin;
    use bevy::input_focus::{
        FocusCause, InputDispatchPlugin, InputFocus, InputFocusPlugin,
    };
    use bevy::text::EditableText;
    use bevy::window::{PrimaryWindow, Window};
    use hamlet_client::outbox::Outbox;
    use serde_json::{Value, json};

    use super::*;
    use crate::me::tests::panel_app;

    fn page(part: Part) -> (App, Entity) {
        let mut app = panel_app();
        app.add_plugins((
            plugin,
            InputPlugin,
            InputFocusPlugin,
            InputDispatchPlugin,
        ));
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        let hamlet = app
            .world_mut()
            .query::<(Entity, &Settlement)>()
            .iter(app.world())
            .next()
            .unwrap()
            .0;
        *app.world_mut().resource_mut::<Part>() = part;
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            panel(hamlet),
        );
        app.update();
        (app, hamlet)
    }

    fn input(app: &mut App, label: &str) -> Entity {
        let world = app.world_mut();
        let mut at = world
            .query::<(Entity, &bevy::ui::widget::Text)>()
            .iter(world)
            .find(|(_, text)| text.0 == label)
            .unwrap()
            .0;
        loop {
            let mut descendants = vec![at];
            while let Some(entity) = descendants.pop() {
                if world.get::<EditableText>(entity).is_some() {
                    return entity;
                }
                if let Some(children) = world.get::<Children>(entity)
                {
                    descendants.extend(children.iter().copied());
                }
            }
            at = world.get::<ChildOf>(at).unwrap().parent();
        }
    }

    fn enter(app: &mut App, label: &str, value: &str) {
        let input = input(app, label);
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(input, FocusCause::Pressed);
        app.update();
        app.world_mut()
            .get_mut::<EditableText>(input)
            .unwrap()
            .editor_mut()
            .set_text(value);
        app.world_mut().resource_mut::<InputFocus>().clear();
        for _ in 0..3 {
            app.update();
        }
    }

    fn edits(app: &App) -> Vec<Value> {
        app.world()
            .resource::<Outbox>()
            .iter()
            .map(|message| serde_json::to_value(message).unwrap())
            .filter(|message| message["type"] == "edit")
            .collect()
    }

    #[test]
    fn settings_points_to_home_without_retargeting_another_hamlets_edits()
     {
        use hamlet_realm::pick::{Also, Selected, Thing};

        use crate::me::tests::press;
        let mut app = panel_app();
        app.add_plugins((
            plugin,
            crate::overview::plugin,
            InputPlugin,
            InputFocusPlugin,
            InputDispatchPlugin,
        ))
        .init_resource::<Selected>()
        .init_resource::<Also>();
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        let mut state = hamlet_testing::realm::state();
        state.hamlets.push(hamlet_testing::realm::hamlet("mill"));
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        let places = app
            .world_mut()
            .query::<(Entity, &Settlement)>()
            .iter(app.world())
            .map(|(e, h)| (h.name.clone(), e))
            .collect::<BTreeMap<_, _>>();
        *app.world_mut().resource_mut::<Part>() = Part::Budget;
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            panel(places["mill"]),
        );
        app.update();
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            realm(),
        );
        app.update();
        enter(&mut app, "May spend, in dollars", "2");
        assert_eq!(edits(&app)[0]["hamlet"], "mill");
        press(&mut app, "OPEN HOME'S BYLAWS");
        assert_eq!(
            app.world().resource::<Selected>().0,
            Some(Thing::Common(places["home"]))
        );
        assert_eq!(*app.world().resource::<Part>(), Part::Realm);
        assert!(
            *app.world().resource::<crate::overview::Page>()
                == crate::overview::Page::Bylaws
        );
    }

    #[test]
    fn vote_and_realm_controls_send_the_received_sections_and_keys() {
        use crate::me::tests::press;
        let (mut app, _) = page(Part::Front);
        let hamlet = hamlet_testing::realm::hamlet("home");
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::Change(protocol::Changes {
                epoch: 1,
                revision: 1,
                changes: vec![protocol::Update::Hamlet { hamlet }],
            }),
        );
        for _ in 0..3 {
            app.update();
        }
        press(&mut app, "THE VOTES");
        for _ in 0..3 {
            app.update();
        }
        enter(&mut app, "Yes votes needed", "3");
        enter(&mut app, "Of every", "4");
        enter(&mut app, "Least voters", "5");
        assert_eq!(
            edits(&app)[0]["set"],
            json!({"section":"votes", "key":"passYes", "value":3})
        );
        assert_eq!(
            edits(&app)[1]["set"],
            json!({"section":"votes", "key":"passOf", "value":4})
        );
        assert_eq!(
            edits(&app)[2]["set"],
            json!({"section":"votes", "key":"leastVoters", "value":5})
        );
        *app.world_mut().resource_mut::<Part>() = Part::Front;
        for _ in 0..3 {
            app.update();
        }
        press(&mut app, "THE REALM");
        for _ in 0..3 {
            app.update();
        }
        enter(&mut app, "Most hamlets", "8");
        enter(&mut app, "Mayors asking for removal", "2");
        assert_eq!(
            edits(&app)[3]["set"],
            json!({"section":"hamlets", "key":"most", "value":8})
        );
        assert_eq!(
            edits(&app)[4]["set"],
            json!({"section":"hamlets", "key":"removalMayors", "value":2})
        );
    }

    #[test]
    fn only_home_offers_the_realms_bylaws() {
        use crate::me::tests::words;
        let mut app = panel_app();
        app.add_plugins(plugin);
        let mut state = hamlet_testing::realm::state();
        state.hamlets.push(hamlet_testing::realm::hamlet("mill"));
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        let mill = app
            .world_mut()
            .query::<(Entity, &Settlement)>()
            .iter(app.world())
            .find(|(_, h)| h.name == "mill")
            .unwrap()
            .0;
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            panel(mill),
        );
        app.update();
        let shown = words(&mut app);
        assert!(shown.iter().any(|s| s == "THE VOTES"));
        assert!(!shown.iter().any(|s| s == "THE REALM"));
    }

    #[test]
    fn daily_cost_and_its_clear_are_limits_edits() {
        let (mut app, _) = page(Part::Budget);
        enter(&mut app, "May spend, in dollars", "2");
        let sent = edits(&app);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["doc"], "limits");
        assert_eq!(sent[0]["hamlet"], "home");
        assert_eq!(
            sent[0]["set"],
            json!({"section": "budget", "key": "cost", "value": 2.0})
        );
        enter(&mut app, "May spend, in dollars", "0");
        assert_eq!(
            edits(&app)[1]["set"],
            json!({"section": "budget", "key": "cost", "value": null})
        );
    }
    #[test]
    fn a_refused_edit_restores_the_received_value_beside_its_words() {
        use hamlet_protocol::client as protocol;

        use crate::me::tests::words;
        let (mut app, _) = page(Part::Budget);
        enter(&mut app, "May spend, in dollars", "2");
        let to =
            edits(&app)[0]["id"].as_str().unwrap().parse().unwrap();
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::Reply(protocol::Reply {
                to,
                kind: protocol::ReplyKind::Error,
                text: "The daily cost was refused.".into(),
                upload: None,
            }),
        );
        for _ in 0..3 {
            app.update();
        }
        assert!(
            words(&mut app)
                .iter()
                .any(|word| word == "The daily cost was refused.")
        );
        let field = input(&mut app, "May spend, in dollars");
        assert_eq!(
            app.world().get::<EditableText>(field).unwrap().value(),
            "5.00"
        );
        let mut hamlet = hamlet_testing::realm::hamlet("home");
        hamlet.limits.budget.cost = Some(3.5);
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::Change(protocol::Changes {
                epoch: 1,
                revision: 1,
                changes: vec![protocol::Update::Hamlet { hamlet }],
            }),
        );
        for _ in 0..3 {
            app.update();
        }
        let field = input(&mut app, "May spend, in dollars");
        assert_eq!(
            app.world().get::<EditableText>(field).unwrap().value(),
            "3.50"
        );
    }

    #[test]
    fn a_top_level_edit_keeps_the_other_received_limits_and_section_keys_are_camel_case()
     {
        let (mut app, _) = page(Part::Mayor);
        enter(&mut app, "Most agents in this hamlet", "8");
        let mut expected = serde_json::to_value(
            hamlet_testing::realm::hamlet("home").limits,
        )
        .unwrap();
        expected["mostAgents"] = json!(8);
        let sent = edits(&app);
        assert_eq!(sent[0]["whole"], expected);
        assert!(sent[0].get("set").is_none());
        enter(&mut app, "Days of its decisions agents are told", "4");
        assert_eq!(
            edits(&app)[1]["set"],
            json!({"section": "mayor", "key": "toldDays", "value": 4})
        );
    }

    #[test]
    fn period_is_text_and_a_free_road_is_not_an_uncapped_meter() {
        use crate::me::tests::press;
        let (mut app, _) = page(Part::Budget);
        press(&mut app, "A MONTH");
        enter(&mut app, "Price of a road, in dollars", "0");
        assert_eq!(
            edits(&app)[0]["set"],
            json!({"section": "budget", "key": "period", "value": "month"})
        );
        assert_eq!(
            edits(&app)[1]["set"],
            json!({"section": "road", "key": "cost", "value": 0.0})
        );
    }
}
