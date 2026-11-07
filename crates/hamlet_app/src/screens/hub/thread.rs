//! The Hub's thread: the chat with one agent under a header that
//! says where it is, over a composer with its chips. The agent's
//! details take its place while open, and its sessions and prompt are
//! pages of their own.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Res, ResMut};
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, Display, UiRect, percent, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, dropdown,
    frame, icon, label, row,
};
use bevy_fynix::{
    ScopedExt, ViewExt, derived, every_frame, keyed, resource,
};
use hamlet_client::realm::{
    Activity, Busy, Kind, Left, LivesIn, Parent, PeerChats, Works,
    id_of, name_of, shown_of,
};
use hamlet_client::waits::Waiting;
use hamlet_protocol::client as protocol;
use hamlet_realm::pick::{Thing, select};
use hamlet_ui::styles::{
    Fills, ICON, bare, choice, close, crumb, crumb_here, crumb_slash,
    crumbs, danger_button, filling, hairline, note, primary_button,
    scroll_area,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::ellipsis::one_line;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::platform::Wraps;
use hamlet_ui::widgets::tabs;
use hamlet_ui::{AnyView, View, built, with_cx};

use super::{Showing, conversations, details, show};
use crate::convo::Convo;
use crate::history::History;
use crate::overview::tell;
use crate::roll::rolled;
use crate::screens::petition::{self, Need};
use crate::standing::Standing;
use crate::{chat, world_menu};

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

/// A page under the thread, as the path to it names it.
#[derive(Clone, Copy, PartialEq)]
enum Under {
    Prompt,
    Sessions,
    /// The past session of that number, read from the sessions.
    Session(i64),
}

impl Under {
    fn name(self) -> String {
        match self {
            Under::Prompt => "Prompt".into(),
            Under::Sessions => "Sessions".into(),
            Under::Session(number) => format!("Session {number}"),
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

/// The chain of command down to the thread: `Realm / hamlet / mayor /
/// lead / agent`, then the pages `under` it. Each step above the last
/// leads to its page or its thread.
fn path(agent: Entity, under: Vec<Under>) -> impl View {
    built(move |cx| {
        let world = &*cx.world;
        let hamlet = world.get::<LivesIn>(agent).map(|l| l.0);
        let mut parts = vec![
            crumb("Realm", |world| show(world, Showing::Realm))
                .boxed(),
        ];
        if let Some(hamlet) = hamlet {
            parts.push(crumb_slash().boxed());
            parts.push(
                crumb(name_of(world, hamlet), move |world| {
                    select(world, Some(Thing::Common(hamlet)));
                    show(world, Showing::Hamlet(hamlet));
                })
                .boxed(),
            );
        }
        // The leads above it in its hamlet, the mayor first.
        let mut above = Vec::new();
        let mut at = agent;
        while let Some(parent) =
            world.get::<Parent>(at).and_then(|parent| {
                hamlet_client::realm::agent(world, parent.0)
            })
        {
            let same = world.get::<LivesIn>(parent).map(|l| l.0);
            if above.contains(&parent)
                || hamlet.is_none()
                || same != hamlet
            {
                break;
            }
            above.insert(0, parent);
            at = parent;
        }
        for lead in above {
            parts.push(crumb_slash().boxed());
            parts.push(
                crumb(called(world, lead), move |world| {
                    crate::convo::talk_to(world, lead);
                })
                .boxed(),
            );
        }
        parts.push(crumb_slash().boxed());
        let address = name_of(world, agent);
        let name = called(world, agent);
        parts.push(if under.is_empty() {
            crumb_here(name)
                .tooltip(move || note(address.clone()))
                .boxed()
        } else {
            crumb(name, |world| show(world, Showing::Thread)).boxed()
        });
        for (at, page) in under.iter().enumerate() {
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
                    AnyView::new(move |cx: &mut hamlet_ui::Cx| {
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
    let mark = icon(rolled(move |world| {
        standing(world).glyph().image(ICON)
    }))
    .size(ICON)
    .tone(rolled(move |world| standing(world).tone()));
    let word = label(rolled(move |world| {
        standing(world).word().to_string()
    }))
    .tone(rolled(move |world| standing(world).tone()))
    .wrap(false);
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
    row((mark, word, column((one_line(doing),)).min_width(px(0.0))))
        .gap(5.0)
        .align(AlignItems::Center)
        .min_width(px(0.0))
}

fn header(agent: Entity) -> impl View {
    built(move |cx| {
        // Everything else that can be done to it, as a right click on
        // it in the world offers: a new session, gifts, its prompt,
        // stopping, roads, merging, ending.
        let more = button(glyph(Glyph::More, ICON))
            .padding(UiRect::axes(px(8.0), px(4.0)))
            .rules(hamlet_ui::styles::button)
            .tagged(Variant::Default)
            .tooltip(|| {
                note(
                    "A new session, its prompt, gifts, roads, \
                     ending it",
                )
            })
            .on_activate(move |world| {
                let hall = hamlet_client::realm::first_mayor(world);
                let thing = if hall == Some(agent) {
                    Thing::Keeper
                } else {
                    Thing::Agent(agent)
                };
                world_menu::open_for(world, thing);
            });
        // Which of its chats the chat tab shows: this session, an
        // earlier one, its letters and questions, its workers. It is
        // there on both tabs, so that the tabs keep their place.
        let sessions = session_menu().tooltip(|| {
            note(
                "Switch to an earlier session, its letters or \
                 questions with other agents, or a worker it started",
            )
        });
        let gap = cx.theme().space.gap;
        cx.build(
            // The path with its state; under it the tabs between the
            // chat and the details, the one shown underlined, with
            // what the chat tab shows and the rest at their end.
            column((
                row((path(agent, Vec::new()), state(agent)))
                    .gap(gap)
                    .align(AlignItems::Center)
                    .tagged(Wraps),
                row((
                    column((tabs::strip(&SHEETS, |world, sheet| {
                        *world.resource_mut::<Sheet>() = sheet;
                    }),))
                    .grow(1.0)
                    .min_width(px(0.0)),
                    sessions,
                    more,
                ))
                .gap(gap)
                .align(AlignItems::Center),
            ))
            .gap(gap),
        )
    })
}

/// The chip of the model, its effort and its tier, which opens the
/// popup they are chosen in.
fn model_chip(agent: Entity) -> impl View {
    button(
        row((world_menu::chip(agent), glyph(Glyph::Chevron, 12.0)))
            .gap(6.0)
            .align(AlignItems::Center),
    )
    .padding(UiRect::axes(px(8.0), px(0.0)))
    .rules(choice(false))
    .tagged(Variant::Text)
    .on_activate(move |world| world_menu::ask_model(world, agent))
}

/// The chip of the hamlet's mode, which a press turns over.
fn mode_chip(agent: Entity) -> impl View {
    keyed(
        derived(move |world: &World| {
            let lives = world.get::<LivesIn>(agent)?.0;
            Some((lives, world.get::<Works>(lives)?.mode))
        })
        .when(every_frame()),
        move |at| {
            let Some((hamlet, mode)) = *at else {
                return frame().boxed();
            };
            let words = match mode {
                hamlet_client::realm::Mode::Manual => "Manual",
                hamlet_client::realm::Mode::Auto => "Auto",
            };
            button(label(words).wrap(false))
                .padding(UiRect::axes(px(8.0), px(0.0)))
                .rules(choice(false))
                .tagged(Variant::Text)
                .tooltip(|| {
                    note(
                        "How this hamlet's work is overseen. Press \
                         to turn it over.",
                    )
                })
                .on_activate(move |world| {
                    let to = match mode {
                        hamlet_client::realm::Mode::Manual => "auto",
                        hamlet_client::realm::Mode::Auto => "manual",
                    };
                    tell(world, hamlet, format!("/mode {to}"));
                })
                .boxed()
        },
    )
    .within(bare())
}

/// The `+` that offers the skills an agent could be given.
fn gifts(agent: Entity) -> impl View {
    row((button(glyph(Glyph::Plus, ICON))
        .padding(UiRect::axes(px(8.0), px(0.0)))
        .tooltip(|| note("Give it a skill"))
        .rules(choice(false))
        .tagged(Variant::Text)
        .on_activate(move |world| {
            world_menu::open_skills(world, agent);
        }),))
    .display(rolled(move |world| {
        if hamlet_client::gifts::offers(world, agent).is_empty() {
            Display::None
        } else {
            Display::Flex
        }
    }))
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
                    .rules(danger_button)
                    .tagged(Variant::Danger)
                    .tagged(Stopping)
                    .tooltip(|| note("Stop"))
                    .on_activate(move |world| {
                        world_menu::stop(world, agent);
                    })
                    .boxed()
            } else {
                round(glyph(Glyph::Send, 16.0).boxed())
                    .rules(primary_button)
                    .tagged(Variant::Primary)
                    .tooltip(|| note("Send"))
                    .on_activate(chat::send_typed)
                    .boxed()
            }
        },
    )
    .within(bare())
}

/// The composer's toolbar, under its field: the gifts, the model and
/// effort and the hamlet's mode, then Send or Stop.
fn chips(agent: Entity) -> impl View {
    row((
        gifts(agent),
        model_chip(agent),
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
            let gap = cx.theme().space.gap;
            // Another's chat is only watched, and leads back to the
            // chief's by the path over it.
            if conversations::is_read_only(cx.world) {
                column((
                    chat::transcript(),
                    close((note("Read only"),)),
                ))
                .fills_height()
                .gap(gap)
                .boxed()
            } else {
                column((
                    chat::transcript(),
                    chat::message_bar(chips(agent)),
                ))
                .fills_height()
                .gap(gap)
                .boxed()
            }
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

/// Reads the prompt the agent the chat is with would be given now,
/// and turns the page to it.
pub(crate) fn show_prompt(world: &mut World) {
    let convo = world.resource::<Convo>();
    let agent = convo.agent;
    *world.resource_mut::<Briefing>() = Briefing {
        agent: Some(agent),
        text: String::new(),
    };
    super::papers::prompt(world);
    show(world, Showing::Prompt);
}

/// Leaves the prompt of an agent the chat is no longer with.
pub(super) fn leave_prompt(
    convo: Res<Convo>,
    briefing: Res<Briefing>,
    mut showing: ResMut<Showing>,
) {
    if *showing == Showing::Prompt
        && briefing.agent != Some(convo.agent)
    {
        *showing = Showing::Thread;
    }
}

/// A page about the agent the chat is with, under the path to it and
/// the pages `under` names.
fn aside(
    under: fn(&World) -> Vec<Under>,
    body: impl View + 'static,
) -> impl View {
    let head = keyed(
        derived(move |world: &World| {
            (world.resource::<Convo>().agent, under(world))
        })
        .when(every_frame()),
        |(agent, under)| path(*agent, under.clone()).boxed(),
    )
    .within(bare());
    with_cx(move |cx| {
        column((head, body))
            .fills_height()
            .gap(cx.theme().space.section)
    })
}

/// The whole of the agent's prompt.
pub(super) fn prompt_page() -> impl View {
    let shown =
        keyed(resource::<Briefing, _>(|b| b.text.clone()), |text| {
            // Each line as the model is given it, where Markdown
            // would run them together.
            hamlet_ui::markdown::fixed(
                &text.replace('\n', "  \n"),
                Tone::Body,
            )
            .boxed()
        })
        .within(bare());
    with_cx(move |cx| {
        let gold = cx.theme().palette.gold_6;
        let gap = cx.theme().space.section;
        aside(
            |_| vec![Under::Prompt],
            column((
                note(
                    "What this agent is told at the start of a turn.",
                ),
                hairline(gold),
                scroll_area((shown,)),
            ))
            .fills_height()
            .gap(gap),
        )
    })
}

/// The past sessions of the agent the chat is with.
pub(super) fn sessions() -> impl View {
    aside(
        |world| match &world.resource::<History>().reading {
            Some(reading) => {
                vec![Under::Sessions, Under::Session(reading.number)]
            }
            None => vec![Under::Sessions],
        },
        crate::history::screen(),
    )
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy::ecs::query::{With, Without};
    use bevy_fynix::Leaving;
    use hamlet_client::outbox::Outbox;

    use super::*;
    use crate::testing::{panel_app, press, words};

    fn hub_app() -> App {
        let mut app = panel_app();
        app.add_plugins((chat::plugin, crate::hub::plugin));
        app.init_resource::<crate::status::Bar>();
        app
    }

    #[test]
    fn the_header_shows_received_work_and_stops_only_a_busy_agent() {
        let mut app = hub_app();
        let mut state = hamlet_testing::realm::state();
        state.agents[0].busy = true;
        state.agents[0].activity.doing_words =
            "Reading the received orchard ledger".into();
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.update();
        bevy_fynix::mount::<hamlet_ui::theme::HamletTheme>(
            app.world_mut(),
            living(),
        );
        app.update();
        assert!(words(&mut app).iter().any(
            |text| text == "Reading the received orchard ledger"
        ));
        let stop = |app: &mut App| {
            let world = app.world_mut();
            // Past one going out as Send comes in.
            world
                .query_filtered::<Entity, (With<Stopping>, Without<Leaving>)>()
                .iter(world)
                .next()
        };
        let button = stop(&mut app).expect("Stop while she works");
        app.world_mut()
            .trigger(bevy::ui_widgets::Activate { entity: button });
        app.world_mut().flush();
        let stopped =
            app.world().resource::<Outbox>().iter().any(|message| {
                let protocol::ClientMessage::Stop(stop) = message
                else {
                    return false;
                };
                stop.agent == Some(protocol::AgentId(1))
            });
        assert!(stopped);
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(
                hamlet_testing::realm::state(),
            ),
        );
        app.update();
        assert!(
            stop(&mut app).is_none(),
            "Send in its place while the received agent is at rest"
        );
    }

    /// The Hub on `home#0`, with her details open.
    fn ada_app() -> App {
        let mut app = hub_app();
        let ada = crate::testing::seat_ada(&mut app);
        app.insert_resource(hamlet_realm::pick::Selected(Some(
            Thing::Agent(ada),
        )))
        .insert_resource(Showing::Thread)
        .insert_resource(Sheet::Details);
        app.update();
        bevy_fynix::mount::<hamlet_ui::theme::HamletTheme>(
            app.world_mut(),
            crate::hub::screen(),
        );
        app.update();
        app
    }

    fn request(app: &App, what: &str) -> serde_json::Value {
        app.world()
            .resource::<Outbox>()
            .iter()
            .map(|message| serde_json::to_value(message).unwrap())
            .filter(|message| message["what"] == what)
            .last()
            .unwrap_or_else(|| panic!("no read {what}"))
    }

    fn has_span(app: &mut App, text: &str) -> bool {
        app.world_mut()
            .query::<&bevy::text::TextSpan>()
            .iter(app.world())
            .any(|span| span.0 == text)
    }

    fn receive(app: &mut App, message: serde_json::Value) {
        hamlet_client::receive(
            app.world_mut(),
            serde_json::from_value(message).unwrap(),
        );
        app.update();
    }

    #[test]
    fn papers_of_the_agent_and_only_its_finished_work_refresh_the_sheet()
     {
        use serde_json::json;
        let mut app = ada_app();
        let read = request(&app, "papers");
        assert_eq!(read["of"], json!({"agent": 2}));
        receive(
            &mut app,
            json!({"type": "read", "to": read["id"], "papers": {
                "memory": [], "learned": [], "character": "", "skills": []
            }}),
        );
        let shown = words(&mut app);
        assert!(
            shown.iter().any(|text| text == "No memory files yet.")
        );
        assert!(shown.iter().any(|text| text == "Not written yet"));
        app.world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .for_each(drop);
        app.world_mut().trigger(
            hamlet_client::messages::JobFinished {
                chat: Entity::PLACEHOLDER,
            },
        );
        assert!(
            app.world().resource::<Outbox>().iter().next().is_none()
        );
        let chat = app.world().resource::<Convo>().session;
        app.world_mut()
            .trigger(hamlet_client::messages::JobFinished { chat });
        app.update();
        let read = request(&app, "papers");
        receive(
            &mut app,
            json!({"type": "read", "to": read["id"], "papers": {
                "memory": [{"path": "notes.md", "description": "", "text": "Received notes"}], "learned": ["notes.md"], "character": "Received character", "skills": [{"name": "orchard", "description": "Tends trees"}]
            }}),
        );
        assert!(has_span(&mut app, "Received character"));
        assert!(
            words(&mut app)
                .iter()
                .any(|text| text.contains("notes.md (learned)"))
        );
        press(&mut app, "notes.md (learned)");
        app.update();
        assert!(has_span(&mut app, "Received notes"));
        let shown = words(&mut app);
        assert!(shown.iter().any(|text| text == "orchard"));
        assert!(shown.iter().any(|text| text == "Tends trees"));
        press(&mut app, "Reflect now");
        let sent = app
            .world()
            .resource::<Outbox>()
            .iter()
            .map(|m| serde_json::to_value(m).unwrap())
            .find(|m| m["type"] == "say")
            .unwrap();
        assert_eq!(sent["to"], json!(2));
        assert_eq!(sent["text"], "/reflect");
    }

    #[test]
    fn a_prompt_reply_or_failure_is_shown_on_its_page() {
        use serde_json::json;
        let mut app = ada_app();
        show_prompt(app.world_mut());
        assert_eq!(
            *app.world().resource::<Showing>(),
            Showing::Prompt
        );
        let read = request(&app, "prompt");
        assert_eq!(read["agent"], json!(2));
        receive(
            &mut app,
            json!({"type": "read", "to": read["id"], "prompt": "The received prompt"}),
        );
        assert_eq!(
            app.world().resource::<Briefing>().text,
            "The received prompt"
        );
        show_prompt(app.world_mut());
        let read = request(&app, "prompt");
        receive(
            &mut app,
            json!({"type": "reply", "to": read["id"], "kind": "error", "text": "No such agent"}),
        );
        assert_eq!(
            app.world().resource::<Briefing>().text,
            "The prompt could not be read: No such agent"
        );
        assert!(
            app.world_mut()
                .query::<&bevy::text::TextSpan>()
                .iter(app.world())
                .any(|text| text.0
                    == "The prompt could not be read: No such agent")
        );
    }

    #[test]
    fn edits_of_the_character_and_of_a_memory_file_return_to_where_they_began()
     {
        use bevy::ecs::observer::On;
        use bevy::ecs::resource::Resource;
        use bevy::ecs::system::ResMut;
        use hamlet_desktop::editor::{Edit, Edited};
        use serde_json::json;
        #[derive(Resource, Default)]
        struct Edits(Vec<Edit>);
        let mut app = ada_app();
        app.init_resource::<Edits>().add_observer(
            |edit: On<Edit>, mut edits: ResMut<Edits>| {
                edits.0.push(edit.event().clone());
            },
        );
        let papers = request(&app, "papers");
        receive(
            &mut app,
            json!({"type": "read", "to": papers["id"], "papers": {"memory": [{"path": "farm/pears.md", "description": "", "text": "Pears"}], "learned": [], "character": "", "skills": []}}),
        );
        press(&mut app, "Edit");
        let read = request(&app, "papers");
        assert_eq!(read["full"], true);
        let text = "character ".repeat(1000);
        receive(
            &mut app,
            json!({"type": "read", "to": read["id"], "papers": {"memory": [], "learned": [], "character": text, "skills": []}}),
        );
        let edit =
            app.world().resource::<Edits>().0.last().unwrap().clone();
        assert_eq!(edit.text, text);
        app.world_mut().trigger(Edited {
            key: edit.key.clone(),
            text: "Saved character".into(),
        });
        app.update();
        let sent = app
            .world()
            .resource::<Outbox>()
            .iter()
            .map(|m| serde_json::to_value(m).unwrap())
            .find(|m| m["type"] == "edit")
            .unwrap();
        assert_eq!(sent["doc"], "character");
        assert_eq!(sent["scholar"], json!(2));
        assert_eq!(sent["text"], "Saved character");
        receive(
            &mut app,
            json!({"type": "reply", "to": sent["id"], "kind": "error", "text": "Character refused"}),
        );
        assert!(
            words(&mut app)
                .iter()
                .any(|text| text == "Character refused")
        );
        // A memory file of the agent's lodge is edited the same way,
        // and removed by sending no text.
        super::super::papers::edit_memory(
            app.world_mut(),
            "farm/pears.md".into(),
        );
        let read = request(&app, "papers");
        receive(
            &mut app,
            json!({"type": "read", "to": read["id"], "papers": {"memory": [{"path": "farm/pears.md", "description": "", "text": "Whole pears"}], "learned": [], "character": "", "skills": []}}),
        );
        let edit =
            app.world().resource::<Edits>().0.last().unwrap().clone();
        assert_eq!(edit.text, "Whole pears");
        app.world_mut().trigger(Edited {
            key: edit.key,
            text: "Ripe pears".into(),
        });
        app.update();
        let sent = app
            .world()
            .resource::<Outbox>()
            .iter()
            .map(|m| serde_json::to_value(m).unwrap())
            .filter(|m| m["doc"] == "memory")
            .last()
            .unwrap();
        assert_eq!(sent["of"], json!({"agent": 2}));
        assert_eq!(sent["path"], "farm/pears.md");
        assert_eq!(sent["text"], "Ripe pears");
        super::super::papers::remove_memory(
            app.world_mut(),
            "farm/pears.md".into(),
        );
        let removal = app
            .world()
            .resource::<Outbox>()
            .iter()
            .map(|m| serde_json::to_value(m).unwrap())
            .filter(|m| m["doc"] == "memory")
            .last()
            .unwrap();
        assert_eq!(removal["text"], "");
    }

    fn select_scout(app: &mut App) {
        use hamlet_client::chats::Chat;
        let mut state = hamlet_testing::realm::state();
        state
            .agents
            .push(hamlet_testing::realm::agent(3, "home", 1));
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        let agent =
            hamlet_client::realm::named(app.world_mut(), "home#1")
                .unwrap();
        let to = protocol::AgentId(3);
        let session = app
            .world_mut()
            .spawn(Chat(protocol::ChatRef {
                kind: None,
                from: None,
                with: to,
                past: None,
            }))
            .id();
        let mut convo = app.world_mut().resource_mut::<Convo>();
        convo.agent = agent;
        convo.session = session;
        convo.with = Some("home#1".into());
        app.update();
    }

    #[test]
    fn late_reads_cannot_fill_the_next_agents_pages() {
        use serde_json::json;
        let mut app = ada_app();
        let papers = request(&app, "papers");
        show_prompt(app.world_mut());
        let prompt = request(&app, "prompt");
        select_scout(&mut app);
        let scout = request(&app, "papers");
        assert_eq!(scout["of"], json!({"agent": 3}));
        receive(
            &mut app,
            json!({"type": "read", "to": scout["id"], "papers": {"memory": [{"path": "notes.md", "description": "", "text": "Scout notes"}], "learned": [], "character": "Scout character", "skills": []}}),
        );
        receive(
            &mut app,
            json!({"type": "read", "to": papers["id"], "papers": {"memory": [{"path": "notes.md", "description": "", "text": "Wrong notes"}], "learned": [], "character": "Wrong character", "skills": []}}),
        );
        receive(
            &mut app,
            json!({"type": "read", "to": prompt["id"], "prompt": "Wrong prompt"}),
        );
        // The prompt page is left for the thread of the next agent.
        assert_eq!(
            *app.world().resource::<Showing>(),
            Showing::Thread
        );
        assert!(has_span(&mut app, "Scout character"));
        assert!(!has_span(&mut app, "Wrong character"));
        press(&mut app, "notes.md");
        app.update();
        assert!(has_span(&mut app, "Scout notes"));
        assert!(!has_span(&mut app, "Wrong notes"));
        assert_ne!(
            app.world().resource::<Briefing>().text,
            "Wrong prompt"
        );
    }

    #[test]
    fn letters_and_questions_are_in_the_session_menu_and_watched_read_only()
     {
        use hamlet_client::chats::Chat;
        use serde_json::json;
        let mut app = ada_app();
        // The chat is under the details, so they are shut.
        app.insert_resource(Sheet::Chat);
        let mut state = hamlet_testing::realm::state();
        state
            .agents
            .push(hamlet_testing::realm::agent(3, "home", 1));
        state.agents[1].conversations =
            serde_json::from_value(json!([
                {"with":3,"kind":"letters","count":2,"last":1000},
                {"with":3,"kind":"questions","count":1,"last":2000}
            ]))
            .unwrap();
        receive(
            &mut app,
            serde_json::to_value(protocol::ServerMessage::State(
                state,
            ))
            .unwrap(),
        );
        let ada = app.world().resource::<Convo>().agent;
        let (items, now) = entries(app.world());
        let names = items
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "Session",
                "Letters with home#1",
                "Questions with home#1"
            ]
        );
        assert_eq!(now, 0);
        for (kind, (_, go)) in
            ["letters", "questions"].iter().zip(&items[1..])
        {
            go_to(&mut app, go);
            let session = app.world().resource::<Convo>().session;
            let chat =
                app.world().get::<Chat>(session).unwrap().0.clone();
            assert_eq!(
                serde_json::to_value(&chat).unwrap(),
                json!({"with":2, "from":3, "kind":kind})
            );
            assert!(app.world().resource::<Outbox>().iter().any(|m| matches!(m, protocol::ClientMessage::Open(open) if open.chat == chat)));
            receive(
                &mut app,
                json!({"type":"transcript","chat":chat,"items":[
                    {"id":"1:0","at":1,"kind":"user","text":"Question","author":"home#1"},
                    {"id":"2:0","at":2,"kind":"assistant","text":"Answer"}
                ], "earlier":"1:0","last":null,"working":true,"pending":0,"usage":{
                    "input":0,"output":0,"cached":0,"calls":0,"lastInput":0,"reasoning":0,"unreported":0,"summary":"","micros":0,"ms":0
                }}),
            );
            let world = app.world_mut();
            assert_eq!(
                world
                    .query_filtered::<Entity, With<chat::ChatField>>()
                    .iter(world)
                    .count(),
                0
            );
            assert_eq!(
                world.query::<&chat::Rewinds>().iter(world).count(),
                0
            );
            assert!(words(&mut app).contains(&"Read only".into()));
            // The menu shows which of them is open.
            assert_ne!(entries(app.world()).1, 0);
        }
        // The first of the menu leads back to the chief's chat.
        go_to(&mut app, &items[0].1);
        let session = app.world().resource::<Convo>().session;
        assert_eq!(app.world().resource::<Convo>().agent, ada);
        assert!(
            app.world()
                .get::<Chat>(session)
                .unwrap()
                .0
                .from
                .is_none()
        );
    }

    fn go_to(app: &mut App, to: &Go) {
        go(app.world_mut(), to);
        app.update();
    }
}
