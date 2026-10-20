//! Petitions: questions for the person about a plugin, each a child
//! of the plugin's entity, listed in the Petitions panel. A plugin
//! that is installed is asked about once it comes, unless the person
//! answered for this version already, and a plugin that asks for
//! more than it was answered for is asked again. A use of a
//! capability answered Ask next time asks the person first.

use std::collections::HashMap;
use std::time::Duration;

use bevy::app::{App, Startup, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::message::{Message, MessageReader, MessageWriter};
use bevy::ecs::observer::On;
use bevy::ecs::query::{Added, Changed, Has};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::time::{Real, Time};
use bevy_fynix::dock::DockTree;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, label,
};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_engine_api::id::ENGINE;
use hamlet_engine_api::{Ask, Capability, PanelId};
use hamlet_engine_core::{Budgets, LogLine};
use hamlet_engine_plugins::{Enable, PluginInfo};
use hamlet_engine_shell::dock::PanelIcon;
use hamlet_engine_shell::icons::Glyph;
use hamlet_engine_shell::style::{
    ButtonStyle, buttons, caption, card, choices, marked, note,
    scroll_area, section,
};
use hamlet_engine_shell::{AnyView, Cx, Panel, dock, panel};

use crate::Admitted;
use crate::file::{Answer, Grants, Record};

/// The name of the Petitions panel.
pub const PETITIONS: &str = "petitions";

pub(crate) fn plugin(app: &mut App) {
    app.add_message::<FirstUse>()
        .add_message::<Decided>()
        .init_resource::<Rows>()
        .add_observer(ask_again)
        .add_systems(Startup, open)
        .add_systems(Update, (admit, first_use, follow).chain());
}

/// A question for the person, on a child of the plugin it is about.
#[derive(Component, Clone, PartialEq, Debug)]
pub struct Petition(pub Question);

#[derive(Clone, PartialEq, Debug)]
pub enum Question {
    /// Whether the plugin may be added, with what it asks for and the
    /// answer chosen for each so far. `more` when the person let an
    /// earlier version in, and these are what it asks for besides.
    Install {
        asks: Vec<Ask>,
        chosen: Vec<Answer>,
        more: bool,
    },
    /// A use of what the plugin asked for, answered Ask next time.
    Use(Ask),
}

/// A plugin wants to use `capability` on `target`, which the person
/// answered Ask next time: they are asked, and [`Decided`] says what
/// they answered.
#[derive(Message, Clone, Debug)]
pub struct FirstUse {
    pub plugin: Entity,
    pub capability: Capability,
    pub target: String,
}

/// The person answered a [`FirstUse`]: whether this use may go on.
#[derive(Message, Clone, Debug)]
pub struct Decided {
    pub plugin: Entity,
    pub capability: Capability,
    pub target: String,
    pub allowed: bool,
}

/// The petitions as the Petitions panel shows them.
#[derive(Resource, Clone, PartialEq, Default)]
struct Rows(Vec<Row>);

#[derive(Clone, PartialEq)]
struct Row {
    petition: Entity,
    /// The line over its card: what it is and from which plugin.
    caption: String,
    title: String,
    question: Question,
}

fn open(mut commands: Commands) {
    commands.spawn((
        Panel {
            id: PanelId::new(ENGINE, PETITIONS).expect("a valid id"),
            title: "Petitions".into(),
            content: view,
        },
        PanelIcon(Glyph::Bell),
    ));
}

/// Switches on each installed plugin the person let in, and asks
/// about the others.
fn admit(
    plugins: Query<(Entity, &PluginInfo), Added<PluginInfo>>,
    mut grants: ResMut<Grants>,
    mut tree: Option<ResMut<DockTree>>,
    mut log: MessageWriter<LogLine>,
    mut commands: Commands,
) {
    for (plugin, info) in &plugins {
        if info.builtin {
            continue;
        }
        let record = grants.record(&info.id).cloned();
        let fresh = |record: &Record| {
            info.asks
                .iter()
                .filter(|ask| {
                    record
                        .answer(ask.capability, &ask.target)
                        .is_none()
                })
                .cloned()
                .collect::<Vec<_>>()
        };
        let question = match record {
            Some(record)
                if record.allowed && fresh(&record).is_empty() =>
            {
                if record.version != info.version {
                    let record = Record {
                        version: info.version.clone(),
                        ..record
                    };
                    keep(&mut grants, info, record, &mut log);
                }
                commands.entity(plugin).insert(Admitted);
                commands.trigger(Enable { entity: plugin });
                continue;
            }
            Some(record)
                if !record.allowed
                    && record.version == info.version =>
            {
                log.write(LogLine::info(
                    "grants",
                    format!(
                        "{} was not allowed; switch it on to be \
                         asked again",
                        info.name
                    ),
                ));
                continue;
            }
            Some(record) if record.allowed => Question::Install {
                chosen: vec![Answer::Ask; fresh(&record).len()],
                asks: fresh(&record),
                more: true,
            },
            _ => install(info),
        };
        raise(
            &mut commands,
            tree.as_deref_mut(),
            &mut log,
            plugin,
            info,
            question,
        );
    }
}

/// Asks again about an installed plugin the person did not let in,
/// as it is switched on.
fn ask_again(
    event: On<Enable>,
    plugins: Query<(&PluginInfo, Has<Admitted>, Option<&Children>)>,
    petitions: Query<&Petition>,
    mut tree: Option<ResMut<DockTree>>,
    mut log: MessageWriter<LogLine>,
    mut commands: Commands,
) {
    let plugin = event.entity;
    let Ok((info, admitted, children)) = plugins.get(plugin) else {
        return;
    };
    let asking = children.into_iter().flatten().any(|child| {
        petitions.get(*child).is_ok_and(|petition| {
            matches!(petition.0, Question::Install { .. })
        })
    });
    if info.builtin || admitted || asking {
        return;
    }
    raise(
        &mut commands,
        tree.as_deref_mut(),
        &mut log,
        plugin,
        info,
        install(info),
    );
}

/// Whether the plugin `info` may be added, with all it asks for.
fn install(info: &PluginInfo) -> Question {
    Question::Install {
        asks: info.asks.clone(),
        chosen: vec![Answer::Ask; info.asks.len()],
        more: false,
    }
}

/// Asks about each use of a plugin answered Ask next time, as many a
/// minute as the budgets allow; the rest are refused. `raised` holds,
/// for each plugin, when its minute began and how many it raised.
fn first_use(
    mut uses: MessageReader<FirstUse>,
    plugins: Query<(&PluginInfo, Option<&Children>)>,
    petitions: Query<&Petition>,
    mut raised: Local<HashMap<Entity, (Duration, u32)>>,
    budgets: Res<Budgets>,
    time: Res<Time<Real>>,
    mut tree: Option<ResMut<DockTree>>,
    mut decided: MessageWriter<Decided>,
    mut log: MessageWriter<LogLine>,
    mut commands: Commands,
) {
    let now = time.elapsed();
    for wanted in uses.read() {
        let Ok((info, children)) = plugins.get(wanted.plugin) else {
            continue;
        };
        let Some(ask) = info
            .asks
            .iter()
            .find(|ask| {
                ask.capability == wanted.capability
                    && ask.target == wanted.target
            })
            .cloned()
        else {
            continue;
        };
        let open = children.into_iter().flatten().any(|child| {
            petitions.get(*child).is_ok_and(|petition| {
                petition.0 == Question::Use(ask.clone())
            })
        });
        if open {
            continue;
        }
        let (since, count) = raised
            .get(&wanted.plugin)
            .copied()
            .filter(|(since, _)| {
                now.saturating_sub(*since) < Duration::from_secs(60)
            })
            .unwrap_or((now, 0));
        if count >= budgets.petitions_per_minute {
            decided.write(Decided {
                plugin: wanted.plugin,
                capability: wanted.capability,
                target: wanted.target.clone(),
                allowed: false,
            });
            continue;
        }
        raised.insert(wanted.plugin, (since, count + 1));
        raise(
            &mut commands,
            tree.as_deref_mut(),
            &mut log,
            wanted.plugin,
            info,
            Question::Use(ask),
        );
    }
}

/// Puts `question` about `plugin` to the person, in the Petitions
/// panel.
fn raise(
    commands: &mut Commands,
    tree: Option<&mut DockTree>,
    log: &mut MessageWriter<LogLine>,
    plugin: Entity,
    info: &PluginInfo,
    question: Question,
) {
    commands.spawn((Petition(question), ChildOf(plugin)));
    log.write(LogLine::info(
        "grants",
        format!("{} has a petition: see Petitions", info.name),
    ));
    if let (Some(tree), Ok(id)) =
        (tree, PanelId::new(ENGINE, PETITIONS))
    {
        dock::open(tree, id.as_str());
    }
}

/// Keeps `record` for the plugin `info`, telling the log when it
/// cannot be written.
fn keep(
    grants: &mut Grants,
    info: &PluginInfo,
    record: Record,
    log: &mut MessageWriter<LogLine>,
) {
    if let Err(why) = grants.set(&info.id, record) {
        log.write(LogLine::error(
            "grants",
            format!("{} cannot be written: {why}", crate::FILE),
        ));
    }
}

/// Gathers the rows again when a petition comes, changes or goes.
fn follow(
    changed: Query<(), Changed<Petition>>,
    mut gone: RemovedComponents<Petition>,
    petitions: Query<(Entity, &Petition, &ChildOf)>,
    plugins: Query<&PluginInfo>,
    mut rows: ResMut<Rows>,
) {
    if changed.is_empty() && gone.read().count() == 0 {
        return;
    }
    let mut now = petitions
        .iter()
        .filter_map(|(petition, Petition(question), parent)| {
            let info = plugins.get(parent.parent()).ok()?;
            let (caption, title) = match question {
                Question::Install { more: false, .. } => (
                    "TO ADD",
                    format!(
                        "{} {} wants to be added.",
                        info.name, info.version
                    ),
                ),
                Question::Install { more: true, .. } => (
                    "TO UPDATE",
                    format!(
                        "{} {} asks for more.",
                        info.name, info.version
                    ),
                ),
                Question::Use(_) => (
                    "A FIRST USE",
                    format!("{} wants to use:", info.name),
                ),
            };
            Some(Row {
                petition,
                caption: format!(
                    "{caption} FROM {}",
                    info.name.to_uppercase()
                ),
                title,
                question: question.clone(),
            })
        })
        .collect::<Vec<_>>();
    now.sort_by_key(|row| row.petition);
    if rows.0 != now {
        rows.0 = now;
    }
}

/// How many petitions wait, over a card for each.
fn view(_: Entity) -> AnyView {
    let count =
        section(resource::<Rows, _>(|rows| match rows.0.len() {
            0 => "NO PETITIONS".to_owned(),
            1 => "1 PETITION".to_owned(),
            n => format!("{n} PETITIONS"),
        }));
    let cards = keyed(resource::<Rows, _>(Clone::clone), |rows| {
        if rows.0.is_empty() {
            return note("What a plugin asks of you waits here.");
        }
        let cards = rows.0.iter().map(row_view).collect::<Vec<_>>();
        AnyView::new(move |cx: &mut Cx| {
            let gap = cx.theme().space.gap;
            cx.build(column(cards).gap(gap))
        })
    })
    .within(column(()).gap(0.0));
    panel(AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.section;
        cx.build(column((count, scroll_area((cards,)))).gap(gap))
    }))
}

/// A petition's card: what it is, about which plugin, what it asks
/// and its answers.
fn row_view(row_of: &Row) -> AnyView {
    let petition = row_of.petition;
    let head = caption(row_of.caption.clone(), Tone::Accent);
    let title = row_of.title.clone();
    let (asks, answers) = match &row_of.question {
        Question::Install { asks, chosen, .. } => {
            let asks = asks
                .iter()
                .zip(chosen)
                .enumerate()
                .map(|(at, (ask, answer))| {
                    let picked = Answer::ALL
                        .iter()
                        .position(|a| a == answer)
                        .unwrap_or(1);
                    ask_view(
                        ask,
                        choices(
                            Answer::ALL
                                .map(|answer| {
                                    answer.label().to_owned()
                                })
                                .to_vec(),
                            picked,
                            move |world, picked| {
                                choose(world, petition, at, picked)
                            },
                        ),
                    )
                })
                .collect::<Vec<_>>();
            let answers = buttons(vec![
                marked(ButtonStyle::Primary, Glyph::Check, "ALLOW")
                    .on_activate(move |world| {
                        answer_install(world, petition, true)
                    })
                    .boxed(),
                marked(
                    ButtonStyle::Critical,
                    Glyph::Cross,
                    "DON'T ALLOW",
                )
                .on_activate(move |world| {
                    answer_install(world, petition, false)
                })
                .boxed(),
            ]);
            (asks, answers)
        }
        Question::Use(ask) => {
            let answers = Answer::ALL
                .iter()
                .map(|&answer| {
                    let (kind, which) = match answer {
                        Answer::Allow => {
                            (ButtonStyle::Primary, Glyph::Check)
                        }
                        Answer::Ask => {
                            (ButtonStyle::Plain, Glyph::Restore)
                        }
                        Answer::Deny => {
                            (ButtonStyle::Critical, Glyph::Cross)
                        }
                    };
                    marked(kind, which, answer.label().to_uppercase())
                        .on_activate(move |world| {
                            answer_use(world, petition, answer)
                        })
                        .boxed()
                })
                .collect::<Vec<_>>();
            (vec![ask_view(ask, frame().boxed())], buttons(answers))
        }
    };
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (size, gap) = (theme.text.heading, theme.space.gap);
        cx.build(card((
            head,
            label(title).size(size),
            column(asks).gap(gap),
            answers,
        )))
    })
}

/// An ask, its reason in the plugin's words, and `answers` under it.
fn ask_view(ask: &Ask, answers: AnyView) -> AnyView {
    let (what, why) = (
        format!("{} {}", ask.capability.name(), ask.target),
        ask.reason.clone(),
    );
    AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.gap / 2.0;
        cx.build(column((label(what), note(why), answers)).gap(gap))
    })
}

/// Chooses the answer at `picked` for the ask at `at` of an install
/// petition.
fn choose(
    world: &mut World,
    petition: Entity,
    at: usize,
    picked: usize,
) {
    let Some(mut petition) = world.get_mut::<Petition>(petition)
    else {
        return;
    };
    if let Question::Install { chosen, .. } = &mut petition.0
        && let (Some(answer), Some(&now)) =
            (chosen.get_mut(at), Answer::ALL.get(picked))
    {
        *answer = now;
    }
}

/// Answers an install petition: the plugin is let in, with the
/// answers chosen, and switched on, or it is not.
pub fn answer_install(
    world: &mut World,
    petition: Entity,
    allowed: bool,
) {
    let Some((Petition(question), plugin)) = parts(world, petition)
    else {
        return;
    };
    let Question::Install { asks, chosen, .. } = question else {
        return;
    };
    let Some(info) = world.get::<PluginInfo>(plugin).cloned() else {
        return;
    };
    world.despawn(petition);
    let mut grants = world.resource_mut::<Grants>();
    let mut record = grants
        .record(&info.id)
        .filter(|record| record.allowed)
        .cloned()
        .unwrap_or_else(|| Record {
            allowed,
            version: info.version.clone(),
            answers: Vec::new(),
        });
    record.allowed = allowed;
    record.version.clone_from(&info.version);
    if allowed {
        for (ask, answer) in asks.iter().zip(chosen) {
            record.set(ask.capability, &ask.target, answer);
        }
    }
    let written = grants.set(&info.id, record);
    let said = if allowed {
        "is allowed"
    } else {
        "is not allowed"
    };
    world.write_message(LogLine::info(
        "grants",
        format!("{} {said}", info.name),
    ));
    if let Err(why) = written {
        world.write_message(LogLine::error(
            "grants",
            format!("{} cannot be written: {why}", crate::FILE),
        ));
    }
    if allowed {
        world.entity_mut(plugin).insert(Admitted);
        world.trigger(Enable { entity: plugin });
    }
}

/// Answers the petition of a use: Allow and Don't allow are kept,
/// and Ask next time lets this use go on.
pub fn answer_use(
    world: &mut World,
    petition: Entity,
    answer: Answer,
) {
    let Some((Petition(Question::Use(ask)), plugin)) =
        parts(world, petition)
    else {
        return;
    };
    let Some(info) = world.get::<PluginInfo>(plugin).cloned() else {
        return;
    };
    world.despawn(petition);
    let mut grants = world.resource_mut::<Grants>();
    let written = match grants.record(&info.id).cloned() {
        Some(mut record) if answer != Answer::Ask => {
            record.set(ask.capability, &ask.target, answer);
            grants.set(&info.id, record)
        }
        _ => Ok(()),
    };
    if let Err(why) = written {
        world.write_message(LogLine::error(
            "grants",
            format!("{} cannot be written: {why}", crate::FILE),
        ));
    }
    world.write_message(Decided {
        plugin,
        capability: ask.capability,
        target: ask.target,
        allowed: answer != Answer::Deny,
    });
}

/// The petition `petition` and its plugin.
fn parts(
    world: &World,
    petition: Entity,
) -> Option<(Petition, Entity)> {
    let entity = world.get_entity(petition).ok()?;
    Some((
        entity.get::<Petition>()?.clone(),
        entity.get::<ChildOf>()?.parent(),
    ))
}
