//! The chat: the one a typed line goes to and the line it is typed
//! in. An agent's one conversation is laid in one line of time
//! (`timeline`), kept as rows (`lines`) and drawn as a group chat
//! (`bubble`) over the `field` a message is typed in, where what is
//! attached sits as chips (`attach`). An edit shows its `change`, a
//! turn the files it changed (`edits`),
//! the chief's own messages the way back to them (`rewind`), and a
//! summary, a merge or a worker its `card`. A message can `reply` to
//! another, and `quote` lines as chat apps do.

mod anchor;
pub(crate) mod attach;
pub(crate) mod bubble;
mod card;
mod change;
mod commands;
mod edits;
mod entry;
mod field;
mod held;
mod lines;
mod quote;
pub(crate) mod reply;
mod rewind;
mod timeline;

use std::collections::HashMap;

use bevy::app::{App, PostUpdate, PreUpdate, Update};
use bevy::color::Color;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::Event;
use bevy::ecs::lifecycle::Remove;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::ui::{ComputedNode, Overflow, ScrollPosition, UiRect, px};
use bevy_fynix::scroll::ScrollGoal;
use bevy_fynix::views::{
    BehaviorExt, Enter, Frame, FrameProps, Piece, VirtualList,
    button, column, rich_field, row,
};
use bevy_fynix::{
    ScopedExt, ViewExt, derived, every_frame, resource, style,
};
use hamlet_client::chats::Chat;
use hamlet_client::messages::{
    ChiefMessage, Compact, Response, Resume, Rewind, ServerFault,
    Withdraw,
};
use hamlet_client::realm as agents;
use hamlet_protocol::client as protocol;
use hamlet_realm::hand::FileDrop;
use hamlet_realm::orders::{Decree, Tell};
use hamlet_ui::styles::{ICON, Looks, Tipped, note, text_input};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::hint::hinted;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, View, with_cx};

pub(crate) use self::change::change_view;
pub use self::change::{Changes, ChatSettings};
pub use self::entry::OpenTools;
pub(crate) use self::entry::Underway;
pub use self::field::refocus;
pub(crate) use self::lines::close_merged;
pub use self::lines::{
    Lines, MergedLines, Unfolded, ask_earlier, break_long_words,
    fold_lines, fold_merged, transcript,
};
use self::lines::{remember_place, restore_place};
pub use self::rewind::{Rewinds, ask as ask_rewind};
use crate::PanelSystems;
use crate::convo::{self, Convo};

/// The room at the sides of the field, as the theme gives a field.
const FIELD_SIDE: f32 = 8.0;

/// The room above and below a single line of the field: what is
/// left of its height once the line and its frame are taken out.
const LINE_ROOM: f32 = 4.5;

/// The least width of the field a chief types in.
const FIELD_LEAST: f32 = 150.0;

/// The most lines the field grows to before it scrolls.
const MOST_LINES: usize = 8;

pub fn plugin(app: &mut App) {
    app.init_resource::<Draft>()
        .init_resource::<Requests>()
        .init_resource::<ChatSettings>()
        .init_resource::<Changes>()
        .init_resource::<Unfolded>()
        .init_resource::<Lines>()
        .init_resource::<ToEnd>()
        .init_resource::<field::Typed>()
        .init_resource::<AwayFromEnd>()
        .init_resource::<hamlet_ui::fields::Resized>()
        .add_observer(decreed)
        .add_observer(told)
        .add_observer(fault)
        .add_observer(remember_said)
        .add_observer(remember_rewind)
        .add_observer(remember_compact)
        .add_observer(remember_withdraw)
        .add_observer(remember_resume)
        .add_observer(response)
        .add_observer(forget_chat)
        .init_resource::<OpenTools>()
        // A row fades while a rewind that would forget it is asked
        // about.
        .init_resource::<crate::world_menu::Open>()
        .add_plugins((
            anchor::plugin,
            attach::plugin,
            rewind::plugin,
            lines::plugin,
            reply::plugin,
        ))
        // The rows are folded before fynix mounts them, so a received
        // reply shows in the update it arrives.
        .add_systems(
            PreUpdate,
            (
                remember_place,
                keep_end,
                ask_earlier,
                fold_lines,
                fold_merged,
                restore_place,
                field::swap_drafts,
            )
                .chain()
                .in_set(PanelSystems::Bind),
        )
        .add_systems(Update, break_long_words)
        .add_systems(PostUpdate, refocus);
}

/// The field a chief types in, which holds the line's text and the
/// chips of what is attached to it.
#[derive(Component)]
pub struct ChatField;

/// The scrolling node of the chat.
#[derive(Component)]
pub struct ChatScroll;

/// Whether the field is to be given the focus, and what is said under
/// it.
#[derive(Resource, Default)]
pub struct Draft {
    pub(crate) focus: bool,
    notice: String,
}

/// Forgets the folds, tool calls and changes opened in a chat that is
/// gone, and shuts a merged conversation open in it.
fn forget_chat(
    gone: On<Remove<Chat>>,
    mut unfolded: ResMut<Unfolded>,
    mut tools: ResMut<OpenTools>,
    mut changes: ResMut<Changes>,
    merged: Res<MergedLines>,
    mut commands: Commands,
) {
    let chat = gone.entity;
    unfolded.0.retain(|(of, _)| *of != chat);
    tools.0.retain(|(of, _)| *of != chat);
    changes.forget(chat);
    if merged.is_in(chat) {
        commands.queue(close_merged);
    }
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

/// The requests sent from the chat whose replies it says under the
/// field.
#[derive(Resource, Default)]
struct Requests(HashMap<protocol::RequestId, Said>);

/// What of a request's reply the chat says.
#[derive(Clone, Copy, PartialEq)]
enum Said {
    Refusal,
    Reply,
}

/// Says the refusal of the request `id` under the field.
pub(super) fn refused(world: &mut World, id: protocol::RequestId) {
    world.resource_mut::<Requests>().0.insert(id, Said::Refusal);
}

/// Says the reply to the request `id` under the field, or its
/// refusal.
pub(super) fn replied(world: &mut World, id: protocol::RequestId) {
    world.resource_mut::<Requests>().0.insert(id, Said::Reply);
}

fn remember_said(
    message: On<ChiefMessage>,
    mut requests: ResMut<Requests>,
) {
    requests.0.insert(message.request.clone(), Said::Refusal);
}

fn remember_rewind(
    rewind: On<Rewind>,
    mut requests: ResMut<Requests>,
) {
    requests.0.insert(rewind.request.clone(), Said::Refusal);
}

fn remember_compact(
    compact: On<Compact>,
    mut requests: ResMut<Requests>,
) {
    requests.0.insert(compact.request.clone(), Said::Refusal);
}

fn remember_withdraw(
    withdraw: On<Withdraw>,
    mut requests: ResMut<Requests>,
) {
    requests.0.insert(withdraw.request.clone(), Said::Refusal);
}

fn remember_resume(
    resume: On<Resume>,
    mut requests: ResMut<Requests>,
) {
    requests.0.insert(resume.request.clone(), Said::Reply);
}

fn response(
    response: On<Response>,
    mut requests: ResMut<Requests>,
    mut draft: ResMut<Draft>,
    mut commands: Commands,
) {
    let (text, refused) = match &response.message {
        protocol::ServerMessage::Reply(reply) => {
            (&reply.text, reply.kind == protocol::ReplyKind::Error)
        }
        protocol::ServerMessage::Fault(fault) => (&fault.text, true),
        // A page asked for came.
        protocol::ServerMessage::Transcript(_) => {
            requests.0.remove(&response.to);
            return;
        }
        _ => return,
    };
    let Some(said) = requests.0.remove(&response.to) else {
        return;
    };
    if refused {
        draft.notice.clone_from(text);
        commands.trigger(ChatError);
    } else if said == Said::Reply {
        draft.notice.clone_from(text);
    }
}

/// Puts `words` in the field as quoted lines, after what is typed,
/// with a line under them for the chief's own words.
pub(crate) fn quote_in_field(world: &mut World, words: &str) {
    if words.trim().is_empty() {
        return;
    }
    let quoted = format!("{}\n", quote::quoting(words));
    field::with_field(world, |mut field| {
        let mut pieces = field.pieces();
        let lead = match pieces.last() {
            None => "",
            Some(Piece::Text(text)) if text.ends_with('\n') => "",
            Some(_) => "\n",
        };
        pieces.push(Piece::Text(format!("{lead}{quoted}")));
        // The caret is at the end of what is put in.
        field.set_content(pieces);
    });
    world.resource_mut::<Draft>().focus = true;
}

/// Sends `text` to the open chat, with its attachments at `places`
/// in it, and says whether it went.
pub(crate) fn send(
    world: &mut World,
    text: String,
    places: &attach::Places,
) -> bool {
    let text = text.trim().to_string();
    let ready = world
        .get_resource::<attach::Attached>()
        .map_or(Some((Vec::new(), Vec::new())), |attached| {
            attached.ready(places)
        });
    // A line of images or files alone is a line, once they are all
    // kept.
    let Some((images, files)) = ready else {
        world.resource_mut::<Draft>().notice =
            "Something attached is still being sent, waits to be \
             sent, or was not kept."
                .into();
        return false;
    };
    if text.is_empty() && images.is_empty() && files.is_empty() {
        return false;
    }
    if matches!(
        text.split_whitespace().next(),
        Some("/hand" | "/agent" | "/hamlet")
    ) {
        return false;
    }
    let convo = world.resource::<Convo>();
    let Some(to) = convo.to(world) else {
        return false;
    };
    // What is sent is at the end of the chat.
    to_end(world);
    world.trigger(ChiefMessage {
        request: protocol::RequestId::new(),
        to,
        text,
        images,
        files,
    });
    if let Some(mut attached) =
        world.get_resource_mut::<attach::Attached>()
    {
        attached.clear();
    }
    true
}

/// A chief's command about a hamlet, addressed to its mayor.
pub(crate) fn decree(world: &mut World, hamlet: &str, text: String) {
    if let Some(mayor) = agents::mayor_of(world, hamlet) {
        command_to(world, mayor, text);
    }
}

/// A chief's command about the realm, addressed to the Hand.
pub(crate) fn command(world: &mut World, text: String) {
    if let Some(hand) = agents::hand(world) {
        command_to(world, hand, text);
    }
}

/// Sends a command the window made, not one typed in a chat, so its
/// refusal is toasted rather than said only in a chat.
fn command_to(world: &mut World, agent: Entity, text: String) {
    if let Some(to) = agents::id_of(world, agent) {
        let message = ChiefMessage::words(to, text);
        crate::toasts::watch(world, message.request.clone());
        world.trigger(message);
    }
}

/// Gives a decree of the world's as [`decree`] does, or as
/// [`command`] does when it is about no hamlet.
pub(crate) fn decreed(decree: On<Decree>, mut commands: Commands) {
    let Decree { hamlet, text } = decree.event().clone();
    commands.queue(move |world: &mut World| match hamlet {
        Some(hamlet) => self::decree(world, &hamlet, text),
        None => command(world, text),
    });
}

/// Says the world's words for an agent in this chief's chat with it.
fn told(tell: On<Tell>, mut commands: Commands) {
    let Tell { agent, text } = tell.event().clone();
    commands.queue(move |world: &mut World| {
        let Some((to, _)) = convo::id_and_shown(world, agent) else {
            return;
        };
        world.trigger(ChiefMessage::words(to, text));
        world.flush();
    });
}

/// Sends what is typed in the field, as Enter does, or says why a
/// command is not. The field is cleared once its line is sent.
pub(crate) fn send_typed(world: &mut World) {
    let Some(plain) = field::typed(world) else {
        return;
    };
    world.resource_mut::<Draft>().focus = true;
    // Commands are not typed here: each has a button or a menu.
    if plain.text.trim_start().starts_with('/') {
        world.resource_mut::<Draft>().notice =
            "Commands are not typed here. Use the menus, or \
             right-click an agent or a hamlet."
                .into();
        return;
    }
    if without_model(world) {
        // Kept in the field, to send once a model is set up.
        world.resource_mut::<Draft>().notice =
            "No model is set up yet. Set one up to start.".into();
        return;
    }
    world.resource_mut::<Draft>().notice.clear();
    // The words are sent trimmed, and a reply puts a line before
    // them, so each place moves with the words it is in.
    let lead = plain.text.len() - plain.text.trim_start().len();
    let words = plain.text.trim().to_string();
    let (text, shift) = reply::attach(world, words.clone());
    let places = plain
        .atoms
        .iter()
        .filter_map(|atom| {
            let key = atom.key.parse::<u64>().ok()?;
            let at = atom.at.saturating_sub(lead);
            Some((key, (at.min(words.len()) + shift) as u64))
        })
        .collect::<attach::Places>();
    let empty = text.is_empty() && places.is_empty();
    let sent = send(world, text, &places);
    if sent && shift > 0 {
        world.resource_mut::<reply::Replying>().0 = None;
    }
    if sent || empty {
        field::with_field(world, |mut field| field.clear());
    }
}

/// Whether the agent the chat is with runs no model, as before one
/// is set up: nothing can answer it, so nothing is sent to it.
pub(crate) fn without_model(world: &World) -> bool {
    world.get_resource::<Convo>().is_some_and(|convo| {
        world
            .get::<agents::Account>(convo.agent)
            .is_some_and(|account| account.model.is_empty())
    })
}

/// The furthest the chat can be scrolled, in pixels.
pub(crate) fn furthest(node: &ComputedNode) -> f32 {
    let seen = node.size().y - node.scrollbar_size.y;
    ((node.content_size().y - seen) * node.inverse_scale_factor())
        .max(0.0)
}

/// True while the chat is shown away from its end, which is when the
/// way back to it is offered.
#[derive(Resource, Default)]
pub struct AwayFromEnd(pub bool);

/// Whether the chat is to be taken to its end.
#[derive(Resource, Default)]
pub struct ToEnd(bool);

/// Takes the chat to its end.
fn to_end(world: &mut World) {
    if let Some(mut to_end) = world.get_resource_mut::<ToEnd>() {
        to_end.0 = true;
    }
}

/// Takes the chat to its end when asked. The list then keeps it there
/// as it grows, until the chief scrolls up to read.
pub fn keep_end(
    mut to_end: ResMut<ToEnd>,
    mut away: ResMut<AwayFromEnd>,
    mut lists: Query<
        (
            &VirtualList,
            &ComputedNode,
            &mut ScrollPosition,
            &mut ScrollGoal,
        ),
        With<ChatScroll>,
    >,
) {
    let jump = to_end.0;
    if jump {
        to_end.0 = false;
    }
    for (list, node, mut at, mut goal) in &mut lists {
        if jump {
            // The wheel's goal elsewhere means nothing at the end.
            *goal = ScrollGoal::default();
            at.y = furthest(node);
        }
        let now = !jump && !list.is_following();
        if away.0 != now {
            away.0 = now;
        }
    }
}

/// The room between the buttons of the composer's toolbar, in the
/// theme's gaps.
pub(crate) const TOOL_GAP: f32 = 0.25;

/// A round button of the icon `which` alone in the composer's
/// toolbar, with `tip` as its tooltip, that runs `act`.
pub(crate) fn tool_button(
    which: Glyph,
    tip: &'static str,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    with_cx(move |cx| {
        let side = cx.theme().space.row;
        button(glyph(which, ICON))
            .width(px(side))
            .height(px(side))
            .padding(UiRect::all(px(0.0)))
            .radius(side / 2.0)
            .variant(Variant::Text)
            .tip(tip)
            .on_activate(act)
    })
    .boxed()
}

/// The one place to type, under the conversation it adds to: the
/// field and `tools` under it in one rounded card.
pub fn message_bar(tools: impl View + 'static) -> impl View {
    with_cx(move |cx| {
        let space = &cx.theme().space;
        // The card is soft and its controls round, so the room
        // between them is what keeps their corners concentric.
        let inset = space.soft - space.round;
        message_bar_in(space.gap, inset, tools)
    })
}

/// The bar with `gap` between its parts and its field `inset` inside
/// the card.
fn message_bar_in(
    gap: f32,
    inset: f32,
    tools: impl View + 'static,
) -> impl View {
    let hint = derived(|world: &World| {
        if without_model(world) {
            return "Set up a model to start".to_string();
        }
        match world.get_resource::<Convo>().map(|c| &c.with) {
            Some(Some(name)) => format!("Message {name}"),
            Some(None) => "Message the Hand".to_string(),
            None => "Message".to_string(),
        }
    })
    .when(every_frame());
    // Hamlet's own hint over the field, as the search fields have.
    let field = row((hinted(
        rich_field()
            .enter(Enter::Submit)
            .on_submit(|world, _| send_typed(world))
            .on_change(|world, pieces| {
                attach::reconcile(world, &pieces);
                field::chip_references(world, &pieces);
                field::keep_draft(world, &pieces);
            })
            .max_lines(MOST_LINES)
            .grow(1.0)
            // The room above and below one line, kept above the first
            // line and below the last of many.
            .padding(UiRect {
                top: px(LINE_ROOM),
                bottom: px(LINE_ROOM),
                ..UiRect::horizontal(px(FIELD_SIDE))
            })
            .rules(in_card)
            // The card around it shows the focus, not a glow behind
            // its first line.
            .tagged(Variant::Subtle)
            .tagged(ChatField),
        hint,
    ),))
    .grow(1.0)
    .min_width(px(FIELD_LEAST))
    .overflow(Overflow::clip());
    let card = column((
        commands::list(),
        reply::preview(),
        attach::indicator(),
        attach::holds(),
        field,
        tools,
    ))
    .gap(gap / 2.0)
    .padding(UiRect::all(px(inset)))
    .rules(card_look)
    .tagged(crate::world_menu::Hook::Composer);
    column((
        held::line(),
        card,
        // An empty label has no height. Keep one line so a
        // notice does not lift the field above it.
        column((note(resource::<Draft, _>(|draft| {
            draft.notice.clone()
        })),))
        .min_height(px(15.0)),
    ))
    .gap(gap / 2.0)
    .tagged(FileDrop)
}

/// The composer's card: a text field's look, rounder, its frame
/// brightening while anything in it has the focus.
fn card_look(cx: &mut Cx) {
    style::<HamletTheme>()
        .with(text_input)
        .frame(|f, t| f.radius(t.space.soft))
        .apply(cx);
}

/// The field inside the card, which draws no frame of its own.
fn in_card(cx: &mut Cx) {
    let bare = |f: Frame, _: &HamletTheme| {
        f.fill(Color::NONE).border_color(Color::NONE)
    };
    style::<HamletTheme>()
        .frame(bare)
        .hovered(|s| s.frame(bare))
        .when::<bevy_fynix::Focused>(|s| s.frame(bare))
        .apply(cx);
}

#[cfg(test)]
mod tests {
    use bevy::ecs::hierarchy::ChildOf;
    use bevy::ecs::message::Messages;
    use bevy::input::keyboard::Key;
    use bevy::input_focus::{FocusCause, InputFocus};
    use bevy_fynix::views::RichField;
    use hamlet_client::outbox::Outbox;
    use hamlet_realm::sound::{Cue, Sound};

    use super::*;

    fn sounding_chat() -> App {
        let mut app = crate::testing::panel_app();
        app.add_message::<Cue>()
            .init_resource::<crate::menu::OpenMenu>()
            .init_resource::<crate::world_menu::Open>()
            .init_resource::<bevy_fynix::dock::DockDrag>()
            .init_resource::<crate::hub::Showing>()
            .init_resource::<crate::hover::Card>()
            .insert_resource(crate::reference::links())
            .add_plugins((plugin, crate::sounds::plugin));
        bevy_fynix::mount::<HamletTheme>(
            app.world_mut(),
            message_bar(bevy_fynix::views::frame()),
        );
        app.update();
        app
    }

    /// Puts `text` in the field of the chat and gives it the focus.
    fn type_in(app: &mut App, text: &str) -> Entity {
        let world = app.world_mut();
        let input = world
            .query_filtered::<Entity, With<RichField>>()
            .single(world)
            .unwrap();
        world
            .resource_mut::<InputFocus>()
            .set(input, FocusCause::Pressed);
        world
            .get_mut::<RichField>(input)
            .unwrap()
            .set_content([Piece::Text(text.into())]);
        input
    }

    fn typed(app: &App, input: Entity) -> String {
        app.world().get::<RichField>(input).unwrap().plain().text
    }

    #[test]
    fn each_chat_keeps_its_draft_and_its_attachments() {
        use hamlet_protocol::client::{AttachedImage, ImageId};

        use crate::testing::{seat_ada, seat_hand};
        let mut app = sounding_chat();
        app.init_resource::<crate::pictures::Pictures>();
        seat_hand(&mut app);
        app.update();
        let image = ImageId::try_from("a".repeat(64)).unwrap();
        let pieces = app
            .world_mut()
            .resource_mut::<attach::Attached>()
            .give_back(
                &[AttachedImage {
                    at: Some(5),
                    id: image.clone(),
                }],
                &[],
                "Which one",
                0,
            );
        field::set(app.world_mut(), pieces);
        app.update();
        app.update();
        let field = |app: &mut App| {
            let world = app.world_mut();
            world
                .query_filtered::<&RichField, With<ChatField>>()
                .single(world)
                .unwrap()
                .plain()
        };
        let line = |app: &App| {
            app.world()
                .resource::<attach::Attached>()
                .ready(&attach::Places::new())
                .unwrap()
                .0
        };

        seat_ada(&mut app);
        app.update();
        app.update();
        assert_eq!(
            field(&mut app),
            bevy_fynix::views::Plain::default()
        );
        assert!(
            line(&app).is_empty(),
            "no attachment of another chat"
        );

        seat_hand(&mut app);
        app.update();
        app.update();
        let kept = field(&mut app);
        assert_eq!(kept.text, "Which one");
        assert_eq!(kept.atoms[0].at, 5);
        assert_eq!(line(&app)[0].id, image);
    }

    #[test]
    fn an_address_written_whole_in_the_field_is_a_chip_sent_as_words()
    {
        let mut app = sounding_chat();
        let ada = agents::named(app.world(), "hire#2").unwrap();
        app.world_mut()
            .entity_mut(ada)
            .insert(agents::Shown("Ada".into()));
        let input = type_in(&mut app, "");
        let mut write = |text: &str| {
            app.world_mut()
                .get_mut::<RichField>(input)
                .unwrap()
                .set_content([Piece::Text(text.into())]);
            app.update();
            app.update();
            app.world()
                .get::<RichField>(input)
                .unwrap()
                .pieces()
                .iter()
                .filter_map(|piece| match piece {
                    Piece::Atom(atom) => {
                        Some(atom.text().to_string())
                    }
                    Piece::Text(_) => None,
                })
                .collect::<Vec<_>>()
        };
        // Typed a letter at a time, it waits for what follows it.
        assert!(write("On the other hand, ask hire#").is_empty());
        assert!(write("On the other hand, ask hire#2").is_empty());
        assert_eq!(
            write("On the other hand, ask hire#2 "),
            ["hire#2"]
        );
        // A paste of the copied form, with a name that is not its own
        // left as words.
        write("");
        assert_eq!(
            write("Ask hire#2 (Ada) or hire#2 (Bo)"),
            ["hire#2 (Ada)"]
        );

        send_typed(app.world_mut());
        let say = app
            .world()
            .resource::<Outbox>()
            .iter()
            .find_map(|message| match message {
                protocol::ClientMessage::Say(say) => {
                    Some(say.clone())
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(say.text, "Ask hire#2 (Ada) or hire#2 (Bo)");
    }

    #[test]
    fn a_line_is_sent_with_its_attachments_where_their_chips_are() {
        use hamlet_protocol::client::{AttachedImage, ImageId};

        let mut app = sounding_chat();
        let image = ImageId::try_from("a".repeat(64)).unwrap();
        // A chip after "  Which", in words typed with white space
        // around them, and a reply that puts a line before them.
        let pieces = app
            .world_mut()
            .resource_mut::<attach::Attached>()
            .give_back(
                &[AttachedImage {
                    at: Some(7),
                    id: image.clone(),
                }],
                &[],
                "  Which one is it  ",
                0,
            );
        field::set(app.world_mut(), pieces);
        let chat = app.world().resource::<Convo>().chat;
        app.world_mut().resource_mut::<reply::Replying>().0 = Some((
            chat,
            reply::Answers {
                by: "Ada".into(),
                text: "Pear or plum?".into(),
            },
        ));
        send_typed(app.world_mut());
        let say = app
            .world()
            .resource::<Outbox>()
            .iter()
            .find_map(|message| match message {
                protocol::ClientMessage::Say(say) => {
                    Some(say.clone())
                }
                _ => None,
            })
            .unwrap();
        let words = "Which one is it";
        assert!(say.text.ends_with(&format!("\n\n{words}")));
        let at = say.images[0].at.unwrap() as usize;
        assert_eq!(&say.text[at..], " one is it");
        assert_eq!(say.images[0].id, image);
        // The line is sent, and leaves no words and no chip in the
        // field, nor an attachment for the next line.
        let world = app.world_mut();
        let field = world
            .query_filtered::<&RichField, With<ChatField>>()
            .single(world)
            .unwrap();
        assert!(field.is_empty());
        let attached = world.resource::<attach::Attached>();
        assert_eq!(
            attached.ready(&attach::Places::new()),
            Some((Vec::new(), Vec::new()))
        );
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

        use crate::testing::panel_app;

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
        let chat = app.world().resource::<Convo>().chat;
        app.world_mut().entity_mut(chat).insert(
            Transcript::from_items(vec![Item {
                id: "message/é:7".into(),
                at: 7,
                queued: false,
                entry: Entry::Assistant("Paper or digital?".into()),
            }]),
        );
        app.update();
        bevy_fynix::mount::<HamletTheme>(
            app.world_mut(),
            column((
                transcript(),
                message_bar(bevy_fynix::views::frame()),
            )),
        );
        app.update();
        let world = app.world_mut();
        let mut mark = world
            .query::<(Entity, &bevy::ui::widget::ImageNode)>()
            .iter(world)
            .find(|(_, image)| image.image == Glyph::Talk.image(ICON))
            .unwrap()
            .0;
        while world.get::<Button>(mark).is_none() {
            mark = world.get::<ChildOf>(mark).unwrap().parent();
        }
        world.trigger(Activate { entity: mark });
        app.update();
        let input = type_in(&mut app, "Digital, please.");
        let world = app.world_mut();
        world.write_message(KeyboardInput {
            key_code: KeyCode::Enter,
            logical_key: Key::Enter,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window,
        });
        app.update();
        let text = app
            .world()
            .resource::<Outbox>()
            .iter()
            .find_map(|message| match message {
                protocol::ClientMessage::Say(say) => {
                    Some(say.text.clone())
                }
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
            chat,
            text,
            images: Vec::new(),
            files: Vec::new(),
        });
        app.update();
        assert_eq!(typed(&app, input), "Digital, please.");
        assert!(
            crate::testing::words(&mut app)
                .iter()
                .any(|text| text.starts_with("Replying to "))
        );
    }

    #[test]
    fn a_focus_asked_for_before_the_field_is_mounted_waits_for_it() {
        let mut app = crate::testing::panel_app();
        app.add_message::<Cue>()
            .init_resource::<crate::menu::OpenMenu>()
            .init_resource::<crate::world_menu::Open>()
            .init_resource::<bevy_fynix::dock::DockDrag>()
            .init_resource::<crate::hub::Showing>()
            .init_resource::<crate::hover::Card>()
            .add_plugins((plugin, crate::sounds::plugin));
        app.update();
        app.world_mut().resource_mut::<Draft>().focus = true;
        app.update();
        assert!(app.world().resource::<Draft>().focus);
        bevy_fynix::mount::<HamletTheme>(
            app.world_mut(),
            message_bar(bevy_fynix::views::frame()),
        );
        app.update();
        app.update();
        let input = app
            .world_mut()
            .query_filtered::<Entity, With<RichField>>()
            .single(app.world())
            .unwrap();
        assert_eq!(
            app.world().resource::<InputFocus>().get(),
            Some(input)
        );
        assert!(!app.world().resource::<Draft>().focus);
    }

    #[test]
    fn an_unaddressed_fault_shows_under_the_field_and_sounds_once() {
        let mut app = sounding_chat();
        heard(&mut app);
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::Fault(protocol::Fault {
                to: None,
                text: "The server could not keep that change.".into(),
            }),
        );
        app.update();
        assert!(
            crate::testing::words(&mut app).iter().any(|text| text
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
        let chat = app.world().resource::<Convo>().chat;
        for (rewind, fault) in
            [(false, false), (true, false), (false, true)]
        {
            if rewind {
                app.world_mut().trigger(Rewind {
                    chat,
                    request: protocol::RequestId::new(),
                    item: "chief:0".into(),
                });
            } else {
                send(
                    app.world_mut(),
                    "Find the key".into(),
                    &attach::Places::new(),
                );
            }
            app.world_mut().flush();
            let id = app
                .world_mut()
                .resource_mut::<Outbox>()
                .drain()
                .find_map(|message| match message {
                    protocol::ClientMessage::Say(say) => Some(say.id),
                    protocol::ClientMessage::Rewind(rewind) => {
                        Some(rewind.id)
                    }
                    _ => None,
                })
                .unwrap();
            heard(&mut app);
            let text = "This conversation cannot do that.".into();
            let message = if fault {
                protocol::ServerMessage::Fault(protocol::Fault {
                    to: Some(id),
                    text,
                })
            } else {
                protocol::ServerMessage::Reply(protocol::Reply {
                    to: id,
                    kind: protocol::ReplyKind::Error,
                    text,
                    upload: None,
                    image: None,
                    images: Vec::new(),
                    file: None,
                    files: Vec::new(),
                })
            };
            hamlet_client::receive(app.world_mut(), message);
            app.update();
            assert!(
                crate::testing::words(&mut app)
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
                protocol::ClientMessage::Check(
                    protocol::Check::Connection { id },
                )
            });
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::Reply(protocol::Reply {
                to: id,
                kind: protocol::ReplyKind::Error,
                text: "Only the settings panel should show this."
                    .into(),
                upload: None,
                image: None,
                images: Vec::new(),
                file: None,
                files: Vec::new(),
            }),
        );
        app.update();
        assert!(
            !crate::testing::words(&mut app).iter().any(|text| text
                == "Only the settings panel should show this.")
        );
        assert_eq!(heard(&mut app), []);
    }

    #[test]
    fn a_resumes_reply_shows_under_the_field() {
        let mut app = sounding_chat();
        let agent = app.world().resource::<Convo>().agent;
        let request = protocol::RequestId::new();
        app.world_mut().trigger(Resume {
            request: request.clone(),
            agent,
        });
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::Reply(protocol::Reply {
                to: request,
                kind: protocol::ReplyKind::Notice,
                text: "Resumed.".into(),
                upload: None,
                image: None,
                images: Vec::new(),
                file: None,
                files: Vec::new(),
            }),
        );
        app.update();
        assert!(
            crate::testing::words(&mut app)
                .iter()
                .any(|text| text == "Resumed.")
        );
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
        for command in ["/compact", "/agent ada", "/hamlet home"] {
            type_in(&mut app, command);
            app.update();
            app.world_mut().write_message(KeyboardInput {
                key_code: KeyCode::Enter,
                logical_key: Key::Enter,
                state: ButtonState::Pressed,
                text: None,
                repeat: false,
                window,
            });
            app.update();
            assert!(crate::testing::words(&mut app).iter().any(
                |text| text
                    == "Commands are not typed here. Use the menus, \
                        or right-click an agent or a hamlet."
            ));
        }
        let places = attach::Places::new();
        send(app.world_mut(), "/agent ada".into(), &places);
        send(app.world_mut(), "/hamlet home".into(), &places);
        assert!(!app.world().resource::<Outbox>().iter().any(
            |message| matches!(
                message,
                protocol::ClientMessage::Say(_)
            )
        ));
    }

    fn paging_pages() -> Vec<protocol::Transcript> {
        let mut pages = include_str!(
            "../../../hamlet_protocol/frames/paging.jsonl"
        )
        .lines()
        .filter_map(|line| {
            let frame =
                serde_json::from_str::<serde_json::Value>(line)
                    .unwrap();
            (frame["from"] == "server")
                .then(|| {
                    serde_json::from_value::<protocol::ServerMessage>(
                        frame["message"].clone(),
                    )
                    .unwrap()
                })
                .and_then(|message| match message {
                    protocol::ServerMessage::Transcript(page) => {
                        Some(page)
                    }
                    _ => None,
                })
        })
        .collect::<Vec<_>>();
        // The recorded latest page holds 100 items; the window under
        // test is built for a page of 40, with six older items before
        // it.
        let latest = &mut pages[0];
        let keep = latest.items.len() - 40;
        latest.items.drain(..keep);
        latest.earlier = Some(latest.items[0].id.clone());
        pages[1].items.truncate(6);
        // The fixture state names its chief "chief".
        for item in pages.iter_mut().flat_map(|page| &mut page.items)
        {
            if item.chief.is_some() {
                item.chief = Some("chief".into());
            }
        }
        pages
    }

    #[test]
    fn a_chat_shorter_than_the_viewport_reads_earlier_pages_without_scrolling()
     {
        use hamlet_client::outbox::Outbox;
        let mut app = crate::testing::panel_app();
        crate::testing::seat_hand(&mut app);
        app.add_plugins(plugin);
        app.update();
        let pages = paging_pages();
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::Transcript(pages[0].clone()),
        );
        bevy_fynix::mount::<HamletTheme>(
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
        app.update();
        let rows = |app: &mut App| {
            let world = app.world_mut();
            world
                .query_filtered::<&VirtualList, With<ChatScroll>>()
                .single(world)
                .unwrap()
                .count()
        };
        let before = rows(&mut app);
        let requests = app
            .world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .filter_map(|message| match message {
                protocol::ClientMessage::Open(open)
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
            protocol::ServerMessage::Transcript(page),
        );
        app.update();
        app.update();
        assert!(rows(&mut app) > before);
        assert!(!app.world().resource::<Outbox>().iter().any(|message| matches!(message, protocol::ClientMessage::Open(open) if open.before.is_some())));
    }

    #[test]
    fn scrolling_back_pages_once_and_shows_the_received_page_above() {
        use hamlet_client::outbox::Outbox;

        let mut app = crate::testing::panel_app();
        crate::testing::seat_hand(&mut app);
        app.add_plugins(plugin);
        app.update();
        let pages = paging_pages();
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::Transcript(pages[0].clone()),
        );
        bevy_fynix::mount::<HamletTheme>(
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
            size: bevy::math::Vec2::new(300.0, 200.0),
            content_size: bevy::math::Vec2::new(300.0, 1000.0),
            inverse_scale_factor: 1.0,
            ..ComputedNode::default()
        });
        app.update();
        app.world_mut()
            .get_mut::<ScrollPosition>(scroll)
            .unwrap()
            .y = 0.0;
        app.update();
        app.update();
        let requests = app
            .world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .filter_map(|message| match message {
                protocol::ClientMessage::Open(open)
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
                protocol::ClientMessage::Open(open)
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
            protocol::ServerMessage::Transcript(latest),
        );
        app.update();
        let requests = app
            .world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .filter_map(|message| match message {
                protocol::ClientMessage::Open(open)
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
            protocol::ServerMessage::Transcript(page),
        );
        app.update();
        app.update();
        // The rows read stay in view, below the page put above them.
        assert!(
            app.world().get::<ScrollPosition>(scroll).unwrap().y
                > 0.0
        );
    }

    #[test]
    fn another_agents_notification_is_a_row_of_its_own_and_a_queued_line_waits()
     {
        use hamlet_client::chats::{Entry, Item, Transcript};

        use crate::testing::{panel_app, words_and_spans};
        let mut app = panel_app();
        app.add_plugins(plugin);
        let chat = app.world().resource::<Convo>().chat;
        let chief = |id: &str, queued| Item {
            id: id.into(),
            at: 0,
            queued,
            entry: Entry::Chief {
                name: "chief".into(),
                text: format!("Line {id}"),
                images: Vec::new(),
                files: Vec::new(),
            },
        };
        app.world_mut().entity_mut(chat).insert(
            Transcript::from_items(vec![
                chief("chief:0", false),
                Item {
                    id: "notification:0".into(),
                    at: 0,
                    queued: false,
                    entry: Entry::Agent {
                        name: "hire#3 (Pip)".into(),
                        from: None,
                        message: Some(
                            hamlet_protocol::client::ItemMessage::Notification,
                        ),
                        text: "The key is by the mill.".into(),
                    },
                },
                chief("queued:chief:1", true),
            ]),
        );
        app.update();
        bevy_fynix::mount::<HamletTheme>(
            app.world_mut(),
            transcript(),
        );
        app.update();
        // Shown without opening the steps of a turn.
        let shown = words_and_spans(&mut app).join("\n");
        assert!(shown.contains("The key is by the mill."), "{shown}");
        assert!(shown.contains("hire#3 (Pip)"), "{shown}");
        // Under its bubble, where it cannot stand over the words.
        let world = app.world_mut();
        let queued = world
            .query::<(Entity, &bevy::ui::widget::Text)>()
            .iter(world)
            .find(|(_, text)| text.0 == "Queued")
            .map(|(node, _)| node)
            .unwrap();
        let mut parents = world.query::<&ChildOf>();
        assert!(
            !parents
                .query(world)
                .iter_ancestors(queued)
                .any(|up| world.get::<bubble::Bubble>(up).is_some())
        );
    }

    #[test]
    fn a_turns_steps_fold_under_one_header_and_a_failed_call_opens() {
        use hamlet_client::chats::{Entry, Item, Transcript};

        use crate::testing::{panel_app, press, words};
        let mut app = panel_app();
        app.add_plugins(plugin);
        let chat = app.world().resource::<Convo>().chat;
        let tool = |id: &str, exit| Item {
            id: id.into(),
            at: 0,
            queued: false,
            entry: Entry::Tool {
                name: "bash".into(),
                call: hamlet_protocol::client::ToolCall::Command {
                    command: "pwd".into(),
                },
                summary: "pwd".into(),
                exit,
                ms: Some(400),
                failure: None,
                change: None,
                agent: None,
            },
        };
        app.world_mut().entity_mut(chat).insert(
            Transcript::from_items(vec![
                Item {
                    id: "chief:0".into(),
                    at: 0,
                    queued: false,
                    entry: Entry::Chief {
                        name: "chief".into(),
                        text: "Go".into(),
                        images: Vec::new(),
                        files: Vec::new(),
                    },
                },
                tool("done:0", Some(0)),
                tool("failed:0", Some(7)),
                Item {
                    id: "reply:0".into(),
                    at: 0,
                    queued: false,
                    entry: Entry::Assistant("Still working.".into()),
                },
                tool("running:0", None),
                Item {
                    id: "reply:1".into(),
                    at: 0,
                    queued: false,
                    entry: Entry::Assistant("The next step.".into()),
                },
                Item {
                    id: "notice:0".into(),
                    at: 0,
                    queued: false,
                    entry: Entry::Notice("A received notice".into()),
                },
            ]),
        );
        app.update();
        bevy_fynix::mount::<HamletTheme>(
            app.world_mut(),
            transcript(),
        );
        app.update();
        // Every step of the turn is under one header, its reply after
        // it.
        let shown = words(&mut app);
        assert!(
            shown.contains(&" · 1 failed".to_string()),
            "{shown:?}"
        );
        press(&mut app, "Worked");
        app.update();
        // Each call is a line of its request, and the failed one
        // opens to its exit.
        let shown = words(&mut app);
        let count =
            |text: &str| shown.iter().filter(|s| *s == text).count();
        assert_eq!(count("pwd"), 3, "{shown:?}");
        app.world_mut()
            .resource_mut::<OpenTools>()
            .0
            .push((chat, "failed:0".into()));
        app.update();
        app.update();
        assert!(
            words(&mut app).contains(&"Failed (exit 7)".to_string()),
            "the failed call opened"
        );
    }

    #[test]
    fn a_row_rewinds_by_its_string_id_and_returns_the_words_to_the_field()
     {
        use bevy::ui_widgets::{Activate, Button};
        use hamlet_client::chats::{Entry, Item, Transcript};
        use hamlet_client::outbox::Outbox;

        use crate::testing::{panel_app, press};
        let mut app = panel_app();
        app.add_plugins(plugin);
        let chat = app.world().resource::<Convo>().chat;
        app.world_mut().entity_mut(chat).insert(
            Transcript::from_items(vec![Item {
                id: "entry-abc:0".into(),
                at: 1,
                queued: false,
                entry: Entry::Chief {
                    name: "chief".into(),
                    text: "Change these words".into(),
                    images: Vec::new(),
                    files: Vec::new(),
                },
            }]),
        );
        app.update();
        bevy_fynix::mount::<HamletTheme>(
            app.world_mut(),
            column((
                transcript(),
                message_bar(bevy_fynix::views::frame()),
                crate::world_menu::view(),
            )),
        );
        app.update();
        let world = app.world_mut();
        let mut mark = world
            .query::<(Entity, &bevy::ui::widget::ImageNode)>()
            .iter(world)
            .find(|(_, image)| {
                image.image == Glyph::Restore.image(ICON)
            })
            .unwrap()
            .0;
        while world.get::<Button>(mark).is_none() {
            mark = world.get::<ChildOf>(mark).unwrap().parent();
        }
        world.trigger(Activate { entity: mark });
        app.update();
        press(&mut app, "Rewind");
        let request = app
            .world()
            .resource::<Outbox>()
            .iter()
            .find_map(|message| match message {
                protocol::ClientMessage::Rewind(request) => {
                    Some(request.clone())
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(request.item, "entry-abc:0");
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::Reply(protocol::Reply {
                to: request.id,
                kind: protocol::ReplyKind::Rewound,
                text: "Change these words".into(),
                upload: None,
                image: None,
                images: Vec::new(),
                file: None,
                files: Vec::new(),
            }),
        );
        app.update();
        assert!(
            app.world_mut()
                .query::<&RichField>()
                .iter(app.world())
                .any(|field| field.plain().text
                    == "Change these words")
        );
    }

    #[test]
    fn a_starter_of_a_quiet_chat_goes_into_the_field() {
        use hamlet_client::chats::Transcript;

        use crate::testing::{panel_app, press};
        let mut app = panel_app();
        app.add_plugins(plugin);
        let chat = app.world().resource::<Convo>().chat;
        app.world_mut()
            .entity_mut(chat)
            .insert(Transcript::default());
        app.update();
        bevy_fynix::mount::<HamletTheme>(
            app.world_mut(),
            column((
                transcript(),
                message_bar(bevy_fynix::views::frame()),
            )),
        );
        app.update();
        press(&mut app, "Help me work on a project folder.");
        app.update();
        app.update();
        assert!(
            app.world_mut()
                .query::<&RichField>()
                .iter(app.world())
                .any(|field| field.plain().text
                    == "Help me work on a project folder.")
        );
    }

    #[test]
    fn a_chat_keeps_its_end_and_is_shown_again_where_it_was_left() {
        use bevy_fynix::views::VirtualList;
        use hamlet_client::chats::{Entry, Item, Transcript};

        use crate::testing::{panel_app, seat_ada, seat_hand};
        let said = |rows: usize| {
            Transcript::from_items(
                (0..rows)
                    .map(|at| Item {
                        id: format!("message/{at}"),
                        at: at as u64,
                        queued: false,
                        // Each reply answers a line, so none is a
                        // step of the turn of the next.
                        entry: match at % 2 {
                            0 => Entry::Chief {
                                name: "chief".into(),
                                text: format!("Row {at}"),
                                images: Vec::new(),
                                files: Vec::new(),
                            },
                            _ => {
                                Entry::Assistant(format!("Row {at}"))
                            }
                        },
                    })
                    .collect(),
            )
        };
        let mut app = panel_app();
        app.add_plugins(plugin);
        for seat in [seat_ada, seat_hand] {
            seat(&mut app);
            let chat = app.world().resource::<Convo>().chat;
            app.world_mut().entity_mut(chat).insert(said(40));
        }
        app.update();
        // Kept from chat to chat, as the hub keeps it.
        bevy_fynix::mount::<HamletTheme>(
            app.world_mut(),
            transcript(),
        );
        // The window lays nothing out here: the list is given its
        // size each time it is built.
        let settle = |app: &mut App| {
            for _ in 0..6 {
                let world = app.world_mut();
                let list = world
                    .query_filtered::<Entity, With<ChatScroll>>()
                    .single(world)
                    .unwrap();
                if world.get::<ComputedNode>(list).unwrap().size.y
                    == 0.0
                {
                    world.entity_mut(list).insert(ComputedNode {
                        size: bevy::math::Vec2::new(300.0, 200.0),
                        content_size: bevy::math::Vec2::new(
                            300.0, 2400.0,
                        ),
                        inverse_scale_factor: 1.0,
                        ..ComputedNode::default()
                    });
                }
                app.update();
            }
            let world = app.world_mut();
            world
                .query_filtered::<(Entity, &VirtualList), With<ChatScroll>>()
                .single(world)
                .map(|(list, state)| {
                    (list, state.is_following(), state.first_visible())
                })
                .unwrap()
        };

        let (_, following, _) = settle(&mut app);
        assert!(following, "a chat opens at its end");
        let chat = app.world().resource::<Convo>().chat;
        app.world_mut().entity_mut(chat).insert(said(41));
        let (list, following, _) = settle(&mut app);
        assert!(following, "a row that comes keeps it at its end");

        app.world_mut().get_mut::<ScrollPosition>(list).unwrap().y =
            300.0;
        let (_, following, left_at) = settle(&mut app);
        assert!(!following);
        seat_ada(&mut app);
        let (_, following, _) = settle(&mut app);
        assert!(following, "another chat opens at its end");
        seat_hand(&mut app);
        // There in the update it is chosen in, not one after.
        app.update();
        let world = app.world_mut();
        let (_, shown) = world
            .query_filtered::<(Entity, &VirtualList), With<ChatScroll>>()
            .single(world)
            .unwrap();
        assert_eq!(shown.first_visible(), left_at);
        let (_, following, first) = settle(&mut app);
        assert!(!following);
        assert_eq!(first, left_at);
    }

    #[test]
    fn a_running_turns_header_stays_built_as_its_steps_come() {
        use bevy::ecs::hierarchy::Children;
        use bevy::text::TextSpan;
        use hamlet_client::chats::Chat;
        use hamlet_ui::widgets::shimmer::Shimmer;
        use protocol::TranscriptOperation as Op;

        let mut app = crate::testing::panel_app();
        app.add_plugins(plugin);
        let chat = app.world().resource::<Convo>().chat;
        let reference =
            app.world().get::<Chat>(chat).unwrap().0.clone();
        let item = |id: &str, kind, text: &str| protocol::Item {
            id: id.into(),
            at: 1_000,
            kind,
            message: None,
            text: text.into(),
            chief: None,
            author: None,
            event: None,
            from: None,
            queued: false,
            rewindable: None,
            tool: None,
            images: Vec::new(),
            files: Vec::new(),
        };
        let tool = |id: &str| protocol::Item {
            tool: Some(protocol::ItemTool {
                agent: None,
                call: protocol::ToolCall::Command {
                    command: "pwd".into(),
                },
                change: None,
                exit: None,
                failure: None,
                ms: None,
                name: "bash".into(),
                summary: "pwd".into(),
            }),
            ..item(id, protocol::ItemKind::Tool, "")
        };
        let add = |item, replaces: Option<&str>| Op::Add {
            item,
            replaces: replaces.map(str::to_string),
        };
        let working = |working| Op::Status {
            held: false,
            last: None,
            pending: u64::from(working),
            working,
        };
        let send = |app: &mut App, operations| {
            hamlet_client::receive(
                app.world_mut(),
                protocol::ServerMessage::Turn(protocol::Turn {
                    chat: reference.clone(),
                    operations,
                }),
            );
            app.update();
        };
        // The working row of the header of the turn that runs.
        let header = |app: &mut App| {
            let world = app.world_mut();
            let words = world
                .query_filtered::<(Entity, &Children), With<Shimmer>>(
                )
                .iter(world)
                .map(|(word, letters)| {
                    let said = letters
                        .iter()
                        .filter_map(|l| world.get::<TextSpan>(*l))
                        .map(|span| span.0.as_str())
                        .collect::<String>();
                    (word, said)
                })
                .collect::<Vec<_>>();
            let word = words
                .iter()
                .find(|(_, said)| {
                    said == "Working" || said == "Thinking"
                })
                .map(|(word, _)| *word)?;
            let up = |at| world.get::<ChildOf>(at).unwrap().parent();
            Some(up(up(word)))
        };

        let mut done = tool("done:1");
        done.tool.as_mut().unwrap().exit = Some(0);
        send(
            &mut app,
            vec![
                add(
                    item("chief:0", protocol::ItemKind::User, "Go"),
                    None,
                ),
                add(done, None),
                add(
                    item(
                        "reply:0",
                        protocol::ItemKind::Assistant,
                        "Done.",
                    ),
                    None,
                ),
            ],
        );
        bevy_fynix::mount::<HamletTheme>(
            app.world_mut(),
            transcript(),
        );
        app.update();
        let mut queued =
            item("queued:1", protocol::ItemKind::User, "More");
        queued.queued = true;
        send(&mut app, vec![add(queued, None)]);
        // Seen to begin before its line is taken up, the answer does
        // not open the turn that ended.
        send(&mut app, vec![working(true)]);
        assert_eq!(header(&mut app), None);

        let chief = item("chief:1", protocol::ItemKind::User, "More");
        send(&mut app, vec![add(chief, Some("queued:1"))]);
        let shown = header(&mut app).expect("the turn's header");
        let steps = [
            vec![add(
                item(
                    "live:0",
                    protocol::ItemKind::Assistant,
                    "Let me",
                ),
                None,
            )],
            vec![
                add(
                    item(
                        "said:1",
                        protocol::ItemKind::Assistant,
                        "Let me",
                    ),
                    Some("live:0"),
                ),
                add(tool("tool:1"), None),
            ],
            vec![add(
                item(
                    "summary:1",
                    protocol::ItemKind::Summary,
                    "Before",
                ),
                None,
            )],
            vec![Op::Close {
                agent: None,
                change: None,
                exit: 0,
                failure: None,
                item: "tool:1".into(),
                ms: Some(400),
            }],
            vec![add(
                item(
                    "thought:1",
                    protocol::ItemKind::Reasoning,
                    "Hmm",
                ),
                None,
            )],
            vec![add(tool("tool:2"), None)],
        ];
        for operations in steps {
            send(&mut app, operations);
            assert_eq!(header(&mut app), Some(shown));
        }
        send(
            &mut app,
            vec![
                add(
                    item(
                        "said:2",
                        protocol::ItemKind::Assistant,
                        "Done.",
                    ),
                    None,
                ),
                working(false),
            ],
        );
        assert_eq!(header(&mut app), None);
        assert!(
            crate::testing::words(&mut app)
                .iter()
                .any(|w| w.starts_with("Worked"))
        );
    }
}
