//! An agent's page: its state, its model and spend, and its notes,
//! its character and its prompt. It is the agent of the open
//! conversation, which selecting an agent opens.

use agents_ui::{
    AgentsWorldExt, Focused, Heard, breadcrumb, either, markdown,
    moved, short,
};
use hamlet_engine_sdk::api::agents::{
    AgentsEvent, AgentsReply, AgentsRequest, CatalogueModel, Papers,
    Recipient, Roster, Selected,
};
use hamlet_engine_sdk::ui::{
    AnyView, ButtonProps, ButtonStyle, FrameProps, Size, Space, Tone,
    UiExt, ViewExt, World, button, column, derived, divider,
    dropdown, icon, icon_button, keyed, label, resource, row, scroll,
};

use crate::{HAMLETS, PURPOSE, agent_named, archive, more, select};

/// The part of the agent the page shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum Tab {
    #[default]
    Notes,
    Character,
    /// Its past sessions, each to be read or gone back to.
    Archive,
    /// The whole of its prompt, which no tab leads to.
    Prompt,
}

/// The tabs, with the words on each.
const TABS: [(Tab, &str); 3] = [
    (Tab::Notes, "NOTES"),
    (Tab::Character, "CHARACTER"),
    (Tab::Archive, "ARCHIVE"),
];

/// The tab shown, and the one the prompt goes back to.
#[derive(Default)]
pub(crate) struct Shown {
    pub tab: Tab,
    back: Tab,
    /// The agent the papers were read for.
    agent: Option<String>,
}

/// The prompt of the agent, as it was last read.
#[derive(Default)]
struct Briefing(String);

/// The models an agent may be given: the home's default and those it
/// allows.
#[derive(Default)]
pub(crate) struct Models {
    pub(crate) default: String,
    pub(crate) allowed: Vec<CatalogueModel>,
}

pub(crate) fn prepare(world: &mut World) {
    world.init_resource::<Shown>();
    world.init_resource::<Papers>();
    world.init_resource::<Briefing>();
    world.init_resource::<Models>();
}

/// Reads the papers again for the agent of the open conversation,
/// and leaves the prompt of one it is no longer with.
pub(crate) fn follow(world: &mut World) {
    let agent = world.resource::<Focused>().agent();
    let shown = world.resource::<Shown>();
    let moved = shown.agent != agent;
    if moved && shown.tab == Tab::Prompt {
        let back = shown.back;
        world.resource_mut::<Shown>().tab = back;
    }
    if moved {
        leave(world, agent.clone());
    }
    if agent.is_some() {
        world.ask(AgentsRequest::Papers);
        if world.resource::<Shown>().tab == Tab::Archive {
            archive::refresh(world);
        }
    }
}

/// Forgets what was read for the agent shown before `agent`.
fn leave(world: &mut World, agent: Option<String>) {
    archive::forget(world);
    world.insert(Papers::default());
    world.insert(Briefing::default());
    world.resource_mut::<Shown>().agent = agent;
}

/// Shows the prompt of the agent `name`, talking to it first.
pub(crate) fn open_prompt(world: &mut World, name: &str) {
    if world.resource::<Focused>().agent().as_deref() != Some(name) {
        select(world, Selected::Agent(name.to_string()));
        // Its papers and prompt are asked after the selection, so
        // they are its own, and the prompt stays once it is selected.
        leave(world, Some(name.to_string()));
    }
    show(world, Tab::Prompt);
}

/// Takes in the papers, the prompt and the models. Those of an agent
/// were asked for the one shown, as each move asks them again.
pub(crate) fn heard(world: &mut World, heard: &Heard) {
    let shown = world.resource::<Shown>().agent.is_some();
    match heard {
        Heard::Reply(_, AgentsReply::Papers(papers)) if shown => {
            world.keep(papers.clone());
        }
        Heard::Reply(_, AgentsReply::Prompt(text)) if shown => {
            world.insert(Briefing(text.clone()));
        }
        Heard::Failed(Some(AgentsRequest::Prompt), why) => {
            world.insert(Briefing(format!(
                "The prompt could not be read: {why}"
            )));
        }
        Heard::Reply(_, AgentsReply::Settings(settings))
        | Heard::Event(AgentsEvent::Settings(settings)) => {
            world
                .resource_mut::<Models>()
                .default
                .clone_from(&settings.model);
        }
        Heard::Reply(_, AgentsReply::Catalogue(models)) => {
            world.resource_mut::<Models>().allowed = models
                .iter()
                .filter(|model| model.allowed)
                .cloned()
                .collect();
        }
        Heard::Event(AgentsEvent::Catalogue) => {
            world.ask(AgentsRequest::Catalogue);
        }
        _ => {}
    }
}

/// Shows `tab`, reading again what it shows.
pub(crate) fn show(world: &mut World, tab: Tab) {
    let shown = world.resource_mut::<Shown>();
    if tab == Tab::Prompt && shown.tab != Tab::Prompt {
        shown.back = shown.tab;
    }
    shown.tab = tab;
    world.ask(AgentsRequest::Papers);
    match tab {
        Tab::Prompt => world.ask(AgentsRequest::Prompt),
        Tab::Archive => archive::refresh(world),
        Tab::Notes | Tab::Character => {}
    }
}

/// What the head of the page shows.
#[derive(Clone, PartialEq, Debug, Default)]
struct Head {
    name: String,
    /// The conversation is with a hamlet's mayor.
    mayor: bool,
    /// The agent's hamlet: its name on disk and its title.
    hamlet: Option<(String, String)>,
    state: String,
    busy: bool,
    purpose: String,
    model: String,
    usage: String,
    spent: String,
}

fn head_of(world: &World) -> Head {
    let Some(focus) = world.resource::<Focused>().0.as_ref() else {
        return Head::default();
    };
    let name = focus.open.agent.clone().unwrap_or_default();
    let agent = agent_named(world, &name);
    let roster = world.resource::<Roster>();
    let hamlet = agent.and_then(|agent| {
        roster
            .hamlets
            .iter()
            .find(|hamlet| hamlet.id == agent.hamlet)
            .map(|hamlet| (hamlet.id.clone(), hamlet.title.clone()))
    });
    Head {
        mayor: matches!(focus.with, Recipient::Mayor(_)),
        hamlet,
        state: agent
            .map(|agent| agent.state.clone())
            .unwrap_or_default(),
        busy: agent.is_some_and(|agent| agent.busy),
        purpose: agent
            .map(|agent| agent.purpose.clone())
            .unwrap_or_default(),
        model: agent
            .map(|agent| agent.model.clone())
            .unwrap_or_default(),
        usage: focus.open.usage.clone(),
        spent: agent
            .map(|agent| agent.spent.clone())
            .unwrap_or_default(),
        name,
    }
}

/// The names a dropdown of models offers: the home's default, every
/// model the home allows, and `now` when it is none of those.
pub(crate) fn offered(models: &Models, now: &str) -> Vec<String> {
    let mut names = vec![models.default.clone()];
    for model in &models.allowed {
        if !names.contains(&model.name) {
            names.push(model.name.clone());
        }
    }
    if !now.is_empty() && !names.iter().any(|name| name == now) {
        names.push(now.to_string());
    }
    names.retain(|name| !name.is_empty());
    names
}

/// The model the agent `name` runs, for its chief to change.
fn model_choice(name: String, now: String) -> AnyView {
    keyed(
        resource::<Models, _>(move |models| {
            (offered(models, &now), now.clone())
        }),
        move |(names, now)| {
            let at = names
                .iter()
                .position(|one| one == now)
                .unwrap_or(names.len());
            let (agent, names) = (name.clone(), names.clone());
            column((
                label("Model").size(Size::Small).tone(Tone::Dim),
                dropdown(
                    names.clone(),
                    u32::try_from(at).unwrap_or(u32::MAX),
                )
                .on_pick(move |world, picked| {
                    if let Some(model) = names.get(picked as usize) {
                        world.ask(AgentsRequest::RunModel {
                            agent: agent.clone(),
                            model: model.clone(),
                        });
                    }
                }),
            ))
            .space(Space::Half)
            .boxed()
        },
    )
    .boxed()
}

/// Where the page of the agent `title` is: under every hamlet and its
/// own, `hamlet`, and over its prompt while that is shown.
fn crumb(hamlet: Option<(String, String)>, title: String) -> AnyView {
    keyed(
        resource::<Shown, _>(|shown| shown.tab == Tab::Prompt),
        move |prompt| {
            let mut trail = vec![HAMLETS.to_string()];
            trail.extend(hamlet.as_ref().map(|(_, of)| of.clone()));
            let current = if *prompt {
                trail.push(title.clone());
                "PROMPT".to_string()
            } else {
                title.clone()
            };
            let id = hamlet.as_ref().map(|(id, _)| id.clone());
            breadcrumb(trail, current, move |world, at| {
                match (at, &id) {
                    (0, _) => select(world, Selected::Nothing),
                    (1, Some(id)) => {
                        select(world, Selected::Hamlet(id.clone()))
                    }
                    _ => {
                        let back = world.resource::<Shown>().back;
                        show(world, back);
                    }
                }
            })
        },
    )
    .boxed()
}

fn header(head: &Head) -> AnyView {
    let mut parts = Vec::new();
    let title = if head.name.is_empty() && head.mayor {
        "THE MAYOR".to_string()
    } else {
        head.name.clone()
    };
    let purpose = if head.mayor {
        "Runs this hamlet.".to_string()
    } else {
        short(&head.purpose, PURPOSE)
    };
    let mut top = vec![
        crumb(head.hamlet.clone(), title),
        icon(if head.busy { "bolt" } else { "moon" })
            .tone(Tone::Accent)
            .boxed(),
        label(head.state.clone()).tone(Tone::Accent).boxed(),
    ];
    // There only while there is work to stop.
    if head.busy {
        let name = head.name.clone();
        top.push(
            icon_button("stop", "STOP")
                .style(ButtonStyle::Critical)
                .on_click(move |world| {
                    world.ask(AgentsRequest::Veto(Some(name.clone())))
                })
                .tip("Stops what it is doing now")
                .boxed(),
        );
    }
    top.push(more::more());
    parts.push(row(top).space(Space::Gap).boxed());
    let whole = if head.mayor {
        purpose.clone()
    } else {
        head.purpose.clone()
    };
    parts.push(label(purpose).tone(Tone::Dim).tip(whole).boxed());
    parts.push(
        label(format!("{}  This chat: {}", head.model, head.usage))
            .size(Size::Small)
            .tone(Tone::Dim)
            .boxed(),
    );
    if !head.spent.is_empty() {
        parts.push(
            label(head.spent.clone())
                .size(Size::Small)
                .tone(Tone::Dim)
                .boxed(),
        );
    }
    if !head.name.is_empty() {
        parts.push(model_choice(
            head.name.clone(),
            head.model.clone(),
        ));
    }
    column(parts).space(Space::Half).boxed()
}

fn notes() -> AnyView {
    column((
        row((icon_button("page", "REFLECT NOW")
            .on_click(|world| world.tell("/reflect")),)),
        scroll((keyed(
            resource::<Papers, _>(|papers| papers.notes.clone()),
            |notes| {
                if notes.trim().is_empty() {
                    label("No notes yet.").boxed()
                } else {
                    markdown(notes, Tone::Body)
                }
            },
        ),)),
    ))
    .space(Space::Gap)
    .boxed()
}

fn character() -> AnyView {
    column((
        row((icon_button("pencil", "EDIT")
            .on_click(|world| world.ask(AgentsRequest::EditCharacter))
            .tip("Opens the file in your editor"),)),
        scroll((keyed(
            resource::<Papers, _>(|papers| papers.character.clone()),
            |character| {
                if character.is_empty() {
                    label(
                        "Write here how this agent behaves and \
                         speaks.",
                    )
                    .tone(Tone::Dim)
                    .boxed()
                } else {
                    markdown(character, Tone::Body)
                }
            },
        ),)),
    ))
    .space(Space::Gap)
    .boxed()
}

fn prompt() -> AnyView {
    column((
        label("What this agent is told at the start of a turn.")
            .size(Size::Small)
            .tone(Tone::Dim),
        divider(),
        scroll((label(resource::<Briefing, _>(|briefing| {
            briefing.0.clone()
        })),)),
    ))
    .space(Space::Gap)
    .boxed()
}

/// The tabs, and PROMPT beside them.
fn tabs() -> AnyView {
    keyed(resource::<Shown, _>(|shown| shown.tab), |now| {
        let mut buttons = TABS
            .iter()
            .map(|(tab, words)| {
                let tab = *tab;
                button(*words)
                    .style(if tab == *now {
                        ButtonStyle::Primary
                    } else {
                        ButtonStyle::Plain
                    })
                    .on_click(move |world| show(world, tab))
                    .boxed()
            })
            .collect::<Vec<_>>();
        buttons.push(
            button("PROMPT")
                .style(ButtonStyle::Quiet)
                .on_click(|world| show(world, Tab::Prompt))
                .boxed(),
        );
        row(buttons).space(Space::Half).boxed()
    })
    .boxed()
}

fn section() -> AnyView {
    keyed(resource::<Shown, _>(|shown| shown.tab), |tab| match tab {
        Tab::Notes => notes(),
        Tab::Character => character(),
        Tab::Archive => archive::screen(),
        Tab::Prompt => prompt(),
    })
    .boxed()
}

/// The page of the agent of the open conversation.
pub(crate) fn living() -> AnyView {
    column((
        keyed(
            derived(head_of)
                .when(either(moved::<Focused>(), moved::<Roster>())),
            header,
        ),
        tabs(),
        section(),
    ))
    .space(Space::Gap)
    .boxed()
}
