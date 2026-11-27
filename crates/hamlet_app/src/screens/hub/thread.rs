//! The Hub's thread: the chat with one agent under a header that
//! says where it is, over a composer with its chips. The agent's
//! details take its place while open, and its sessions and prompt are
//! pages of their own.

pub(crate) mod prompt;

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::query::Has;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, Res};
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, UiRect, Val, percent, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, dropdown,
    frame, label, row,
};
use bevy_fynix::{
    ScopedExt, ViewExt, derived, every_frame, keyed, resource, style,
};
use hamlet_client::realm::{
    Activity, Busy, Kind, Left, LivesIn, Parent, PeerChats, Works,
    id_of, name_of, shown_of,
};
use hamlet_client::waits::Waiting;
use hamlet_protocol::client as protocol;
use hamlet_realm::pick::{Thing, select};
use hamlet_ui::styles::{
    Fills, ICON, Looks, Tipped, bare, choice, close, crumb,
    crumb_here, crumb_slash, crumbs, filling, icon_button,
    more_button, note, page_head, scroll_area, status_icon, title,
    transparent,
};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::ellipsis::{cut, one_line};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::tabs::{self, Spread};
use hamlet_ui::{AnyView, Cx, View, built, with_cx};

use super::{Showing, conversations, details, show};
use crate::convo::Convo;
use crate::history::History;
use crate::overview::tell;
use crate::roll::rolled;
use crate::screens::petition::{self, Need};
use crate::standing::Standing;
use crate::{chat, words, world_menu};

/// What the thread's page shows: its chat, or its details in the
/// chat's place.
#[derive(Resource, Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Sheet {
    #[default]
    Chat,
    Details,
}

/// The tabs between a thread's chat and its details.
const SHEETS: [(Sheet, Glyph, &str); 2] = [
    (Sheet::Chat, Glyph::Talk, "Chat"),
    (Sheet::Details, Glyph::Sliders, "Details"),
];

/// The system prompt of an agent, as it was last read for the chief.
#[derive(Resource, Default, PartialEq)]
pub struct Briefing {
    pub agent: Option<Entity>,
    pub prompt: Vec<protocol::PromptSection>,
    /// Why it could not be read.
    pub failed: Option<String>,
    /// Whether it went to the clipboard since it was read.
    pub copied: bool,
}

impl Briefing {
    /// The prompt as the model reads it, in one text.
    fn whole(&self) -> String {
        self.prompt
            .iter()
            .map(|section| section.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
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
            column((note(purpose.clone()),)).max_width(px(320.0))
        })
        .boxed()
}

pub(super) fn dim(text: impl Into<String>) -> impl View {
    label(text.into()).tone(Tone::Dim)
}

/// Where the session menu leads.
#[derive(Clone, PartialEq)]
enum Go {
    /// The chief's own chat with the agent.
    Chief,
    /// A session of the agent's, read in the sessions page.
    Past(i64),
    Peer(protocol::ConversationView),
    Worker(Entity),
}

/// The names of the session menu, what each leads to, and which of
/// them the thread shows now.
fn entries(world: &World) -> (Vec<(String, Go)>, usize) {
    let agent = world.resource::<Convo>().agent;
    let pasts = world
        .get_resource::<History>()
        .map(|history| history.pasts.as_slice())
        .unwrap_or_default();
    let live = pasts.iter().find(|past| past.current);
    let mut items = vec![(
        live.map_or("Session".to_string(), |past| {
            format!("Session {}", past.number)
        }),
        Go::Chief,
    )];
    let mut earlier = pasts
        .iter()
        .filter(|past| !past.current)
        .collect::<Vec<_>>();
    earlier.sort_by_key(|past| std::cmp::Reverse(past.number));
    for past in earlier {
        items.push((
            format!("Session {}", past.number),
            Go::Past(past.number),
        ));
    }
    let peers = world
        .get::<PeerChats>(agent)
        .map(|chats| chats.0.clone())
        .unwrap_or_default();
    for peer in peers {
        let kind = match peer.kind {
            protocol::ConversationViewKind::Letters => "Letters",
            protocol::ConversationViewKind::Questions => "Questions",
        };
        items.push((
            format!(
                "{kind} with {}",
                conversations::name_of_peer(world, peer.with)
            ),
            Go::Peer(peer),
        ));
    }
    for worker in workers(world, agent) {
        items.push((shown_of(world, worker), Go::Worker(worker)));
    }
    let now = conversations::peer_shown(world)
        .and_then(|shown| {
            items.iter().position(|(_, go)| {
                matches!(go, Go::Peer(peer) if *peer == shown)
            })
        })
        .unwrap_or(0);
    (items, now)
}

/// The living workers `agent` has started.
pub(super) fn workers(world: &World, agent: Entity) -> Vec<Entity> {
    let Some(id) = id_of(world, agent) else {
        return Vec::new();
    };
    let Some(mut query) = world.try_query_filtered::<(
        Entity,
        &Kind,
        &Parent,
    ), bevy::ecs::query::Without<Left>>() else {
        return Vec::new();
    };
    let mut found = query
        .iter(world)
        .filter(|(_, kind, parent)| {
            kind.0 == protocol::AgentKind::Worker && parent.0 == id
        })
        .map(|(worker, ..)| worker)
        .collect::<Vec<_>>();
    found.sort();
    found
}

fn go(world: &mut World, to: &Go) {
    match to {
        Go::Chief => conversations::chief(world),
        Go::Past(number) => {
            crate::history::open(world, *number);
            show(world, Showing::Sessions);
        }
        Go::Peer(peer) => conversations::open(world, peer.clone()),
        Go::Worker(worker) => crate::convo::talk_to(world, *worker),
    }
}

/// The session menu: a button saying which session this is, over a
/// list of the others. A choice shows the chat, even from the
/// details.
fn session_menu() -> impl View {
    keyed(
        derived(|world: &World| {
            let (items, now) = entries(world);
            (
                items
                    .iter()
                    .map(|(name, _)| name.clone())
                    .collect::<Vec<_>>(),
                items
                    .into_iter()
                    .map(|(_, to)| to)
                    .collect::<Vec<_>>(),
                now,
            )
        })
        .when(every_frame()),
        |(names, goes, now)| {
            let (goes, now) = (goes.clone(), *now);
            dropdown(
                names.clone(),
                now,
                Glyph::Chevron.image(ICON),
                move |world, chosen| {
                    if *world.resource::<Sheet>() != Sheet::Chat {
                        *world.resource_mut::<Sheet>() = Sheet::Chat;
                    }
                    if chosen != now
                        && let Some(to) = goes.get(chosen)
                    {
                        go(world, to);
                    }
                },
            )
            .boxed()
        },
    )
    .within(bare())
}

/// Characters of a past session's title its page shows.
const TITLE_SHOWN: usize = 36;

/// A page under the thread, as its title names it.
#[derive(Clone, PartialEq)]
enum Under {
    Prompt,
    Sessions,
    /// A past session, read from the sessions, by its title cut
    /// short.
    Session(String),
}

impl Under {
    fn name(&self) -> String {
        match self {
            Under::Prompt => "Prompt".into(),
            Under::Sessions => "Sessions".into(),
            Under::Session(title) => title.clone(),
        }
    }

    /// The head of its page, with the prompt's way to copy it at the
    /// end of that line.
    fn title(&self) -> AnyView {
        match self {
            Under::Session(_) | Under::Sessions => {
                title(self.name()).boxed()
            }
            Under::Prompt => with_cx(|cx| {
                row((
                    title("Prompt"),
                    frame().grow(1.0),
                    prompt::copy(),
                ))
                .gap(cx.theme().space.gap)
                .align(AlignItems::Center)
                .width(percent(100.0))
            })
            .boxed(),
        }
    }
}

/// What `agent` is called in the path: its nickname, else its address
/// without its hamlet, and the Hand as itself.
fn called(world: &World, agent: Entity) -> String {
    if world.get::<Kind>(agent).map(|kind| kind.0)
        == Some(protocol::AgentKind::Hand)
    {
        return "The Hand".into();
    }
    shown_of(world, agent)
}

/// The steps above `agent` in the chain of command, `Realm / hamlet /
/// mayor / lead /`, each leading to its page or its thread.
fn above(world: &World, agent: Entity) -> Vec<AnyView> {
    let hamlet = world.get::<LivesIn>(agent).map(|l| l.0);
    let mut parts = vec![
        crumb("Realm", |world| show(world, Showing::Realm)).boxed(),
        crumb_slash().boxed(),
    ];
    if let Some(hamlet) = hamlet {
        parts.push(
            crumb(name_of(world, hamlet), move |world| {
                select(world, Some(Thing::Common(hamlet)));
                show(world, Showing::Hamlet(hamlet));
            })
            .boxed(),
        );
        parts.push(crumb_slash().boxed());
    }
    // The leads above it in its hamlet, the mayor first.
    let mut leads = Vec::new();
    let mut at = agent;
    while let Some(parent) =
        world.get::<Parent>(at).and_then(|parent| {
            hamlet_client::realm::agent(world, parent.0)
        })
    {
        let same = world.get::<LivesIn>(parent).map(|l| l.0);
        if leads.contains(&parent)
            || hamlet.is_none()
            || same != hamlet
        {
            break;
        }
        leads.insert(0, parent);
        at = parent;
    }
    for lead in leads {
        parts.push(
            crumb(called(world, lead), move |world| {
                crate::convo::talk_to(world, lead);
            })
            .boxed(),
        );
        parts.push(crumb_slash().boxed());
    }
    parts
}

/// The path to the thread, then the pages `under` it.
fn path(agent: Entity, under: Vec<Under>) -> impl View {
    built(move |cx| {
        let world = &*cx.world;
        let mut parts = above(world, agent);
        let name = called(world, agent);
        parts.push(if under.is_empty() {
            crate::naming::name_crumb(agent, name)
        } else {
            crumb(name, |world| show(world, Showing::Thread)).boxed()
        });
        for (at, page) in under.iter().enumerate() {
            // A past session's title is too long for the path, so it
            // only heads its page.
            if matches!(page, Under::Session(_)) {
                continue;
            }
            parts.push(crumb_slash().boxed());
            parts.push(if at + 1 == under.len() {
                crumb_here(page.name()).boxed()
            } else {
                crumb(page.name(), crate::history::close).boxed()
            });
        }
        cx.build(crumbs(parts))
    })
}

/// The waits `agent` raised that the chief has not answered, oldest
/// first.
fn waits_of(world: &World, agent: Entity) -> Vec<Entity> {
    let address = name_of(world, agent);
    let Some(mut query) = world.try_query::<(Entity, &Waiting)>()
    else {
        return Vec::new();
    };
    let mut found = query
        .iter(world)
        .filter(|(_, waiting)| waiting.by == address)
        .map(|(entity, waiting)| (waiting.since, entity))
        .collect::<Vec<_>>();
    found.sort();
    found.into_iter().map(|(_, entity)| entity).collect()
}

/// What the agent waits on the chief for, each with its answers, over
/// its chat, so a row of NEEDS YOU lands where it can be answered.
fn waits(agent: Entity) -> impl View {
    keyed(
        derived(move |world: &World| waits_of(world, agent))
            .when(every_frame()),
        |waits| {
            let cards = waits
                .iter()
                .copied()
                .map(|wait| {
                    AnyView::new(move |cx: &mut Cx| {
                        match cx.world.get::<Waiting>(wait) {
                            Some(waiting) => {
                                let need = Need::of(wait, waiting);
                                cx.build(petition::strip(&need))
                            }
                            None => cx.build(frame()),
                        }
                    })
                })
                .collect::<Vec<_>>();
            if cards.is_empty() {
                return frame().boxed();
            }
            column((scroll_area((column(cards).gap(8.0),)),))
                .min_height(px(0.0))
                .boxed()
        },
    )
    // As tall as the cards, but never more than part of the page, so
    // the chat stays in view; the rest scrolls.
    .within(bare().shrink(0.0).max_height(percent(40.0)))
}

/// Its state, by its name, and what it works on while it works.
fn state(agent: Entity) -> impl View {
    let standing = move |world: &World| Standing::of(world, agent);
    // Its state is its icon alone, with the word under the pointer.
    let mark = keyed(rolled(standing), |standing| {
        status_icon(
            standing.glyph(),
            standing.word(),
            standing.kind(),
        )
        .boxed()
    })
    .within(row(()).gap(0.0));
    let doing = label(rolled(move |world| {
        if standing(world) != Standing::Works {
            return String::new();
        }
        world
            .get::<Activity>(agent)
            .map(|activity| activity.words.clone())
            .unwrap_or_default()
    }))
    .tone(Tone::Dim);
    row((mark, column((one_line(doing),)).min_width(px(0.0))))
        .gap(4.0)
        .align(AlignItems::Center)
        .min_width(px(0.0))
}

/// What is done to `agent` most, each a button of its own beside the
/// tabs: a new session, a hire under it, a new name. The rest is in
/// the menu of the "..." after them.
fn quick(world: &World, agent: Entity) -> Vec<AnyView> {
    let kind = world.get::<Kind>(agent).map(|kind| kind.0);
    let living = world.get::<Left>(agent).is_none();
    let mut buttons = vec![icon_button(
        Glyph::NewSession,
        "Start a new session",
        move |world| {
            crate::convo::talk_to(world, agent);
            world_menu::ask(world, world_menu::Deed::NewSession);
        },
    )];
    if living
        && kind == Some(protocol::AgentKind::Hire)
        && hamlet_client::realm::Deeds::is_allowed(world, "hire")
    {
        buttons.push(icon_button(
            Glyph::Plus,
            "Hire under it",
            move |world| world_menu::ask_hire(world, agent),
        ));
    }
    buttons
}

fn header(agent: Entity) -> impl View {
    built(move |cx| {
        // Its menu, as a right click on it opens. Gone while empty,
        // as for the Hand at rest.
        let thing = if cx.world.get::<Kind>(agent).map(|kind| kind.0)
            == Some(protocol::AgentKind::Hand)
        {
            Thing::Hand
        } else {
            Thing::Agent(agent)
        };
        let more = keyed(
            derived(move |world: &World| {
                world_menu::has_items(world, thing)
            })
            .when(every_frame()),
            move |&any| {
                if !any {
                    return frame().boxed();
                }
                more_button(
                    "Hiring, roads, merging and ending it",
                    move |world| {
                        world_menu::open_for(world, thing);
                    },
                )
                .boxed()
            },
        )
        .within(bare());
        let quick = quick(cx.world, agent);
        // Which of its chats the chat tab shows: this session, an
        // earlier one, its letters and questions, its workers. It is
        // there on both tabs, so that the tabs keep their place.
        let sessions = session_menu().tooltip(|| {
            note(
                "Switch to an earlier session, its letters or \
                 questions with other agents, or a worker it started",
            )
        });
        let address = name_of(cx.world, agent);
        let space = &cx.theme().space;
        let (gap, section) = (space.gap, space.section);
        cx.build(
            // The path, ending in the agent's name, with its state;
            // under them the tabs between the chat and the details,
            // the one shown underlined, with what the chat tab shows
            // and the rest at their end.
            column((
                row((
                    path(agent, Vec::new())
                        .tooltip(move || note(address.clone())),
                    state(agent),
                ))
                .gap(section)
                .align(AlignItems::Center)
                .min_width(px(0.0)),
                row((
                    column((tabs::strip(
                        &SHEETS,
                        Spread::Packed,
                        |world, sheet| {
                            *world.resource_mut::<Sheet>() = sheet;
                        },
                    ),))
                    .grow(1.0)
                    .min_width(px(0.0)),
                    row(quick)
                        .gap(gap / 2.0)
                        .align(AlignItems::Center),
                    sessions,
                    more,
                ))
                .gap(gap)
                .align(AlignItems::Center),
            ))
            .gap(section),
        )
    })
}

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, light_chips);
}

/// On the chip of the model of the agent it holds.
#[derive(Component, Clone, Copy)]
struct ModelChip(Entity);

/// On the chip of the model while its popup is open, which lights it.
#[derive(Component)]
struct ChipOpen;

/// The chip of the model, its effort and its tier, which opens the
/// popup they are chosen in and is lit while it is open. Built once,
/// and lit in place, so it does not come in again as it opens.
fn model_chip(agent: Entity) -> impl View {
    button(
        row((world_menu::chip(agent), glyph(Glyph::Chevron, 12.0)))
            .gap(6.0)
            .align(AlignItems::Center),
    )
    .padding(UiRect::axes(px(8.0), px(0.0)))
    .rules(chip_look)
    .tagged(Variant::Text)
    .tagged(world_menu::Hook::Chip)
    .tagged(ModelChip(agent))
    .on_activate(move |world| {
        world_menu::ask_model(world, agent);
    })
}

/// Lights the chip of the model whose popup is open.
fn light_chips(
    open: Option<Res<world_menu::Open>>,
    chips: Query<(Entity, &ModelChip, Has<ChipOpen>)>,
    mut commands: Commands,
) {
    for (chip, of, lit) in &chips {
        let wanted = open.as_deref().is_some_and(|open| {
            *open == world_menu::Open::Model(of.0)
        });
        if wanted && !lit {
            commands.entity(chip).try_insert(ChipOpen);
        } else if !wanted && lit {
            commands.entity(chip).try_remove::<ChipOpen>();
        }
    }
}

/// The look of a chip of the composer: a choice on a plate, with no
/// edge at rest inside the card's own, and picked while [`ChipOpen`].
fn chip_look(cx: &mut Cx) {
    choice(false)(cx);
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(t.palette.button)
                .border_color(transparent(t.palette.border))
        })
        .when::<ChipOpen>(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.selected)
                    .border_color(t.palette.gold_4)
            })
            .tone(Tone::Body)
        })
        .apply(cx);
}

/// The hamlet `agent` lives in and how its work is overseen.
fn mode_of(
    world: &World,
    agent: Entity,
) -> Option<(Entity, hamlet_client::realm::Mode)> {
    let lives = world.get::<LivesIn>(agent)?.0;
    Some((lives, world.get::<Works>(lives)?.mode))
}

/// The chip of the hamlet's mode, which a press turns over. Built
/// once for its hamlet; its words follow the mode in place.
fn mode_chip(agent: Entity) -> impl View {
    keyed(
        derived(move |world: &World| {
            mode_of(world, agent).map(|at| at.0)
        })
        .when(every_frame()),
        move |hamlet| {
            let Some(hamlet) = *hamlet else {
                return frame().boxed();
            };
            let words = derived(move |world: &World| {
                match mode_of(world, agent).map(|at| at.1) {
                    Some(hamlet_client::realm::Mode::Auto) => {
                        words::NO_ASKING
                    }
                    _ => words::ASK_FIRST,
                }
                .to_string()
            })
            .when(every_frame());
            button(
                row((
                    glyph(Glyph::Shield, 12.0),
                    label(words).wrap(false),
                ))
                .gap(6.0)
                .align(AlignItems::Center),
            )
            .padding(UiRect::axes(px(8.0), px(0.0)))
            .rules(chip_look)
            .tagged(Variant::Text)
            .tooltip(|| {
                note(format!("{} Press to switch.", words::APPROVALS))
            })
            .on_activate(move |world| {
                let to = match mode_of(world, agent).map(|at| at.1) {
                    Some(hamlet_client::realm::Mode::Auto) => {
                        "manual"
                    }
                    _ => "auto",
                };
                tell(world, hamlet, format!("/mode {to}"));
            })
            .boxed()
        },
    )
    .within(bare())
}

/// The side of the round send and stop buttons.
const ROUND: f32 = 30.0;

/// On the round button that stops the agent.
#[derive(Component)]
struct Stopping;

/// The round button at the end of the toolbar: Send, or Stop while
/// the agent works.
fn send_or_stop(agent: Entity) -> impl View {
    keyed(
        derived(move |world: &World| {
            world.get::<Busy>(agent).is_some()
        })
        .when(every_frame()),
        move |busy| {
            let round = |face: AnyView| {
                button(face)
                    .width(px(ROUND))
                    .height(px(ROUND))
                    .padding(UiRect::all(px(0.0)))
                    .radius(ROUND / 2.0)
            };
            if *busy {
                let square = with_cx(|cx| {
                    frame()
                        .width(px(10.0))
                        .height(px(10.0))
                        .radius(2.0)
                        .fill(cx.theme().palette.red)
                });
                round(square.boxed())
                    .variant(Variant::Danger)
                    .tagged(Stopping)
                    .tip("Stop")
                    .on_activate(move |world| {
                        world_menu::stop(world, agent);
                    })
                    .boxed()
            } else {
                round(glyph(Glyph::Send, 16.0).boxed())
                    .variant(Variant::Primary)
                    .tip("Send")
                    .on_activate(chat::send_typed)
                    .boxed()
            }
        },
    )
    .within(bare())
}

/// The composer's toolbar, under its field: the model and effort and
/// the hamlet's mode, then Send or Stop.
fn chips(agent: Entity) -> impl View {
    // With no model set up, the model's chip is the way to set one
    // up.
    let model = keyed(
        derived(chat::without_model).when(every_frame()),
        move |&none| {
            if !none {
                return model_chip(agent).boxed();
            }
            button(
                row((
                    glyph(Glyph::Chip, 12.0).tone(Tone::Critical),
                    label("Set up a model").wrap(false),
                ))
                .gap(6.0)
                .align(AlignItems::Center),
            )
            .padding(UiRect::axes(px(8.0), px(0.0)))
            .rules(chip_look)
            .tagged(Variant::Text)
            .tooltip(|| {
                note("No model is set up yet, so nothing can answer")
            })
            .on_activate(crate::welcome::again)
            .boxed()
        },
    )
    .within(bare());
    row((
        model,
        mode_chip(agent),
        frame().grow(1.0),
        send_or_stop(agent),
    ))
    .gap(2.0)
    .align(AlignItems::Center)
}

/// The transcript over its composer, or over a note when the chat
/// is only watched.
fn talk(agent: Entity) -> AnyView {
    keyed(resource::<Convo, _>(|m| m.session), move |_| {
        with_cx(move |cx| {
            let space = &cx.theme().space;
            let (gap, widest) = (space.gap, px(space.thread_width));
            // Another's chat is only watched, and leads back to the
            // chief's by the path over it.
            let under = if conversations::is_read_only(cx.world) {
                close((note("Read only"),)).boxed()
            } else {
                chat::message_bar(chips(agent)).boxed()
            };
            // In a column of its own in the middle of a wide panel.
            column((chat::transcript(), under))
                .fills_height()
                .gap(gap)
                .width(percent(100.0))
                .max_width(widest)
                .margin(UiRect::horizontal(Val::Auto))
                .boxed()
        })
        .boxed()
    })
    .within(filling())
    .boxed()
}

/// The thread of the agent the chat is with.
pub(super) fn living() -> impl View {
    keyed(
        resource::<Convo, _>(|m| (m.session, m.agent)),
        |(_, agent)| {
            let agent = *agent;
            // The details take the chat's place while they are open.
            let body = keyed(
                resource::<Sheet, _>(|sheet| {
                    *sheet == Sheet::Details
                }),
                move |open| {
                    if *open {
                        details::sheet(agent).boxed()
                    } else {
                        talk(agent)
                    }
                },
            )
            .within(filling());
            with_cx(move |cx| {
                let gap = cx.theme().space.gap;
                column((header(agent), waits(agent), body))
                    .fills_height()
                    .gap(gap)
            })
            .boxed()
        },
    )
    .within(filling())
}

/// A page about the agent the chat is with, under the path to it and
/// the pages `under` names, and the title of the last of them.
fn aside(
    under: fn(&World) -> Vec<Under>,
    body: impl View + 'static,
) -> impl View {
    let head = keyed(
        derived(move |world: &World| {
            (world.resource::<Convo>().agent, under(world))
        })
        .when(every_frame()),
        |(agent, under)| {
            let name = under
                .last()
                .map_or_else(|| title("").boxed(), Under::title);
            page_head(path(*agent, under.clone()), name).boxed()
        },
    )
    .within(bare());
    with_cx(move |cx| {
        column((head, body))
            .fills_height()
            .gap(cx.theme().space.section)
    })
}

/// The past sessions of the agent the chat is with.
pub(super) fn sessions() -> impl View {
    aside(
        |world| {
            let history = world.resource::<History>();
            match (&history.reading, history.read()) {
                (Some(_), Some(past)) => vec![
                    Under::Sessions,
                    Under::Session(cut(&past.title, TITLE_SHOWN)),
                ],
                (Some(reading), None) => vec![
                    Under::Sessions,
                    Under::Session(format!(
                        "Session {}",
                        reading.number
                    )),
                ],
                (None, _) => vec![Under::Sessions],
            }
        },
        crate::history::screen(),
    )
}

#[cfg(test)]
mod tests;
