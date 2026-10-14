//! An agent's page of the Inspector: its state, and its talk, its
//! notes, its character and its past sessions as tabs. The whole
//! of its prompt is a page with no tab.

use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Res, ResMut};
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, Display, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, frame, icon,
    label, row, scroll,
};
use bevy_fynix::{ScopedExt, ViewExt, keyed, resource};
use hamlet_core::agents::{
    Account, Activity, Busy, Doing, LivesIn, Paper, Purpose, Treasury,
};
use hamlet_core::core::RefreshLists;
use hamlet_core::session::{self, Seat};
use hamlet_world::pick::{Thing, select};

use super::papers::Papers;
use crate::feel::Mood;
use crate::icons::Glyph;
use crate::looks::{
    Fills, ICON, action, bad_action, bare, filling, hairline, headed,
    note, page_head, title,
};
use crate::me::Me;
use crate::platform::{DisplayText, Wraps};
use crate::roll::rolled;
use crate::status::Bar;
use crate::theme::paint;
use crate::world_menu::{self};
use crate::{AnyView, Cx, View, chat, markdown, tabs};

/// The part of the selected agent the panel shows.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Default)]
pub enum Section {
    #[default]
    Conversation,
    Notes,
    Character,
    /// Its past sessions, each to be read or gone back to.
    History,
    /// The whole of its prompt, which no tab leads to.
    Prompt,
}

/// The sections that have a tab, with the icon and the name on it.
const TABS: [(Section, Glyph, &str); 4] = [
    (Section::Conversation, Glyph::Talk, "CHAT"),
    (Section::Notes, Glyph::Notes, "NOTES"),
    (Section::Character, Glyph::Robot, "CHARACTER"),
    (Section::History, Glyph::History, "ARCHIVE"),
];

/// The system prompt of an agent, as it was last read for the chief.
#[derive(Resource, Default, PartialEq)]
pub struct Briefing {
    pub agent: Option<Entity>,
    pub text: String,
}

/// An agent's purpose cut short, whole under the pointer.
pub(super) fn said(purpose: String) -> AnyView {
    let shown =
        crate::hover::short(&purpose, crate::hover::PURPOSE_SHOWN);
    if shown == purpose {
        return dim(purpose).boxed();
    }
    label(shown)
        .tone(Tone::Dim)
        .tooltip(move || {
            column((label(purpose.clone()).size(12.0),))
                .max_width(px(320.0))
        })
        .boxed()
}

pub(super) fn dim(text: impl Into<String>) -> impl View {
    label(text.into()).tone(Tone::Dim)
}

/// The step an agent is on, in words.
pub(crate) fn now(busy: bool, activity: Activity) -> String {
    if !busy {
        return "At rest".into();
    }
    let paper = |p: Paper| match p {
        Paper::File => "a file",
        Paper::Notes => "its notes",
        Paper::Skill => "a skill",
    };
    match activity.doing {
        Doing::Unknown => "Working".into(),
        Doing::Think => "Thinking".into(),
        Doing::Read(p) => format!("Reading {}", paper(p)),
        Doing::Write(p) => format!("Writing {}", paper(p)),
        Doing::Run => "Running a command".into(),
        Doing::Fetch => "Fetching from the network".into(),
        Doing::CustomTool => "Using a tool added by hand".into(),
        Doing::Await => "Waiting for an agent's answer".into(),
    }
}

/// The spending of `agent` from its hamlet's budget in the period, as
/// a line: "Spent today: $0.10". Nothing for an agent with no account
/// yet.
fn spent(world: &World, agent: Entity) -> String {
    let Some(account) = world.get::<Account>(agent) else {
        return String::new();
    };
    let period = world
        .get::<LivesIn>(agent)
        .and_then(|lives| world.get::<Treasury>(lives.0))
        .map(|treasury| treasury.0.period.clone())
        .filter(|period| !period.is_empty())
        .unwrap_or_else(|| "today".into());
    format!("Spent {period}: {}", account.0.cost())
}

fn header(agent: Entity, with: Option<String>) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let (name, purpose) = match &with {
            Some(name) => (
                name.to_uppercase(),
                cx.world
                    .get::<Purpose>(agent)
                    .map(|p| p.0.clone())
                    .unwrap_or_default(),
            ),
            None => (
                "THE MAYOR".to_string(),
                "Runs this hamlet.".to_string(),
            ),
        };
        let state = label(rolled(move |world| {
            now(
                world.get::<Busy>(agent).is_some(),
                world
                    .get::<Activity>(agent)
                    .copied()
                    .unwrap_or_default(),
            )
        }))
        .tone(Tone::Accent);
        let mark = icon(rolled(move |world| {
            let which = match world.get::<Busy>(agent) {
                Some(_) => Glyph::Bolt,
                None => Glyph::Moon,
            };
            which.image(ICON)
        }))
        .size(ICON)
        .tone(Tone::Accent);
        // The way to its hamlet, whose page leads on to its mayor.
        let hamlet =
            cx.world.get::<LivesIn>(agent).map(|lives| lives.0);
        let crumb = match hamlet {
            Some(hamlet) => {
                let of =
                    hamlet_core::agents::name_of(cx.world, hamlet);
                button(headed(
                    Glyph::Back,
                    label(of.to_uppercase())
                        .wrap(false)
                        .tagged(DisplayText),
                ))
                .rules(crate::looks::retrace)
                .tagged(Mood::Ink)
                .on_activate(move |world| {
                    select(world, Some(Thing::Common(hamlet)));
                })
                .boxed()
            }
            None => frame().boxed(),
        };
        // There only while there is work to stop.
        let stop =
            row((bad_action(Glyph::Stop, "STOP", move |world| {
                world_menu::stop(world, agent)
            }),))
            .display(rolled(move |world| {
                match world.get::<Busy>(agent) {
                    Some(_) => Display::Flex,
                    None => Display::None,
                }
            }));
        // Everything else that can be done to it, as a right click
        // on it in the world offers.
        let more = button(crate::looks::raised(crate::icons::glyph(
            Glyph::More,
            ICON,
        )))
        .padding(UiRect::axes(px(10.0), px(0.0)))
        .min_height(px(crate::looks::CONTROL))
        .tooltip(|| label("More to do with it").size(12.0))
        .rules(crate::looks::brass)
        .tagged(Mood::Plain)
        .on_activate(move |world| {
            let mayor = hamlet_core::agents::mayor(world);
            let thing = if mayor == Some(agent) {
                Thing::Keeper
            } else {
                Thing::Resident(agent)
            };
            world_menu::open_for(world, thing);
        });
        cx.build(
            column((
                row((crumb,)),
                row((
                    title(name),
                    row((mark, state.wrap(false)))
                        .gap(5.0)
                        .align(AlignItems::Center),
                    frame().grow(1.0),
                    stop,
                    more,
                ))
                .gap(12.0)
                .align(AlignItems::Center)
                .tagged(Wraps),
                said(purpose),
                note(resource::<Bar, _>(|b| {
                    format!("{}  This chat: {}", b.model, b.usage)
                })),
                // Its spending from the hamlet's budget, which is
                // more than this one chat.
                note(rolled(move |world| spent(world, agent))),
                crate::models::of_agent(agent),
            ))
            .gap(6.0),
        )
    })
}

fn notes() -> impl View {
    let shown =
        keyed(resource::<Papers, _>(|p| p.notes.clone()), |text| {
            if text.trim().is_empty() {
                label("No notes yet.").boxed()
            } else {
                markdown::fixed(text, Tone::Body).boxed()
            }
        })
        .within(bare());
    // Reflecting is what writes them.
    let reflect = action(Glyph::Notes, "REFLECT NOW", |w| {
        chat::send(w, "/reflect".into())
    });
    column((row((reflect,)), scroll((shown,)).fills_height()))
        .fills_height()
        .gap(8.0)
}

/// Opens the selected agent's character in the program the system
/// keeps for such a file, made empty if there is none yet.
fn edit_character(world: &mut World) {
    let file = super::papers::house(world).character();
    if file.exists() || std::fs::write(&file, "").is_ok() {
        crate::links::open_file(&file);
    }
}

fn character() -> impl View {
    let shown = keyed(
        resource::<Papers, _>(|p| p.character.clone()),
        |text| {
            if text.is_empty() {
                note("Write here how this agent behaves and speaks.")
                    .boxed()
            } else {
                markdown::fixed(text, Tone::Body).boxed()
            }
        },
    )
    .within(bare());
    let edit =
        action(Glyph::Rename, "EDIT", edit_character).tooltip(|| {
            label("Opens the file in your editor").size(12.0)
        });
    column((row((edit,)), scroll((shown,)).fills_height()))
        .fills_height()
        .gap(8.0)
}

/// Reads the prompt the agent the chat is with would be given now,
/// and turns its page to it.
pub(crate) fn show_prompt(world: &mut World) {
    let me = world.resource::<Me>();
    let (agent, session) = (me.agent, me.session);
    let text = match world.get::<Seat>(session) {
        Some(seat) => {
            session::prompt_of(world, seat).unwrap_or_else(|e| {
                format!("The prompt could not be read: {e:#}")
            })
        }
        None => String::new(),
    };
    *world.resource_mut::<Briefing>() = Briefing {
        agent: Some(agent),
        text,
    };
    *world.resource_mut::<Section>() = Section::Prompt;
}

/// Leaves the prompt of an agent the chat is no longer with.
pub(super) fn leave_prompt(
    me: Res<Me>,
    briefing: Res<Briefing>,
    mut section: ResMut<Section>,
) {
    if *section == Section::Prompt && briefing.agent != Some(me.agent)
    {
        *section = Section::Conversation;
    }
}

fn prompt() -> impl View {
    let shown =
        keyed(resource::<Briefing, _>(|b| b.text.clone()), |text| {
            // Each line as the model is given it, where Markdown
            // would run them together.
            markdown::fixed(&text.replace('\n', "  \n"), Tone::Body)
                .boxed()
        })
        .within(bare());
    column((
        page_head(
            "CHAT",
            |world| {
                *world.resource_mut::<Section>() =
                    Section::Conversation;
            },
            Glyph::Notes,
            "PROMPT",
        ),
        note("What this agent is told at the start of a turn."),
        hairline(paint(|p| p.brass_6)),
        scroll((shown,)).fills_height(),
    ))
    .fills_height()
    .gap(8.0)
}

/// The sections as tabs.
fn tabs() -> AnyView {
    tabs::strip(&TABS, |world, section| {
        *world.resource_mut::<Section>() = section;
        world.trigger(RefreshLists);
    })
    .boxed()
}

fn sections() -> impl View {
    keyed(resource::<Section, _>(|s| *s), |section| match section {
        Section::Conversation => {
            column((chat::transcript(), chat::message_bar()))
                .fills_height()
                .gap(8.0)
                .boxed()
        }
        Section::Notes => notes().boxed(),
        Section::Character => character().boxed(),
        Section::History => crate::history::screen(),
        Section::Prompt => prompt().boxed(),
    })
    .within(filling())
}

pub(super) fn living() -> impl View {
    keyed(
        resource::<Me, _>(|m| (m.session, m.agent, m.with.clone())),
        |(_, agent, with)| {
            column((header(*agent, with.clone()), tabs(), sections()))
                .fills_height()
                .gap(12.0)
                .boxed()
        },
    )
    .within(filling())
}
