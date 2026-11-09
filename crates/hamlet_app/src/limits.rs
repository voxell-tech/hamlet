//! A hamlet's received limits and the chief's edits through the
//! server.

use std::collections::BTreeMap;

use bevy::app::{App, PreUpdate};
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Query, ResMut};
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, number_field, row,
};
use bevy_fynix::{ScopedExt, ViewExt, keyed, resource};
use hamlet_client::messages::Response;
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::{Bylaws, HamletId, RealmBylaws};
use hamlet_protocol::client::{self as protocol, Limits, Period};
use hamlet_ui::styles::{
    Fills, action, bare, beside, breadcrumb, close, description,
    group, nav_list, nav_row, note, parts, segmented_words,
    text_input,
};
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::widgets::switch::switch as toggle;
use hamlet_ui::{AnyView, View, with_cx};
use serde_json::Value;

use crate::PanelSystems;
use crate::overview::heading;

const CONTROLS: f32 = 150.0;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Book>()
        .init_resource::<Part>()
        .add_systems(PreUpdate, follow.in_set(PanelSystems::Follow))
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

/// `limits` as the map a whole edit sends.
pub(crate) fn whole_of(limits: &Limits) -> BTreeMap<String, Value> {
    match serde_json::to_value(limits) {
        Ok(Value::Object(fields)) => fields.into_iter().collect(),
        _ => unreachable!(
            "Limits is a struct with string keys: it serializes to \
             an object"
        ),
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
    let Some(name) =
        world.get::<HamletId>(hamlet).map(|hamlet| hamlet.0.clone())
    else {
        return;
    };
    let Some(limits) = world.get::<Bylaws>(hamlet) else {
        return;
    };
    let whole = whole_of(&limits.0);
    send(world, Some(name), whole, section, key, value);
}

/// Changes one of the realm's own limits.
fn put_realm(
    world: &mut World,
    section: &'static str,
    key: &'static str,
    value: Value,
) {
    let Some(limits) = world.get_resource::<RealmBylaws>() else {
        return;
    };
    let whole = match serde_json::to_value(&limits.0) {
        Ok(Value::Object(fields)) => fields.into_iter().collect(),
        _ => return,
    };
    send(world, None, whole, section, key, value);
}

/// Sends the edit of `key` in `section`, or of the top-level `key`
/// of `whole` when there is no section. The limits are a hamlet's, or
/// the realm's when `hamlet` is none.
fn send(
    world: &mut World,
    hamlet: Option<String>,
    mut whole: BTreeMap<String, Value>,
    section: &'static str,
    key: &'static str,
    value: Value,
) {
    let whole = if section.is_empty() {
        whole.insert(key.into(), value.clone());
        Some(whole)
    } else {
        None
    };
    let id = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Edit(protocol::Edit::Limits(
            match whole {
                Some(whole) => protocol::EditLimits::Whole {
                    id,
                    hamlet,
                    reason: None,
                    whole,
                },
                None => protocol::EditLimits::Set {
                    id,
                    hamlet,
                    reason: None,
                    set: protocol::LimitsEdit {
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
                        .tone(Tone::Critical)
                        .boxed(),
                );
            }
            close(parts).boxed()
        },
    )
    .within(bare())
    .boxed()
}

fn line(what: &'static str, control: AnyView) -> impl View {
    with_cx(move |cx| {
        row((
            row((note(what),)).fills_width(),
            row((control,)).width(px(CONTROLS)).shrink(0.0),
        ))
        .gap(cx.theme().space.gap)
        .align(AlignItems::Center)
    })
}

fn fine(text: &'static str) -> impl View {
    description(text)
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
        .rules(text_input)
        .boxed()
    })
}

fn cost(
    section: &'static str,
    key: &'static str,
    get: fn(&Limits) -> Option<f64>,
) -> impl View {
    control(section, key, move || {
        number_field(
            resource::<Book, _>(move |book| {
                book.limits.as_ref().and_then(get).unwrap_or(0.0)
            }),
            move |world, value: f64| {
                put(
                    world,
                    section,
                    key,
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
        .rules(text_input)
        .boxed()
    })
}

fn switch(
    section: &'static str,
    key: &'static str,
    get: fn(&Limits) -> bool,
) -> impl View {
    control(section, key, move || {
        toggle(
            resource::<Book, _>(move |book| {
                book.limits.as_ref().is_some_and(get)
            }),
            move |world, on| {
                put(world, section, key, on.into());
            },
        )
        .boxed()
    })
}

/// The budget of `hamlet` and the caps on one task.
fn budget(hamlet: Entity) -> impl View {
    let period = control("budget", "period", || {
        keyed(
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
            |chosen| {
                segmented_words(
                    ["A day", "A month"],
                    *chosen,
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
            },
        )
        .within(bare())
        .boxed()
    });
    group((
        heading(Glyph::Meter, "The budget"),
        fine("0 means no limit."),
        line("It starts again every", period),
        line(
            "May spend, in dollars",
            cost("budget", "cost", |l| l.budget.cost).boxed(),
        ),
        beside((
            action(
                Glyph::Restore,
                "Start it again now",
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
        .tagged(hamlet_ui::widgets::platform::Wraps)
        .boxed(),
        heading(Glyph::Coins, "One task"),
        fine("Counted across every agent the task touches."),
        line(
            "May cost, in dollars",
            cost("task", "cost", |l| l.task.cost).boxed(),
        ),
        line(
            "Messages between agents",
            whole("task", "messages", |l| l.task.messages, 10_000)
                .boxed(),
        ),
    ))
}

/// The leads' acts a chief keeps for itself, and the caps on the
/// tree and its roads.
fn leads() -> impl View {
    group((
        heading(Glyph::Mayor, "Waits for you"),
        fine("What the leads may not do without asking you first."),
        line(
            "Hire a scholar",
            switch("reserved", "hire", |l| l.reserved.hire).boxed(),
        ),
        line(
            "End a hire",
            switch("reserved", "end", |l| l.reserved.end).boxed(),
        ),
        line(
            "Merge hires",
            switch("reserved", "merge", |l| l.reserved.merge).boxed(),
        ),
        line(
            "Lay roads",
            switch("reserved", "roads", |l| l.reserved.roads).boxed(),
        ),
        heading(Glyph::Signpost, "The tree and its roads"),
        line(
            "Deepest tree under the mayor",
            whole("", "depth", |l| l.depth, 1000).boxed(),
        ),
        line(
            "Most roads",
            whole("road", "most", |l| l.road.most, 10_000).boxed(),
        ),
        line(
            "Days a road takes to grow",
            whole(
                "road",
                "growsOverDays",
                |l| l.road.grows_over_days,
                3650,
            )
            .boxed(),
        ),
        line(
            "Price of a road, in dollars",
            cost("road", "price", |l| Some(l.road.price)).boxed(),
        ),
        heading(Glyph::Clock, "What hires are told"),
        line(
            "Days of decisions each hire is told",
            whole("", "toldDays", |l| l.told_days, 3650).boxed(),
        ),
        line(
            "Minutes a chief's task is protected",
            whole(
                "",
                "protectedMinutes",
                |l| l.protected_minutes,
                100_000,
            )
            .boxed(),
        ),
    ))
}

/// The realm's own limits: the Hand's budget and whether mayors may
/// agree roads between their hamlets.
pub fn realm() -> AnyView {
    let period = control("budget", "period", || {
        keyed(
            resource::<RealmBylaws, _>(|limits| {
                match limits.0.budget.period {
                    Period::Day => 0,
                    Period::Month => 1,
                }
            }),
            |chosen| {
                segmented_words(
                    ["A day", "A month"],
                    *chosen,
                    |world, chosen| {
                        let period =
                            if chosen == 0 { "day" } else { "month" };
                        put_realm(
                            world,
                            "budget",
                            "period",
                            serde_json::json!(period),
                        );
                    },
                )
                .boxed()
            },
        )
        .within(bare())
        .boxed()
    });
    let spend = control("budget", "cost", || {
        number_field(
            resource::<RealmBylaws, _>(|limits| {
                limits.0.budget.cost.unwrap_or(0.0)
            }),
            |world, value: f64| {
                put_realm(
                    world,
                    "budget",
                    "cost",
                    serde_json::json!((value > 0.0).then_some(value)),
                );
            },
        )
        .range(0.0, 1_000_000.0)
        .step(0.05)
        .precision(2)
        .width(px(110.0))
        .rules(text_input)
        .boxed()
    });
    let roads = control("", "roadsBetween", || {
        toggle(
            resource::<RealmBylaws, _>(|limits| {
                limits.0.roads_between
            }),
            |world, on| {
                put_realm(world, "", "roadsBetween", on.into());
            },
        )
        .boxed()
    });
    group((
        heading(Glyph::Globe, "The realm"),
        fine("0 means no limit."),
        line("The Hand's budget starts again every", period),
        line("The Hand may spend, in dollars", spend),
        line("Mayors may agree roads between hamlets", roads),
    ))
    .boxed()
}

/// The bylaws of `hamlet`, for its chief to change.
pub fn panel(hamlet: Entity) -> AnyView {
    let page =
        keyed(resource::<Part, _>(|part| *part), move |part| {
            if *part == Part::Front {
                return AnyView::new(
                    move |cx: &mut hamlet_ui::Cx| {
                        let space = &cx.theme().space;
                        let padded = |part: AnyView| {
                            column((part,))
                                .padding(UiRect::axes(
                                    px(space.card_padding_x),
                                    px(space.card_padding_y),
                                ))
                                .boxed()
                        };
                        // The folder it works on and its approvals
                        // lead the list, open where the others lead
                        // to a page.
                        let open = [
                            padded(
                                crate::folder::section(hamlet)
                                    .boxed(),
                            ),
                            padded(
                                crate::folder::approvals(hamlet)
                                    .boxed(),
                            ),
                        ];
                        let rows =
                            Part::PAGES.into_iter().map(|part| {
                                nav_row(
                                    part.glyph(),
                                    part.title(),
                                    Some(
                                        resource::<Book, _>(
                                            move |book| {
                                                book.limits
                                                    .as_ref()
                                                    .map_or(
                                                    part.holds()
                                                        .to_string(),
                                                    |limits| {
                                                        part.now(
                                                            limits,
                                                        )
                                                    },
                                                )
                                            },
                                        )
                                        .into(),
                                    ),
                                    move |world| {
                                        *world
                                            .resource_mut::<Part>() =
                                            part;
                                    },
                                )
                            });
                        let parts = open
                            .into_iter()
                            .chain(rows)
                            .collect::<Vec<_>>();
                        cx.build(with_cx(move |cx| {
                            nav_list(cx, parts)
                        }))
                    },
                );
            }
            let body = match part {
                Part::Budget => budget(hamlet).boxed(),
                Part::Models => {
                    crate::models::of_hamlet(hamlet).boxed()
                }
                Part::Front | Part::Leads => leads().boxed(),
            };
            parts((
                breadcrumb(
                    vec!["Bylaws".into()],
                    part.title(),
                    |world, _| {
                        *world.resource_mut::<Part>() = Part::Front;
                    },
                ),
                body,
            ))
            .boxed()
        })
        .within(bare());
    AnyView::new(move |cx: &mut hamlet_ui::Cx| {
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
    Leads,
}

impl Part {
    const PAGES: [Part; 3] =
        [Part::Budget, Part::Models, Part::Leads];

    fn title(self) -> &'static str {
        match self {
            Part::Front => "Bylaws",
            Part::Budget => "Budget",
            Part::Models => "Models",
            Part::Leads => "Leads and limits",
        }
    }

    fn glyph(self) -> Glyph {
        match self {
            Part::Front => Glyph::Book,
            Part::Budget => Glyph::Meter,
            Part::Models => Glyph::Chip,
            Part::Leads => Glyph::Mayor,
        }
    }

    /// What the part holds now, as its row on the front says it.
    fn now(self, limits: &Limits) -> String {
        let dollars = |cost: Option<f64>| match cost {
            Some(cost) if cost > 0.0 => format!("${cost:.2}"),
            _ => "No limit".to_string(),
        };
        match self {
            Part::Front => String::new(),
            Part::Budget => format!(
                "{} {}, {} a task",
                dollars(limits.budget.cost),
                match limits.budget.period {
                    Period::Day => "a day",
                    Period::Month => "a month",
                },
                dollars(limits.task.cost).to_lowercase(),
            ),
            Part::Models => match &limits.model {
                Some(model) => {
                    format!(
                        "Runs {}",
                        crate::models::model_name(model)
                    )
                }
                None => "Runs the realm's default model".to_string(),
            },
            Part::Leads => {
                let kept = [
                    (limits.reserved.hire, "hire"),
                    (limits.reserved.end, "end a hire"),
                    (limits.reserved.merge, "merge"),
                    (limits.reserved.roads, "link agents"),
                ]
                .into_iter()
                .filter(|(kept, _)| *kept)
                .map(|(_, what)| what)
                .collect::<Vec<_>>();
                let asks = match kept.split_last() {
                    None => {
                        "Leads act without asking you".to_string()
                    }
                    Some((last, [])) => {
                        format!("Leads ask you before they {last}")
                    }
                    Some((last, rest)) => format!(
                        "Leads ask you before they {} or {last}",
                        rest.join(", ")
                    ),
                };
                format!(
                    "{asks}. Hires go {} levels deep at most, with \
                     up to {} links between agents.",
                    limits.depth, limits.road.most
                )
            }
        }
    }

    fn holds(self) -> &'static str {
        match self {
            Part::Front => "",
            Part::Budget => "The budget, and the caps on one task.",
            Part::Models => "Which models this hamlet may run.",
            Part::Leads => {
                "What the leads may do without asking, and the caps \
                 on hires and the links between agents."
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
    use crate::testing::panel_app;

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
            .query::<(Entity, &HamletId)>()
            .iter(app.world())
            .next()
            .unwrap()
            .0;
        *app.world_mut().resource_mut::<Part>() = part;
        bevy_fynix::mount::<hamlet_ui::theme::HamletTheme>(
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
        app.update();
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
    fn the_realms_limits_are_edited_without_a_hamlet() {
        let mut app = panel_app();
        app.add_plugins((
            plugin,
            InputPlugin,
            InputFocusPlugin,
            InputDispatchPlugin,
        ));
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        bevy_fynix::mount::<hamlet_ui::theme::HamletTheme>(
            app.world_mut(),
            realm(),
        );
        app.update();
        enter(&mut app, "The Hand may spend, in dollars", "2");
        let sent = edits(&app);
        assert_eq!(sent[0]["doc"], "limits");
        assert!(sent[0].get("hamlet").is_none());
        assert_eq!(
            sent[0]["set"],
            json!({"section": "budget", "key": "cost", "value": 2.0})
        );
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

        use crate::testing::words;
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
        app.update();
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
        let mut hamlet = hamlet_testing::realm::hamlet("home", 2);
        hamlet.limits.budget.cost = Some(3.5);
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::Change(protocol::Changes {
                epoch: 1,
                revision: 1,
                changes: vec![protocol::Update::Hamlet { hamlet }],
            }),
        );
        app.update();
        let field = input(&mut app, "May spend, in dollars");
        assert_eq!(
            app.world().get::<EditableText>(field).unwrap().value(),
            "3.50"
        );
    }

    #[test]
    fn a_top_level_edit_keeps_the_other_received_limits_and_section_keys_are_camel_case()
     {
        let (mut app, _) = page(Part::Leads);
        enter(&mut app, "Deepest tree under the mayor", "8");
        let mut expected = serde_json::to_value(
            hamlet_testing::realm::hamlet("home", 2).limits,
        )
        .unwrap();
        expected["depth"] = json!(8);
        let sent = edits(&app);
        assert_eq!(sent[0]["whole"], expected);
        assert!(sent[0].get("set").is_none());
        enter(&mut app, "Days a road takes to grow", "4");
        assert_eq!(
            edits(&app)[1]["set"],
            json!({"section": "road", "key": "growsOverDays", "value": 4})
        );
    }

    #[test]
    fn period_is_text_and_a_free_road_is_not_an_uncapped_meter() {
        use crate::testing::press;
        let (mut app, _) = page(Part::Budget);
        press(&mut app, "A month");
        assert_eq!(
            edits(&app)[0]["set"],
            json!({"section": "budget", "key": "period", "value": "month"})
        );
        *app.world_mut().resource_mut::<Part>() = Part::Leads;
        app.update();
        enter(&mut app, "Price of a road, in dollars", "0");
        assert_eq!(
            edits(&app)[1]["set"],
            json!({"section": "road", "key": "price", "value": 0.0})
        );
    }
}
