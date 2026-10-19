//! The chat: the one a typed line goes to and the line it is typed
//! in. An agent's conversations are laid in one line of time
//! (`timeline`), kept as rows (`lines`) and drawn as a group chat
//! (`bubble`) over the `field` a message is typed in. An edit
//! shows its `change`, and the chief's own messages the way back to
//! them (`rewind`). A message can `reply` to another, and `quote`
//! lines as chat apps do.

mod bubble;
mod change;
mod entry;
mod field;
mod lines;
mod quote;
mod reply;
mod rewind;
mod timeline;

use std::collections::HashSet;

use bevy::app::{App, PostUpdate, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::Event;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::input::ButtonInput;
use bevy::input::keyboard::Key;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::text::{EditableText, TextEdit};
use bevy::ui::{
    AlignItems, ComputedNode, Overflow, ScrollPosition, UiRect,
    percent, px,
};
use bevy_fynix::scroll::ScrollGoal;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, overlay, row, text_field,
};
use bevy_fynix::{ScopedExt, resource};
use hamlet_client::chats::Chat;
use hamlet_client::messages::{
    Response, Rewind, ServerFault, UserMessage,
};
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::MAYOR;
use hamlet_protocol::v0;
use hamlet_realm::orders::{Decree, Tell};

pub(crate) use self::change::change_view;
pub use self::change::{Changes, ChatSettings};
pub(crate) use self::entry::entry_view;
pub use self::field::{
    Placeholder, grow, keep_typed, show_placeholder,
};
pub use self::lines::{
    ChatRow, Lines, Reach, Unfolded, Whole, break_long_words,
    fold_lines, hide_unseen, read_back, transcript,
};
pub use self::rewind::{Rewinds, asked as rewind_asked};
use crate::View;
use crate::looks::well;
use crate::me::{self, Me};

/// The least width of the field a chief types in.
const FIELD_LEAST: f32 = 150.0;

pub fn plugin(app: &mut App) {
    app.init_resource::<Draft>()
        .init_resource::<Requests>()
        .init_resource::<ChatSettings>()
        .init_resource::<Changes>()
        .init_resource::<Unfolded>()
        .init_resource::<Lines>()
        .init_resource::<Reach>()
        .add_observer(keep_typed)
        .add_observer(decreed)
        .add_observer(told)
        .add_observer(fault)
        .add_observer(remember_said)
        .add_observer(remember_rewind)
        .add_observer(response)
        .add_plugins((rewind::plugin, lines::plugin, reply::plugin))
        .add_systems(
            Update,
            (
                (stick_to_bottom, read_back, fold_lines).chain(),
                hide_unseen,
                break_long_words,
                show_placeholder,
                grow,
            ),
        )
        .add_systems(PostUpdate, refocus);
}

/// The text field's root, to find its editable child.
#[derive(Component)]
pub struct ChatField;

/// The scrolling node of the chat.
#[derive(Component)]
pub struct ChatScroll;

/// The input's bound text and the steps of clearing and refocusing it
/// after a send.
#[derive(Resource, Default)]
pub struct Draft {
    pub(crate) text: String,
    pub(crate) step: u8,
    notice: String,
}

/// A failure displayed under the chat field.
#[derive(Event)]
pub(crate) struct ChatError;

fn fault(
    fault: On<ServerFault>,
    mut draft: ResMut<Draft>,
    mut commands: Commands,
) {
    draft.notice.clone_from(&fault.text);
    commands.trigger(ChatError);
}

#[derive(Resource, Default)]
struct Requests(HashSet<String>);

fn remember_said(message: On<UserMessage>, mut commands: Commands) {
    let message = message.event().clone();
    // The client's observers enqueue the requests before this command
    // keeps their ids.
    commands.queue(move |world: &mut World| {
        remember(world, |request| match request {
            v0::ClientMessage::Say(say)
                if say.to == message.to
                    && say.text == message.text =>
            {
                Some(say.id.clone())
            }
            _ => None,
        });
    });
}

fn remember_rewind(
    rewind: On<Rewind>,
    chats: Query<&Chat>,
    mut commands: Commands,
) {
    let Ok(chat) = chats.get(rewind.chat) else {
        return;
    };
    let (chat, item) = (chat.0.clone(), rewind.item.clone());
    commands.queue(move |world: &mut World| {
        remember(world, |request| match request {
            v0::ClientMessage::Rewind(rewind)
                if rewind.chat == chat && rewind.item == item =>
            {
                Some(rewind.id.clone())
            }
            _ => None,
        });
    });
}

fn remember(
    world: &mut World,
    request: impl Fn(&v0::ClientMessage) -> Option<String>,
) {
    let ids = world
        .resource::<Outbox>()
        .iter()
        .filter_map(request)
        .collect::<Vec<_>>();
    world.resource_mut::<Requests>().0.extend(ids);
}

fn response(
    response: On<Response>,
    mut requests: ResMut<Requests>,
    mut draft: ResMut<Draft>,
    mut commands: Commands,
) {
    let text = match &response.message {
        v0::ServerMessage::Reply(reply) => (reply.kind
            == v0::ReplyKind::Error)
            .then_some(&reply.text),
        v0::ServerMessage::Fault(fault) => Some(&fault.text),
        v0::ServerMessage::Transcript(_) => None,
        _ => return,
    };
    if requests.0.remove(&response.to)
        && let Some(text) = text
    {
        draft.notice.clone_from(text);
        commands.trigger(ChatError);
    }
}

/// Puts `words` in the field as quoted lines, after what is typed,
/// with a line under them for the chief's own words.
pub(crate) fn quote_in_field(world: &mut World, words: &str) {
    if words.trim().is_empty() {
        return;
    }
    let mut draft = world.resource_mut::<Draft>();
    let lead =
        match draft.text.is_empty() || draft.text.ends_with('\n') {
            true => "",
            false => "\n",
        };
    draft.text =
        format!("{}{lead}{}\n", draft.text, quote::quoting(words));
    // The field is focused one step on, with the caret at its end.
    draft.step = 1;
}

/// Sends `text` to the open chat.
pub fn send(world: &mut World, text: String) {
    let text = text.trim().to_string();
    if text.is_empty() {
        return;
    }
    if matches!(
        text.split_whitespace().next(),
        Some("/agent" | "/hamlet")
    ) {
        return;
    }
    let me = world.resource::<Me>();
    if world
        .get::<hamlet_client::chats::Chat>(me.session)
        .is_some_and(|chat| chat.0.past.is_some())
    {
        return;
    }
    let to = me.to.clone();
    world.trigger(UserMessage { to, text });
}

/// A chief's command addressed to a hamlet's mayor.
pub(crate) fn decree(world: &mut World, hamlet: &str, text: String) {
    world.trigger(UserMessage {
        to: v0::Address {
            hamlet: hamlet.into(),
            agent: MAYOR.into(),
        },
        text,
    });
}

/// Gives a decree of the world's as [`decree`] does.
pub(crate) fn decreed(decree: On<Decree>, mut commands: Commands) {
    let Decree { hamlet, text } = decree.event().clone();
    commands.queue(move |world: &mut World| {
        self::decree(world, &hamlet, text)
    });
}

/// Says the world's words for an agent in this chief's chat with it.
fn told(tell: On<Tell>, mut commands: Commands) {
    let Tell { agent, text } = tell.event().clone();
    commands.queue(move |world: &mut World| {
        let Some((to, _)) = me::address(world, agent) else {
            return;
        };
        world.trigger(UserMessage { to, text });
        world.flush();
    });
}

fn input_root(world: &mut World) -> Option<Entity> {
    world
        .query_filtered::<Entity, With<ChatField>>()
        .iter(world)
        .next()
}

/// Enter sends. A commit from focus leaving only keeps the draft.
fn commit(world: &mut World, text: String) {
    let root = input_root(world);
    let focused = world.resource::<InputFocus>().get();
    let entering = match (root, focused) {
        (Some(root), Some(f)) => world
            .get::<ChildOf>(f)
            .is_some_and(|c| c.parent() == root),
        _ => false,
    };
    world.resource_mut::<Draft>().text = text.clone();
    if !entering {
        return;
    }
    // Shift+Enter starts a new line, as in the chat apps.
    if world.resource::<ButtonInput<Key>>().pressed(Key::Shift) {
        let mut draft = world.resource_mut::<Draft>();
        draft.text = format!("{text}\n");
        draft.step = 1;
        return;
    }
    // Commands are not typed here: each has a button or a menu.
    if text.trim_start().starts_with('/') {
        world.resource_mut::<Draft>().notice =
            "Commands are not typed here. Use the menus, or \
             right-click an agent or a hamlet."
                .into();
    } else {
        world.resource_mut::<Draft>().notice.clear();
        let text = reply::attach(world, text);
        send(world, text);
    }
    world.resource_mut::<Draft>().step = 3;
}

/// Clears the input after a send, then gives it focus again, with
/// the caret at the end of what is left in it.
pub fn refocus(
    mut draft: ResMut<Draft>,
    root: Query<&Children, With<ChatField>>,
    mut texts: Query<&mut EditableText>,
    mut focus: ResMut<InputFocus>,
) {
    if draft.step == 0 {
        return;
    }
    draft.step -= 1;
    match draft.step {
        2 => draft.text.clear(),
        0 => {
            let input = root
                .iter()
                .flat_map(|c| c.iter().copied())
                .find(|c| texts.contains(*c));
            if let Some(input) = input {
                focus.set(input, FocusCause::Pressed);
                if let Ok(mut text) = texts.get_mut(input) {
                    text.queue_edit(TextEdit::TextEnd(false));
                }
            }
        }
        _ => {}
    }
}

/// The furthest the chat can be scrolled, in pixels.
pub(crate) fn furthest(node: &ComputedNode) -> f32 {
    let seen = node.size().y - node.scrollbar_size.y;
    ((node.content_size().y - seen) * node.inverse_scale_factor())
        .max(0.0)
}

/// The chat held at its end, and the place it was last left.
#[derive(Clone, Copy)]
pub struct AtEnd {
    held: bool,
    was: f32,
}

impl Default for AtEnd {
    fn default() -> Self {
        Self {
            held: true,
            was: 0.0,
        }
    }
}

/// Keeps the newest message in view as the chat grows, until the
/// chief scrolls up to read, and again once they are back at its end.
/// Another conversation is shown from its end.
pub fn stick_to_bottom(
    me: Res<Me>,
    mut scrolls: Query<
        (&ComputedNode, &mut ScrollPosition, Option<&mut ScrollGoal>),
        With<ChatScroll>,
    >,
    mut end: Local<AtEnd>,
) {
    for (node, mut at, goal) in &mut scrolls {
        let most = furthest(node);
        if me.is_changed() {
            end.held = true;
            // The wheel's goal in the last chat means nothing in this
            // one.
            if let Some(mut goal) = goal {
                *goal = ScrollGoal::default();
            }
        } else if at.y < end.was - 0.5 {
            end.held = false;
        } else if at.y >= most - 1.0 {
            end.held = true;
        }
        if end.held && (at.y - most).abs() > 0.5 {
            at.y = most;
        }
        end.was = at.y;
    }
}

/// The one place to type, under the conversation it adds to.
pub fn message_bar() -> impl View {
    let field = row((
        text_field(resource::<Draft, _>(|d| d.text.clone()), commit)
            .grow(1.0)
            .rules(well)
            .tagged(ChatField),
        // Shown in the empty field, under the caret.
        overlay((row((label("Message")
            .size(13.0)
            .wrap(false)
            .tone(Tone::Dim),))
        .align(AlignItems::Center)
        .height(percent(100.0))
        .padding(UiRect::left(px(10.0)))
        .tagged(Placeholder),)),
    ))
    .grow(1.0)
    .min_width(px(FIELD_LEAST))
    .overflow(Overflow::clip());
    column((
        crate::asking::in_chat(),
        reply::preview(),
        row((field,)).gap(8.0).align(AlignItems::Center),
        // An empty label has no height. Keep one line so a
        // notice does not lift the field above it.
        column((label(resource::<Draft, _>(|draft| {
            draft.notice.clone()
        }))
        .size(12.0)
        .tone(Tone::Dim),))
        .min_height(px(15.0)),
    ))
    .gap(4.0)
}

#[cfg(test)]
mod tests {
    use bevy::ecs::message::Messages;
    use hamlet_realm::sound::{Cue, Sound};

    use super::*;

    fn sounding_chat() -> App {
        let mut app = crate::me::tests::panel_app();
        app.add_message::<Cue>()
            .init_resource::<crate::menu::OpenMenu>()
            .init_resource::<crate::world_menu::Open>()
            .init_resource::<bevy_fynix::dock::DockDrag>()
            .init_resource::<crate::inspector::Section>()
            .init_resource::<crate::hover::Card>()
            .add_plugins((plugin, crate::sounds::plugin));
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            message_bar(),
        );
        app.update();
        app
    }

    fn heard(app: &mut App) -> Vec<Sound> {
        app.world_mut()
            .resource_mut::<Messages<Cue>>()
            .drain()
            .map(|cue| cue.sound)
            .collect()
    }

    #[test]
    fn replying_to_a_received_string_id_sends_quoted_words_and_rewinds_them()
     {
        use bevy::input::ButtonState;
        use bevy::input::keyboard::{KeyCode, KeyboardInput};
        use bevy::input_focus::InputDispatchPlugin;
        use bevy::ui_widgets::{Activate, Button};
        use bevy::window::{PrimaryWindow, Window};
        use hamlet_client::chats::{Entry, Item, Transcript};

        use crate::me::tests::panel_app;

        let mut app = panel_app();
        app.add_plugins((
            plugin,
            bevy::input::InputPlugin,
            InputDispatchPlugin,
        ));
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        let session = app.world().resource::<Me>().session;
        app.world_mut().entity_mut(session).insert(
            Transcript::from_items(vec![Item {
                id: "message/é:7".into(),
                at: 7,
                entry: Entry::Assistant("Paper or digital?".into()),
            }]),
        );
        app.update();
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            column((transcript(), message_bar())),
        );
        app.update();
        let world = app.world_mut();
        let mut mark = world
            .query::<(Entity, &bevy::ui::widget::ImageNode)>()
            .iter(world)
            .find(|(_, image)| {
                image.image
                    == crate::icons::Glyph::Talk.image(crate::ICON)
            })
            .unwrap()
            .0;
        while world.get::<Button>(mark).is_none() {
            mark = world.get::<ChildOf>(mark).unwrap().parent();
        }
        world.trigger(Activate { entity: mark });
        app.update();
        let world = app.world_mut();
        let input = world
            .query_filtered::<Entity, With<EditableText>>()
            .single(world)
            .unwrap();
        world
            .resource_mut::<InputFocus>()
            .set(input, FocusCause::Pressed);
        world
            .get_mut::<EditableText>(input)
            .unwrap()
            .editor_mut()
            .set_text("Digital, please.");
        world.write_message(KeyboardInput {
            key_code: KeyCode::Enter,
            logical_key: Key::Enter,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window,
        });
        for _ in 0..4 {
            app.update();
        }
        let text = app
            .world()
            .resource::<Outbox>()
            .iter()
            .find_map(|message| match message {
                v0::ClientMessage::Say(say) => Some(say.text.clone()),
                _ => None,
            })
            .unwrap();
        assert!(text.starts_with("(Replying to "), "{text}");
        assert!(
            text.ends_with(
                ": \"Paper or digital?\")\n\nDigital, please."
            ),
            "{text}"
        );
        app.world_mut().resource_mut::<InputFocus>().clear();
        app.update();
        app.world_mut().trigger(hamlet_client::messages::Rewound {
            chat: session,
            text,
        });
        for _ in 0..4 {
            app.update();
        }
        assert_eq!(
            app.world().get::<EditableText>(input).unwrap().value(),
            "Digital, please."
        );
        assert!(
            crate::me::tests::words(&mut app)
                .iter()
                .any(|text| text.starts_with("Replying to "))
        );
    }

    #[test]
    fn an_unaddressed_fault_shows_under_the_field_and_sounds_once() {
        let mut app = sounding_chat();
        heard(&mut app);
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::Fault(v0::Fault {
                to: None,
                text: "The server could not keep that change.".into(),
            }),
        );
        app.update();
        assert!(
            crate::me::tests::words(&mut app).iter().any(|text| text
                == "The server could not keep that change.")
        );
        assert_eq!(heard(&mut app), [Sound::Error]);
        app.update();
        assert_eq!(heard(&mut app), []);
    }

    #[test]
    fn refusals_of_chat_requests_show_and_sound_but_panel_replies_do_not()
     {
        use hamlet_client::messages::Rewind;
        use hamlet_client::outbox::Outbox;

        let mut app = sounding_chat();
        let chat = app.world().resource::<Me>().session;
        for (rewind, fault) in
            [(false, false), (true, false), (false, true)]
        {
            if rewind {
                app.world_mut().trigger(Rewind {
                    chat,
                    item: "chief:0".into(),
                });
            } else {
                send(app.world_mut(), "Find the key".into());
            }
            app.world_mut().flush();
            let id = app
                .world_mut()
                .resource_mut::<Outbox>()
                .drain()
                .find_map(|message| match message {
                    v0::ClientMessage::Say(say) => Some(say.id),
                    v0::ClientMessage::Rewind(rewind) => {
                        Some(rewind.id)
                    }
                    _ => None,
                })
                .unwrap();
            heard(&mut app);
            let text = "This conversation cannot do that.".into();
            let message = if fault {
                v0::ServerMessage::Fault(v0::Fault {
                    to: Some(id),
                    text,
                })
            } else {
                v0::ServerMessage::Reply(v0::Reply {
                    to: id,
                    kind: v0::ReplyKind::Error,
                    text,
                    upload: None,
                })
            };
            hamlet_client::receive(app.world_mut(), message);
            app.update();
            assert!(
                crate::me::tests::words(&mut app)
                    .iter()
                    .any(|text| text
                        == "This conversation cannot do that.")
            );
            assert_eq!(heard(&mut app), [Sound::Error]);
            app.update();
            assert_eq!(heard(&mut app), []);
        }
        let id =
            app.world_mut().resource_mut::<Outbox>().send(|id| {
                v0::ClientMessage::Check(v0::Check::Connection { id })
            });
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::Reply(v0::Reply {
                to: id,
                kind: v0::ReplyKind::Error,
                text: "Only the settings panel should show this."
                    .into(),
                upload: None,
            }),
        );
        app.update();
        assert!(
            !crate::me::tests::words(&mut app)
                .iter()
                .any(|text| text
                    == "Only the settings panel should show this.")
        );
        assert_eq!(heard(&mut app), []);
    }

    #[test]
    fn typed_commands_are_refused_locally_and_address_commands_never_leave()
     {
        use bevy::input::ButtonState;
        use bevy::input::keyboard::{Key, KeyCode, KeyboardInput};
        use bevy::input_focus::InputDispatchPlugin;
        use bevy::window::{PrimaryWindow, Window};

        let mut app = sounding_chat();
        app.add_plugins((
            bevy::input::InputPlugin,
            InputDispatchPlugin,
        ));
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        let world = app.world_mut();
        let input = world
            .query_filtered::<Entity, With<EditableText>>()
            .single(world)
            .unwrap();
        for command in ["/new", "/agent founder", "/hamlet home"] {
            app.world_mut()
                .resource_mut::<InputFocus>()
                .set(input, FocusCause::Pressed);
            app.update();
            app.world_mut()
                .get_mut::<EditableText>(input)
                .unwrap()
                .editor_mut()
                .set_text(command);
            app.world_mut().write_message(KeyboardInput {
                key_code: KeyCode::Enter,
                logical_key: Key::Enter,
                state: ButtonState::Pressed,
                text: None,
                repeat: false,
                window,
            });
            for _ in 0..4 {
                app.update();
            }
            assert!(crate::me::tests::words(&mut app).iter().any(
                |text| text
                    == "Commands are not typed here. Use the menus, \
                        or right-click an agent or a hamlet."
            ));
        }
        send(app.world_mut(), "/agent founder".into());
        send(app.world_mut(), "/hamlet home".into());
        assert!(!app.world().resource::<Outbox>().iter().any(
            |message| matches!(message, v0::ClientMessage::Say(_))
        ));
    }

    fn paging_pages() -> Vec<v0::Transcript> {
        include_str!("../../../hamlet_protocol/frames/paging.jsonl")
            .lines()
            .filter_map(|line| {
                let frame =
                    serde_json::from_str::<serde_json::Value>(line)
                        .unwrap();
                (frame["from"] == "server"
                    && frame["message"]["type"] == "transcript")
                    .then(|| {
                        serde_json::from_value::<v0::Transcript>(
                            frame["message"].clone(),
                        )
                        .unwrap()
                    })
            })
            .collect::<Vec<_>>()
    }

    #[test]
    fn a_chat_shorter_than_the_viewport_reads_earlier_pages_without_scrolling()
     {
        use hamlet_client::outbox::Outbox;
        let mut app = crate::me::tests::panel_app();
        app.add_plugins(plugin);
        app.update();
        let pages = paging_pages();
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::Transcript(pages[0].clone()),
        );
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            transcript(),
        );
        app.update();
        let world = app.world_mut();
        let scroll = world
            .query_filtered::<Entity, With<ChatScroll>>()
            .single(world)
            .unwrap();
        world.entity_mut(scroll).insert(ComputedNode {
            size: bevy::math::Vec2::new(300.0, 1000.0),
            content_size: bevy::math::Vec2::new(300.0, 400.0),
            inverse_scale_factor: 1.0,
            ..Default::default()
        });
        for _ in 0..10 {
            app.update();
        }
        let requests = app
            .world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .filter_map(|message| match message {
                v0::ClientMessage::Open(open)
                    if open.before.is_some() =>
                {
                    Some(open)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(requests.len(), 1, "earlier page without scroll");
        let mut page = pages[1].clone();
        page.to = Some(requests[0].id.clone());
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::Transcript(page),
        );
        for _ in 0..10 {
            app.update();
        }
        let shown = crate::me::tests::words(&mut app).join("\n");
        assert!(shown.contains("Line 1."), "{shown}");
        assert!(
            !shown.contains("Earlier lines. Scroll up to read them.")
        );
        assert!(!app.world().resource::<Outbox>().iter().any(|message| matches!(message, v0::ClientMessage::Open(open) if open.before.is_some())));
    }

    #[test]
    fn scrolling_back_pages_once_and_shows_the_received_page_above() {
        use hamlet_client::outbox::Outbox;

        use crate::me::tests::{panel_app, words};
        let mut app = panel_app();
        app.add_plugins(plugin);
        app.update();
        let pages = paging_pages();
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::Transcript(pages[0].clone()),
        );
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            transcript(),
        );
        app.update();
        let world = app.world_mut();
        assert_eq!(
            world
                .query_filtered::<Entity, With<ChatRow>>()
                .iter(world)
                .count(),
            24
        );
        let scroll = world
            .query_filtered::<Entity, With<ChatScroll>>()
            .single(world)
            .unwrap();
        world.entity_mut(scroll).insert(ComputedNode {
            size: bevy::math::Vec2::new(300.0, 200.0),
            content_size: bevy::math::Vec2::new(300.0, 1000.0),
            inverse_scale_factor: 1.0,
            ..ComputedNode::default()
        });
        app.update();
        assert!(
            words(&mut app).iter().any(|text| text
                == "Earlier lines. Scroll up to read them.")
        );
        app.world_mut()
            .get_mut::<ScrollPosition>(scroll)
            .unwrap()
            .y = 0.0;
        app.update();
        app.world_mut()
            .get_mut::<ComputedNode>(scroll)
            .unwrap()
            .content_size
            .y = 1200.0;
        app.update();
        app.world_mut()
            .get_mut::<ScrollPosition>(scroll)
            .unwrap()
            .y = 0.0;
        for _ in 0..4 {
            app.update();
        }
        let requests = app
            .world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .filter_map(|message| match message {
                v0::ClientMessage::Open(open)
                    if open.before.is_some() =>
                {
                    Some(open)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(requests.len(), 1);
        assert!(pages[0].earlier.is_some());
        assert_eq!(requests[0].before, pages[0].earlier);
        app.world_mut()
            .trigger(hamlet_client::messages::Reconnected);
        app.update();
        let reopened = app
            .world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .find_map(|message| match message {
                v0::ClientMessage::Open(open)
                    if open.before.is_none() =>
                {
                    Some(open)
                }
                _ => None,
            })
            .unwrap();
        let mut latest = pages[0].clone();
        latest.to = Some(reopened.id);
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::Transcript(latest),
        );
        app.update();
        let requests = app
            .world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .filter_map(|message| match message {
                v0::ClientMessage::Open(open)
                    if open.before.is_some() =>
                {
                    Some(open)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            requests.len(),
            1,
            "paging remains available after reconnecting while a \
             page was out"
        );
        let mut page = pages[1].clone();
        page.to = Some(requests[0].id.clone());
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::Transcript(page),
        );
        app.update();
        app.world_mut()
            .get_mut::<ComputedNode>(scroll)
            .unwrap()
            .content_size
            .y = 1400.0;
        app.update();
        assert_eq!(
            app.world().get::<ScrollPosition>(scroll).unwrap().y,
            200.0
        );
        let shown = words(&mut app);
        assert!(
            shown.iter().any(|text| text.trim_end() == "Line 1."),
            "{shown:?}"
        );
        assert!(
            !shown.iter().any(|text| text
                == "Earlier lines. Scroll up to read them.")
        );
        let world = app.world_mut();
        let mut children = world.query::<&Children>();
        let row_times = children
            .query(world)
            .iter_descendants(scroll)
            .filter_map(|entity| {
                world.get::<Rewinds>(entity).map(|row| row.at)
            })
            .collect::<Vec<_>>();
        assert_eq!(row_times.len(), 23);
        assert!(
            row_times.windows(2).all(|pair| pair[0] <= pair[1]),
            "the page precedes the kept rows"
        );
    }

    #[test]
    fn received_items_show_tool_outcomes_folded_steps_and_notices() {
        use hamlet_client::chats::{Entry, Item, Transcript};

        use crate::me::tests::{panel_app, press, words};
        let mut app = panel_app();
        app.add_plugins(plugin);
        let session = app.world().resource::<Me>().session;
        let tool = |id: &str, exit| Item {
            id: id.into(),
            at: 0,
            entry: Entry::Tool {
                name: "bash".into(),
                summary: "pwd".into(),
                exit,
                ms: Some(3),
                failure: None,
                change: None,
            },
        };
        app.world_mut().entity_mut(session).insert(
            Transcript::from_items(vec![
                Item {
                    id: "chief:0".into(),
                    at: 0,
                    entry: Entry::User {
                        user: "chief".into(),
                        text: "Go".into(),
                    },
                },
                tool("done:0", Some(0)),
                tool("failed:0", Some(7)),
                Item {
                    id: "reply:0".into(),
                    at: 0,
                    entry: Entry::Assistant("Still working.".into()),
                },
                tool("running:0", None),
                Item {
                    id: "reply:1".into(),
                    at: 0,
                    entry: Entry::Assistant("The next step.".into()),
                },
                Item {
                    id: "notice:0".into(),
                    at: 0,
                    entry: Entry::Notice("A received notice".into()),
                },
            ]),
        );
        app.update();
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            transcript(),
        );
        app.update();
        press(&mut app, "+  Worked, 2 steps, 1 failed");
        press(&mut app, "+  Working, 1 step so far");
        press(&mut app, "+  Thought it over");
        for _ in 0..3 {
            app.update();
        }
        let shown = words(&mut app);
        for text in [
            "Done",
            "Failed (exit 7)",
            "Running",
            "A received notice",
        ] {
            assert!(
                shown.iter().any(|shown| shown == text),
                "missing {text}: {shown:?}"
            );
        }
    }

    #[test]
    fn a_row_rewinds_by_its_string_id_and_returns_the_words_to_the_field()
     {
        use bevy::ui_widgets::{Activate, Button};
        use hamlet_client::chats::{Entry, Item, Transcript};
        use hamlet_client::outbox::Outbox;

        use crate::me::tests::{panel_app, press};
        let mut app = panel_app();
        app.add_plugins(plugin)
            .init_resource::<crate::world_menu::Open>();
        let session = app.world().resource::<Me>().session;
        app.world_mut().entity_mut(session).insert(
            Transcript::from_items(vec![Item {
                id: "entry-abc:0".into(),
                at: 1,
                entry: Entry::User {
                    user: "chief".into(),
                    text: "Change these words".into(),
                },
            }]),
        );
        app.update();
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            bevy_fynix::views::column((
                transcript(),
                message_bar(),
                crate::world_menu::view(),
            )),
        );
        app.update();
        let world = app.world_mut();
        let mut mark = world
            .query::<(Entity, &bevy::ui::widget::ImageNode)>()
            .iter(world)
            .find(|(_, image)| {
                image.image
                    == crate::icons::Glyph::Restore.image(crate::ICON)
            })
            .unwrap()
            .0;
        while world.get::<Button>(mark).is_none() {
            mark = world.get::<ChildOf>(mark).unwrap().parent();
        }
        world.trigger(Activate { entity: mark });
        app.update();
        press(&mut app, "REWIND");
        let request = app
            .world()
            .resource::<Outbox>()
            .iter()
            .find_map(|message| match message {
                v0::ClientMessage::Rewind(request) => {
                    Some(request.clone())
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(request.item, "entry-abc:0");
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::Reply(v0::Reply {
                to: request.id,
                kind: v0::ReplyKind::Rewound,
                text: "Change these words".into(),
                upload: None,
            }),
        );
        for _ in 0..3 {
            app.update();
        }
        assert!(
            app.world_mut()
                .query::<&EditableText>()
                .iter(app.world())
                .any(|field| field.value() == "Change these words")
        );
    }

    #[test]
    fn the_chat_stays_at_its_end_until_it_is_read_back() {
        let mut app = bevy::app::App::new();
        let world = app.world_mut();
        let nobody = world.spawn_empty().id();
        world.insert_resource(Me::of(nobody, nobody, "wren", None));
        let tall = |content: f32| ComputedNode {
            size: bevy::math::Vec2::new(300.0, 200.0),
            content_size: bevy::math::Vec2::new(300.0, content),
            inverse_scale_factor: 1.0,
            ..ComputedNode::default()
        };
        let chat = world
            .spawn((ChatScroll, tall(1000.0), ScrollGoal::default()))
            .id();
        app.add_systems(bevy::app::Update, stick_to_bottom);
        let shown = |app: &mut bevy::app::App,
                     content: f32,
                     scrolled: Option<f32>| {
            let mut node = app.world_mut().entity_mut(chat);
            node.insert(tall(content));
            if let Some(to) = scrolled {
                node.get_mut::<ScrollPosition>().unwrap().y = to;
            }
            app.update();
            app.world().get::<ScrollPosition>(chat).unwrap().y
        };
        // At its end, and no further: the wheel starts from here.
        assert_eq!(shown(&mut app, 1000.0, None), 800.0);
        assert_eq!(shown(&mut app, 1200.0, None), 1000.0);
        // Read back, it stays where it was put as more is said.
        assert_eq!(shown(&mut app, 1200.0, Some(940.0)), 940.0);
        assert_eq!(shown(&mut app, 1500.0, None), 940.0);
        // Back at its end, it follows again.
        assert_eq!(shown(&mut app, 1500.0, Some(1300.0)), 1300.0);
        assert_eq!(shown(&mut app, 1700.0, None), 1500.0);
        // Another chat is shown from its end, wherever this one was.
        shown(&mut app, 1700.0, Some(300.0));
        app.world_mut().resource_mut::<Me>().with =
            Some("Sorrel".into());
        assert_eq!(shown(&mut app, 900.0, None), 700.0);
    }
}
