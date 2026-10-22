//! Petitions: questions for the person about a plugin, each a child
//! of the plugin's entity, listed in the Petitions panel. A plugin
//! that is installed is asked about once it comes, unless the person
//! answered for this version already, and a plugin that asks for
//! more than it was answered for is asked again. A use of a
//! capability answered Ask next time asks the person first.
//!
//! The panel holds everything that waits on the person: the sections
//! plugins add to it first, then these questions under PLUGINS, each
//! part shown while it holds something.

use std::collections::HashMap;
use std::time::Duration;

use bevy::app::{App, Startup, Update};
use bevy::ecs::change_detection::DetectChanges;
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
use bevy::ui::{UiRect, px};
use bevy_fynix::dock::DockTree;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, label,
};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_engine_api::id::ENGINE;
use hamlet_engine_api::{Ask, Capability, PanelId, PluginId};
use hamlet_engine_core::{Budgets, LogLine};
use hamlet_engine_plugins::{
    ContributedBy, Enable, PluginInfo, PluginKind, SwitchedOff,
    resolve,
};
use hamlet_engine_shell::dock::PanelIcon;
use hamlet_engine_shell::icons::Glyph;
use hamlet_engine_shell::style::{
    ButtonStyle, buttons, caption, card, choices, heading,
    icon_button, note, scroll_area, section,
};
use hamlet_engine_shell::{
    AnyView, Cx, DockTreeExt, Panel, PanelSection, SettingsPage,
    ShellWorldExt, StatusItem, StyleExt, section_content,
};

use crate::Admitted;
use crate::file::{Answer, Grants, Record};
use crate::page::{PERMISSIONS, Permissions};

/// The name of the Petitions panel, and of its status item.
pub const PETITIONS: &str = "petitions";

/// What the Petitions panel says when nothing waits.
const EMPTY: &str = "Nothing waits for you. Petitions, objections, \
                     questions and faults come here, oldest first.";

pub(crate) fn plugin(app: &mut App) {
    app.add_message::<FirstUse>()
        .add_message::<Decided>()
        .init_resource::<Rows>()
        .init_resource::<Parts>()
        .init_resource::<Waiting>()
        .add_observer(ask_again)
        .add_systems(Startup, open)
        .add_systems(
            Update,
            // Plugins let in together are blocked or not together.
            (admit.before(resolve), first_use, follow, gather)
                .chain(),
        );
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

/// The sections plugins added to the Petitions panel that hold
/// something, in the order it shows them.
#[derive(Resource, Clone, PartialEq, Default)]
struct Parts(Vec<Part>);

#[derive(Clone, PartialEq)]
struct Part {
    section: Entity,
    head: String,
}

/// How many things wait on the person, in the sections and the
/// petitions together.
#[derive(Resource, Clone, Copy, PartialEq, Default)]
struct Waiting(u32);

fn open(mut commands: Commands) {
    let id = PanelId::new(ENGINE, PETITIONS).expect("a valid id");
    commands.spawn(StatusItem::new(id.clone(), false));
    commands.spawn((
        Panel {
            id,
            title: "Petitions".into(),
            content: view,
        },
        PanelIcon(Glyph::Bell),
    ));
}

/// Switches on each installed plugin the person let in and did not
/// switch off, and asks about the others. A plugin that comes with
/// Hamlet is let in with all it asks, and nobody is asked.
fn admit(
    plugins: Query<(Entity, &PluginInfo), Added<PluginInfo>>,
    mut grants: ResMut<Grants>,
    off: Res<SwitchedOff>,
    mut tree: Option<ResMut<DockTree>>,
    mut log: MessageWriter<LogLine>,
    mut commands: Commands,
) {
    for (plugin, info) in &plugins {
        match info.kind {
            PluginKind::BuiltIn => continue,
            PluginKind::Bundled => {
                let record =
                    Record::bundled(&info.version, &info.asks);
                grants.keep(info, record, &mut log);
                commands.entity(plugin).insert(Admitted);
                if !off.contains(&info.id) {
                    commands.trigger(Enable { entity: plugin });
                }
                continue;
            }
            PluginKind::Installed => {}
        }
        // What was allowed while a plugin of this id came with Hamlet
        // is no answer for one installed.
        let record = grants
            .record(&info.id)
            .filter(|record| !record.bundled)
            .cloned();
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
                    grants.keep(info, record, &mut log);
                }
                commands.entity(plugin).insert(Admitted);
                if !off.contains(&info.id) {
                    commands.trigger(Enable { entity: plugin });
                }
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
            _ => Question::install(info),
        };
        commands.raise_petition(
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
    if info.kind != PluginKind::Installed || admitted || asking {
        return;
    }
    commands.raise_petition(
        tree.as_deref_mut(),
        &mut log,
        plugin,
        info,
        Question::install(info),
    );
}

impl Question {
    /// Whether the plugin `info` may be added, with all it asks for.
    fn install(info: &PluginInfo) -> Self {
        Self::Install {
            asks: info.asks.clone(),
            chosen: vec![Answer::Ask; info.asks.len()],
            more: false,
        }
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
    // The petitions raised here are not children until the end.
    let mut asking = Vec::new();
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
        let question = Question::Use(ask);
        let open = children.into_iter().flatten().any(|child| {
            petitions
                .get(*child)
                .is_ok_and(|petition| petition.0 == question)
        }) || asking
            .contains(&(wanted.plugin, question.clone()));
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
        asking.push((wanted.plugin, question.clone()));
        commands.raise_petition(
            tree.as_deref_mut(),
            &mut log,
            wanted.plugin,
            info,
            question,
        );
    }
}

/// The grants' methods on [`Commands`].
pub trait GrantsCommandsExt {
    /// Puts `question` about `plugin` to the person, in the Petitions
    /// panel, which is brought to the front in `tree`.
    fn raise_petition(
        &mut self,
        tree: Option<&mut DockTree>,
        log: &mut MessageWriter<LogLine>,
        plugin: Entity,
        info: &PluginInfo,
        question: Question,
    );
}

impl GrantsCommandsExt for Commands<'_, '_> {
    fn raise_petition(
        &mut self,
        tree: Option<&mut DockTree>,
        log: &mut MessageWriter<LogLine>,
        plugin: Entity,
        info: &PluginInfo,
        question: Question,
    ) {
        self.spawn((Petition(question), ChildOf(plugin)));
        log.write(LogLine::info(
            "grants",
            format!("{} has a petition: see Petitions", info.name),
        ));
        if let (Some(tree), Ok(id)) =
            (tree, PanelId::new(ENGINE, PETITIONS))
        {
            tree.open_panel(id.as_str());
        }
    }
}

impl Grants {
    /// Keeps `record` for the plugin `info`, telling the log when it
    /// cannot be written.
    fn keep(
        &mut self,
        info: &PluginInfo,
        record: Record,
        log: &mut MessageWriter<LogLine>,
    ) {
        if let Err(why) = self.set(&info.id, record) {
            log.write(LogLine::error(
                "grants",
                format!("{} cannot be written: {why}", crate::FILE),
            ));
        }
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

/// Gathers the sections of the Petitions panel that hold something,
/// those of plugins that come with Hamlet first, and counts what
/// waits, for the panel and its status item. An installed plugin's
/// section is headed by its name too, so it never passes for the
/// engine's.
fn gather(
    changed: Query<(), Changed<PanelSection>>,
    mut gone: RemovedComponents<PanelSection>,
    sections: Query<(Entity, &PanelSection, &ContributedBy)>,
    plugins: Query<&PluginInfo>,
    rows: Res<Rows>,
    mut parts: ResMut<Parts>,
    mut waiting: ResMut<Waiting>,
    mut items: Query<&mut StatusItem>,
) {
    if changed.is_empty()
        && gone.read().count() == 0
        && !rows.is_changed()
    {
        return;
    }
    let Ok(here) = PanelId::new(ENGINE, PETITIONS) else {
        return;
    };
    let mut held = sections
        .iter()
        .filter(|(_, section, _)| {
            section.panel == here && section.holds > 0
        })
        .filter_map(|(entity, section, by)| {
            let info = plugins.get(by.0).ok()?;
            let installed = info.kind == PluginKind::Installed;
            let head = if installed {
                format!("{} FROM {}", section.title, info.name)
            } else {
                section.title.clone()
            };
            Some((
                (installed, info.id.clone(), entity),
                section.holds,
                head,
            ))
        })
        .collect::<Vec<_>>();
    held.sort_by(|(a, ..), (b, ..)| a.cmp(b));
    let count = held
        .iter()
        .map(|(_, holds, _)| *holds)
        .fold(0, u32::saturating_add)
        .saturating_add(
            u32::try_from(rows.0.len()).unwrap_or(u32::MAX),
        );
    let now = Parts(
        held.into_iter()
            .map(|((.., section), _, head)| Part {
                section,
                head: head.to_uppercase(),
            })
            .collect(),
    );
    if *parts != now {
        *parts = now;
    }
    if waiting.0 == count {
        return;
    }
    waiting.0 = count;
    if let Some(mut item) =
        items.iter_mut().find(|item| item.id == here)
    {
        item.icon = Some(Glyph::Bell);
        item.tone = Tone::Accent;
        item.tip = "Petitions".into();
        item.text = match count {
            0 => String::new(),
            n => format!("{n} waiting for you"),
        };
    }
}

/// How many things wait, over each part that holds something: the
/// sections plugins add, then the petitions under PLUGINS.
fn view(_: Entity) -> AnyView {
    let count =
        heading(resource::<Waiting, _>(|waiting| match waiting.0 {
            0 => "NOTHING NEEDS YOU".to_owned(),
            1 => "1 THING NEEDS YOU".to_owned(),
            n => format!("{n} THINGS NEED YOU"),
        }));
    let empty = keyed(
        resource::<Waiting, _>(|waiting| waiting.0 == 0),
        |&none| {
            if none {
                note(EMPTY)
            } else {
                column(()).boxed()
            }
        },
    )
    .within(column(()).gap(0.0));
    let sections =
        keyed(resource::<Parts, _>(Clone::clone), |parts| {
            let shown = parts
                .0
                .iter()
                .map(|at| {
                    part(at.head.clone(), section_content(at.section))
                })
                .collect::<Vec<_>>();
            column(shown).gap(0.0).boxed()
        })
        .within(column(()).gap(0.0));
    let petitions =
        keyed(resource::<Rows, _>(Clone::clone), |rows| {
            if rows.0.is_empty() {
                return column(()).boxed();
            }
            let cards =
                rows.0.iter().map(row_view).collect::<Vec<_>>();
            part(
                "PLUGINS".to_owned(),
                AnyView::new(move |cx: &mut Cx| {
                    let gap = cx.theme().space.gap;
                    cx.build(column(cards).gap(gap))
                }),
            )
        })
        .within(column(()).gap(0.0));
    AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.section;
        cx.build(
            column((
                count,
                scroll_area((
                    column((empty, sections, petitions)).gap(0.0),
                )),
            ))
            .gap(gap)
            .grow(1.0)
            .min_height(px(0.0)),
        )
    })
    .in_panel()
}

/// A part of the Petitions panel: `content` under its `head`.
fn part(head: String, content: AnyView) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let space = &cx.theme().space;
        let (gap, below) = (space.gap, space.section);
        cx.build(
            column((section(head), content))
                .gap(gap)
                .padding(UiRect::bottom(px(below))),
        )
    })
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
                                world.choose_answer(
                                    petition, at, picked,
                                )
                            },
                        ),
                    )
                })
                .collect::<Vec<_>>();
            let answers = buttons(vec![
                icon_button(
                    ButtonStyle::Primary,
                    Glyph::Check,
                    "ALLOW",
                )
                .on_activate(move |world| {
                    world.answer_install(petition, true)
                })
                .boxed(),
                icon_button(
                    ButtonStyle::Critical,
                    Glyph::Cross,
                    "DON'T ALLOW",
                )
                .on_activate(move |world| {
                    world.answer_install(petition, false)
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
                    icon_button(
                        kind,
                        which,
                        answer.label().to_uppercase(),
                    )
                    .on_activate(move |world| {
                        world.answer_use(petition, answer)
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

/// The grants' methods on [`World`].
pub trait GrantsWorldExt {
    /// Chooses the answer at `picked` for the ask at `at` of an
    /// install petition.
    fn choose_answer(
        &mut self,
        petition: Entity,
        at: usize,
        picked: usize,
    );

    /// Answers an install petition: the plugin is let in, with the
    /// answers chosen, and switched on, or it is not.
    fn answer_install(&mut self, petition: Entity, allowed: bool);

    /// Answers the petition of a use: Allow and Don't allow are kept,
    /// and Ask next time lets this use go on.
    fn answer_use(&mut self, petition: Entity, answer: Answer);

    /// Changes the answer of the plugin `id` to `capability` on
    /// `target`, telling the log when it cannot be written. What a
    /// plugin that comes with Hamlet is allowed is not changed.
    fn change_answer(
        &mut self,
        id: &PluginId,
        capability: Capability,
        target: &str,
        answer: Answer,
    );

    /// The petition `petition` and its plugin.
    fn petition(
        &self,
        petition: Entity,
    ) -> Option<(Petition, Entity)>;

    /// Shows the Permissions page in Settings with the answers of the
    /// plugin `id` alone.
    fn show_permissions(&mut self, id: &PluginId);
}

impl GrantsWorldExt for World {
    fn choose_answer(
        &mut self,
        petition: Entity,
        at: usize,
        picked: usize,
    ) {
        let Some(mut petition) = self.get_mut::<Petition>(petition)
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

    fn answer_install(&mut self, petition: Entity, allowed: bool) {
        let Some((Petition(question), plugin)) =
            self.petition(petition)
        else {
            return;
        };
        let Question::Install { asks, chosen, .. } = question else {
            return;
        };
        let Some(info) = self.get::<PluginInfo>(plugin).cloned()
        else {
            return;
        };
        self.despawn(petition);
        let mut grants = self.resource_mut::<Grants>();
        let mut record = grants
            .record(&info.id)
            .filter(|record| record.allowed && !record.bundled)
            .cloned()
            .unwrap_or_else(|| Record {
                allowed,
                version: info.version.clone(),
                bundled: false,
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
        self.write_message(LogLine::info(
            "grants",
            format!("{} {said}", info.name),
        ));
        if let Err(why) = written {
            self.write_message(LogLine::error(
                "grants",
                format!("{} cannot be written: {why}", crate::FILE),
            ));
        }
        if allowed {
            let kept = self
                .resource_mut::<SwitchedOff>()
                .set(&info.id, false);
            if let Err(why) = kept {
                self.write_message(LogLine::error("grants", why));
            }
            self.entity_mut(plugin).insert(Admitted);
            self.trigger(Enable { entity: plugin });
        }
    }

    fn answer_use(&mut self, petition: Entity, answer: Answer) {
        let Some((Petition(Question::Use(ask)), plugin)) =
            self.petition(petition)
        else {
            return;
        };
        let Some(info) = self.get::<PluginInfo>(plugin).cloned()
        else {
            return;
        };
        self.despawn(petition);
        let mut grants = self.resource_mut::<Grants>();
        let written = match grants.record(&info.id).cloned() {
            Some(mut record) if answer != Answer::Ask => {
                record.set(ask.capability, &ask.target, answer);
                grants.set(&info.id, record)
            }
            _ => Ok(()),
        };
        if let Err(why) = written {
            self.write_message(LogLine::error(
                "grants",
                format!("{} cannot be written: {why}", crate::FILE),
            ));
        }
        self.write_message(Decided {
            plugin,
            capability: ask.capability,
            target: ask.target,
            allowed: answer != Answer::Deny,
        });
    }

    fn change_answer(
        &mut self,
        id: &PluginId,
        capability: Capability,
        target: &str,
        answer: Answer,
    ) {
        let mut grants = self.resource_mut::<Grants>();
        let Some(mut record) = grants
            .record(id)
            .filter(|record| !record.bundled)
            .cloned()
        else {
            return;
        };
        record.set(capability, target, answer);
        if let Err(why) = grants.set(id, record) {
            self.write_message(LogLine::error(
                "grants",
                format!("{} cannot be written: {why}", crate::FILE),
            ));
        }
    }

    fn petition(
        &self,
        petition: Entity,
    ) -> Option<(Petition, Entity)> {
        let entity = self.get_entity(petition).ok()?;
        Some((
            entity.get::<Petition>()?.clone(),
            entity.get::<ChildOf>()?.parent(),
        ))
    }

    fn show_permissions(&mut self, id: &PluginId) {
        self.resource_mut::<Permissions>().show_only(id);
        let page = PanelId::new(ENGINE, PERMISSIONS).ok();
        let found = self
            .try_query::<(Entity, &SettingsPage)>()
            .and_then(|mut pages| {
                pages
                    .iter(self)
                    .find(|(_, at)| Some(&at.id) == page.as_ref())
                    .map(|(entity, _)| entity)
            });
        if let Some(page) = found {
            self.open_settings_page(page);
        }
    }
}
