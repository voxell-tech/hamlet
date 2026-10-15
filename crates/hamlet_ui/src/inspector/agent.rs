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
use hamlet_client::messages::UserMessage;
use hamlet_client::realm::{
    Account, Activity, Busy, LivesIn, Purpose, Treasury,
};
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
use crate::world_menu::{self};
use crate::{AnyView, Cx, View, chat, markdown, tabs, with_cx};

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
        .map(|treasury| treasury.0.period_words.clone())
        .filter(|period| !period.is_empty())
        .unwrap_or_else(|| "today".into());
    format!("Spent {period}: {}", account.cost.clone())
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
            world
                .get::<Activity>(agent)
                .cloned()
                .unwrap_or_default()
                .words
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
                    hamlet_client::realm::name_of(cx.world, hamlet);
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
            let mayor = hamlet_client::realm::mayor(world);
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
        if let Some(to) = super::papers::selected(w) {
            w.trigger(UserMessage {
                to,
                text: "/reflect".into(),
            });
        }
    });
    column((row((reflect,)), scroll((shown,)).fills_height()))
        .fills_height()
        .gap(8.0)
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
        action(Glyph::Rename, "EDIT", super::papers::edit_character)
            .tooltip(|| {
                label("Opens the file in your editor").size(12.0)
            });
    let failed = keyed(
        resource::<Papers, _>(|p| p.failed.clone()),
        |failed| match failed {
            Some(why) => {
                label(why.clone()).tone(Tone::Critical).boxed()
            }
            None => frame().boxed(),
        },
    )
    .within(bare());
    column((
        row((edit, failed)).gap(8.0),
        scroll((shown,)).fills_height(),
    ))
    .fills_height()
    .gap(8.0)
}

/// Reads the prompt the agent the chat is with would be given now,
/// and turns its page to it.
pub(crate) fn show_prompt(world: &mut World) {
    let me = world.resource::<Me>();
    let agent = me.agent;
    *world.resource_mut::<Briefing>() = Briefing {
        agent: Some(agent),
        text: String::new(),
    };
    super::papers::prompt(world);
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

fn prompt(cx: &Cx) -> impl View + use<> {
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
        hairline(cx.theme().palette.brass_6),
        scroll((shown,)).fills_height(),
    ))
    .fills_height()
    .gap(8.0)
}

/// The sections as tabs.
fn tabs() -> AnyView {
    tabs::strip(&TABS, |world, section| {
        *world.resource_mut::<Section>() = section;
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
        Section::Prompt => with_cx(prompt),
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

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use hamlet_client::outbox::Outbox;
    use hamlet_protocol::v0;

    use super::*;
    use crate::me::tests::{panel_app, press, words};

    #[test]
    fn the_header_shows_received_work_and_stops_only_a_busy_agent() {
        let mut app = panel_app();
        app.add_plugins((
            crate::chat::plugin,
            crate::inspector::plugin,
        ));
        app.init_resource::<Bar>();
        let mut state = hamlet_testing::realm::state();
        state.agents[0].busy = true;
        state.agents[0].activity.doing_words =
            "Reading the received orchard ledger".into();
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::State(state),
        );
        app.update();
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            living(),
        );
        app.update();
        assert!(words(&mut app).iter().any(
            |text| text == "Reading the received orchard ledger"
        ));
        press(&mut app, "STOP");
        let stopped =
            app.world().resource::<Outbox>().iter().any(|message| {
                let v0::ClientMessage::Stop(stop) = message else {
                    return false;
                };
                stop.agent.as_ref().is_some_and(|agent| {
                    agent.hamlet == "home" && agent.agent == "founder"
                })
            });
        assert!(stopped);
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::State(hamlet_testing::realm::state()),
        );
        for _ in 0..2 {
            app.update();
        }
        let world = app.world_mut();
        let stop = world
            .query::<(Entity, &bevy::ui::widget::Text)>()
            .iter(world)
            .find(|(_, text)| text.0 == "STOP")
            .unwrap()
            .0;
        let mut at = stop;
        let mut hidden = false;
        loop {
            hidden |= world
                .get::<bevy::ui::Node>(at)
                .is_some_and(|node| node.display == Display::None);
            let Some(parent) =
                world.get::<bevy::ecs::hierarchy::ChildOf>(at)
            else {
                break;
            };
            at = parent.parent();
        }
        assert!(
            hidden,
            "STOP is hidden while the received agent is at rest"
        );
    }

    fn inspector_app() -> App {
        let mut app = panel_app();
        app.add_plugins((
            crate::chat::plugin,
            crate::inspector::plugin,
        ));
        app.init_resource::<Bar>();
        let agent = app.world().resource::<Me>().agent;
        app.insert_resource(hamlet_world::pick::Selected(Some(
            Thing::Resident(agent),
        )));
        app.update();
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            crate::inspector::screen(),
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
        app.update();
    }

    #[test]
    fn selected_papers_and_only_its_finished_work_refresh_the_pages()
    {
        use serde_json::json;
        let mut app = inspector_app();
        let read = request(&app, "papers");
        assert_eq!(
            read["agent"],
            json!({"hamlet": "home", "agent": "founder"})
        );
        receive(
            &mut app,
            json!({"type": "read", "to": read["id"], "papers": {
                "notes": "", "character": "", "skills": []
            }}),
        );
        press(&mut app, "NOTES");
        app.update();
        assert!(
            words(&mut app)
                .iter()
                .any(|text| text == "No notes yet.")
        );
        press(&mut app, "CHARACTER");
        app.update();
        assert!(words(&mut app).iter().any(|text| text
            == "Write here how this agent behaves and speaks."));
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
        let chat = app.world().resource::<Me>().session;
        app.world_mut()
            .trigger(hamlet_client::messages::JobFinished { chat });
        app.update();
        let read = request(&app, "papers");
        receive(
            &mut app,
            json!({"type": "read", "to": read["id"], "papers": {
                "notes": "Received notes", "character": "Received character", "skills": [{"name": "orchard", "description": "Tends trees"}]
            }}),
        );
        assert!(has_span(&mut app, "Received character"));
        press(&mut app, "NOTES");
        app.update();
        assert!(has_span(&mut app, "Received notes"));
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            crate::inspector::skills_screen(),
        );
        app.update();
        let shown = words(&mut app);
        assert!(shown.iter().any(|text| text == "orchard"));
        assert!(shown.iter().any(|text| text == "Tends trees"));
        press(&mut app, "REFLECT NOW");
        let sent = app
            .world()
            .resource::<Outbox>()
            .iter()
            .map(|m| serde_json::to_value(m).unwrap())
            .find(|m| m["type"] == "say")
            .unwrap();
        assert_eq!(
            sent["to"],
            json!({"hamlet": "home", "agent": "founder"})
        );
        assert_eq!(sent["text"], "/reflect");
    }

    #[test]
    fn a_prompt_reply_or_failure_is_shown_on_its_page() {
        use serde_json::json;
        let mut app = inspector_app();
        crate::inspector::show_prompt(app.world_mut());
        let read = request(&app, "prompt");
        assert_eq!(read["agent"]["agent"], "founder");
        receive(
            &mut app,
            json!({"type": "read", "to": read["id"], "prompt": "The received prompt"}),
        );
        assert_eq!(
            app.world().resource::<Briefing>().text,
            "The received prompt"
        );
        crate::inspector::show_prompt(app.world_mut());
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
    fn character_edits_read_the_whole_document_and_save_to_its_original_agent()
     {
        use bevy::ecs::observer::On;
        use bevy::ecs::resource::Resource;
        use bevy::ecs::system::ResMut;
        use hamlet_desktop::editor::{Edit, Edited};
        use serde_json::json;
        #[derive(Resource, Default)]
        struct Edits(Vec<Edit>);
        let mut app = inspector_app();
        app.init_resource::<Edits>().add_observer(
            |edit: On<Edit>, mut edits: ResMut<Edits>| {
                edits.0.push(edit.event().clone())
            },
        );
        press(&mut app, "CHARACTER");
        app.update();
        press(&mut app, "EDIT");
        let read = request(&app, "papers");
        assert_eq!(read["full"], true);
        let text = "character ".repeat(1000);
        receive(
            &mut app,
            json!({"type": "read", "to": read["id"], "papers": {"notes": "", "character": text, "skills": []}}),
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
        assert_eq!(
            sent["agent"],
            json!({"hamlet": "home", "agent": "founder"})
        );
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
        select_scout(&mut app);
        app.world_mut().trigger(Edited {
            key: edit.key,
            text: "Still the founder's character".into(),
        });
        app.update();
        let sent = app
            .world()
            .resource::<Outbox>()
            .iter()
            .map(|m| serde_json::to_value(m).unwrap())
            .filter(|m| m["type"] == "edit")
            .last()
            .unwrap();
        assert_eq!(
            sent["agent"],
            json!({"hamlet": "home", "agent": "founder"})
        );
        assert_eq!(sent["text"], "Still the founder's character");
    }

    fn select_scout(app: &mut App) {
        use hamlet_client::chats::Chat;
        let mut state = hamlet_testing::realm::state();
        state
            .agents
            .push(hamlet_testing::realm::agent("home", 1, "scout"));
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::State(state),
        );
        let agent =
            hamlet_client::realm::named(app.world_mut(), "scout")
                .unwrap();
        let to = v0::Address {
            hamlet: "home".into(),
            agent: "scout".into(),
        };
        let session = app
            .world_mut()
            .spawn(Chat(v0::ChatRef {
                from: None,
                with: to.clone(),
                past: None,
            }))
            .id();
        let mut me = app.world_mut().resource_mut::<Me>();
        me.agent = agent;
        me.session = session;
        me.to = to;
        me.with = Some("scout".into());
        app.update();
    }

    #[test]
    fn late_reads_cannot_fill_the_next_agents_pages() {
        use serde_json::json;
        let mut app = inspector_app();
        let papers = request(&app, "papers");
        crate::inspector::show_prompt(app.world_mut());
        let prompt = request(&app, "prompt");
        select_scout(&mut app);
        let scout = request(&app, "papers");
        assert_eq!(scout["agent"]["agent"], "scout");
        receive(
            &mut app,
            json!({"type": "read", "to": scout["id"], "papers": {"notes": "Scout notes", "character": "Scout character", "skills": []}}),
        );
        receive(
            &mut app,
            json!({"type": "read", "to": papers["id"], "papers": {"notes": "Wrong notes", "character": "Wrong character", "skills": []}}),
        );
        receive(
            &mut app,
            json!({"type": "read", "to": prompt["id"], "prompt": "Wrong prompt"}),
        );
        assert!(matches!(
            *app.world().resource::<Section>(),
            Section::Conversation
        ));
        press(&mut app, "NOTES");
        app.update();
        assert!(has_span(&mut app, "Scout notes"));
        assert!(!has_span(&mut app, "Wrong notes"));
        press(&mut app, "CHARACTER");
        app.update();
        assert!(has_span(&mut app, "Scout character"));
        assert!(!has_span(&mut app, "Wrong character"));
        assert_ne!(
            app.world().resource::<Briefing>().text,
            "Wrong prompt"
        );
    }
}
