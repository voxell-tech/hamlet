//! The chat with an agent and the field a message is typed in.

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

use bevy::app::{App, Last, PostUpdate, PreUpdate, Update};
use bevy::color::Color;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::Event;
use bevy::ecs::lifecycle::Remove;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Query, ResMut};
use bevy::ecs::world::World;
use bevy::ui::{ComputedNode, Overflow, ScrollPosition, UiRect, px};
use bevy_fynix::scroll::ScrollGoal;
use bevy_fynix::views::{
    BehaviorExt, Enter, Frame, FrameProps, Piece, VirtualList,
    button, column, rich_field, row,
};
use bevy_fynix::{
    ScopedExt, ViewExt, component, derived, every_frame, style,
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
pub use self::lines::{
    Lines, MergedLines, Unfolded, ask_earlier, break_long_words,
    fold_lines, fold_merged, transcript,
};
pub(crate) use self::lines::{close_merged, keep_merged};
use self::lines::{remember_place, restore_place};
pub use self::rewind::{Rewinds, ask as ask_rewind, rewind_to};
use crate::{PanelSystems, convo};

/// The room at the sides of the field, as the theme gives a field.
const FIELD_SIDE: f32 = 8.0;

/// The room above and below a single line of the field.
const LINE_ROOM: f32 = 4.5;

/// The least width of the field a chief types in.
const FIELD_LEAST: f32 = 150.0;

/// The most lines the field grows to before it scrolls.
const MOST_LINES: usize = 8;

pub fn plugin(app: &mut App) {
    app.init_resource::<Requests>()
        .init_resource::<ChatSettings>()
        .init_resource::<Changes>()
        .init_resource::<Unfolded>()
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
        .add_observer(forget_hub)
        .init_resource::<OpenTools>()
        // Read by the rewind's fading.
        .init_resource::<crate::world_menu::Open>()
        .add_plugins((
            anchor::plugin,
            attach::plugin,
            rewind::plugin,
            lines::plugin,
            reply::plugin,
        ))
        // Before fynix mounts, so a reply shows in the update it
        // arrives.
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
        .add_systems(PostUpdate, refocus)
        // After the field takes in an edit, late in PostUpdate.
        .add_systems(Last, field::chip_early);
}

/// Gives `hub` its chat's view: its rows, its place and its draft.
pub(crate) fn equip(world: &mut World, hub: Entity) {
    world.entity_mut(hub).insert((
        Draft::default(),
        Lines::default(),
        MergedLines::default(),
        lines::Reading::default(),
        ToEnd::default(),
        AwayFromEnd::default(),
        field::Typed::default(),
        field::Unsent::default(),
        lines::ChatPlaces::default(),
        reply::Replying::default(),
        crate::screens::wait::ChatWaits::default(),
    ));
}

/// The field a chief types in, in a Hub.
#[derive(Component)]
pub struct ChatField(pub Entity);

/// The scrolling node of a Hub's chat.
#[derive(Component)]
pub struct ChatScroll(pub Entity);

/// Whether a Hub's field is to get the focus, and the notice under
/// it.
#[derive(Component, Default)]
pub struct Draft {
    pub(crate) focus: bool,
    notice: String,
}

/// Asks for the focus to go to `hub`'s field.
pub(crate) fn focus(world: &mut World, hub: Entity) {
    if let Some(mut draft) = world.get_mut::<Draft>(hub) {
        draft.focus = true;
    }
}

/// Says `text` under `hub`'s field.
fn tell(world: &mut World, hub: Option<Entity>, text: &str) {
    if let Some(mut draft) =
        hub.and_then(|hub| world.get_mut::<Draft>(hub))
        && draft.notice != text
    {
        draft.notice = text.to_string();
    }
}

/// Forgets what was opened in a chat that is gone.
fn forget_chat(
    gone: On<Remove<Chat>>,
    mut unfolded: ResMut<Unfolded>,
    mut tools: ResMut<OpenTools>,
    mut changes: ResMut<Changes>,
    mut hubs: Query<(&mut field::Unsent, &mut lines::ChatPlaces)>,
    merged: Query<(Entity, &MergedLines)>,
    mut commands: Commands,
) {
    let chat = gone.entity;
    unfolded.0.retain(|(_, of, _)| *of != chat);
    tools.0.retain(|(_, of, _)| *of != chat);
    changes.forget(|(_, of, _)| *of == chat);
    for (mut unsent, mut places) in &mut hubs {
        unsent.forget(chat);
        places.0.remove(&chat);
    }
    for (hub, merged) in &merged {
        if merged.is_in(chat) {
            commands.queue(move |world: &mut World| {
                close_merged(world, hub);
            });
        }
    }
}

/// Forgets the folds a closed Hub had open; a Hub on a hidden desk
/// is kept, and keeps them.
fn forget_hub(
    gone: On<Remove<crate::hub::Hub>>,
    mut unfolded: ResMut<Unfolded>,
    mut tools: ResMut<OpenTools>,
    mut changes: ResMut<Changes>,
) {
    let hub = gone.entity;
    unfolded.0.retain(|(of, ..)| *of != hub);
    tools.0.retain(|(of, ..)| *of != hub);
    changes.forget(|(of, ..)| *of == hub);
}

/// A failure displayed under the chat field.
#[derive(Event)]
pub(crate) struct ChatError;

fn fault(fault: On<ServerFault>, mut commands: Commands) {
    let text = fault.text.clone();
    commands.queue(move |world: &mut World| {
        let hub = crate::hub::active(world);
        tell(world, hub, &text);
    });
    commands.trigger(ChatError);
}

/// Requests whose replies show under a field, with the Hub each was
/// made in, if known.
#[derive(Resource, Default)]
struct Requests(HashMap<protocol::RequestId, (Said, Option<Entity>)>);

impl Requests {
    /// Keeps what of `id`'s reply shows, under the field of the Hub
    /// the chief works in when it comes, unless [`asked_in`] names
    /// the Hub it was made in.
    fn note(&mut self, id: &protocol::RequestId, said: Said) {
        self.0.insert(id.clone(), (said, None));
    }
}

/// Says the reply to the request `id` under `hub`'s field, not that
/// of the Hub the chief has gone to since.
pub(super) fn asked_in(
    world: &mut World,
    id: protocol::RequestId,
    hub: Entity,
) {
    if let Some((_, from)) =
        world.resource_mut::<Requests>().0.get_mut(&id)
    {
        *from = Some(hub);
    }
}

/// What of a request's reply shows.
#[derive(Clone, Copy, PartialEq)]
enum Said {
    Rejection,
    Reply,
}

/// Says the rejection of the request `id` under `hub`'s field.
pub(super) fn rejected(
    world: &mut World,
    hub: Entity,
    id: protocol::RequestId,
) {
    world
        .resource_mut::<Requests>()
        .0
        .insert(id, (Said::Rejection, Some(hub)));
}

/// Says the reply to the request `id` under `hub`'s field.
pub(super) fn replied(
    world: &mut World,
    hub: Entity,
    id: protocol::RequestId,
) {
    world
        .resource_mut::<Requests>()
        .0
        .insert(id, (Said::Reply, Some(hub)));
}

fn remember_said(
    message: On<ChiefMessage>,
    mut requests: ResMut<Requests>,
) {
    requests.note(&message.request, Said::Rejection);
}

fn remember_rewind(
    rewind: On<Rewind>,
    mut requests: ResMut<Requests>,
) {
    requests.note(&rewind.request, Said::Rejection);
}

fn remember_compact(
    compact: On<Compact>,
    mut requests: ResMut<Requests>,
) {
    requests.note(&compact.request, Said::Rejection);
}

fn remember_withdraw(
    withdraw: On<Withdraw>,
    mut requests: ResMut<Requests>,
) {
    requests.note(&withdraw.request, Said::Rejection);
}

fn remember_resume(
    resume: On<Resume>,
    mut requests: ResMut<Requests>,
) {
    requests.note(&resume.request, Said::Reply);
}

fn response(
    response: On<Response>,
    mut requests: ResMut<Requests>,
    mut commands: Commands,
) {
    let (text, rejected) = match &response.message {
        protocol::ServerMessage::Reply(reply) => {
            (&reply.text, reply.kind == protocol::ReplyKind::Error)
        }
        protocol::ServerMessage::Fault(fault) => (&fault.text, true),
        protocol::ServerMessage::Transcript(_) => {
            requests.0.remove(&response.to);
            return;
        }
        _ => return,
    };
    let Some((said, from)) = requests.0.remove(&response.to) else {
        return;
    };
    if rejected || said == Said::Reply {
        let text = text.clone();
        commands.queue(move |world: &mut World| {
            let hub = from
                .filter(|hub| world.get::<Draft>(*hub).is_some())
                .or_else(|| crate::hub::active(world));
            tell(world, hub, &text);
        });
    }
    if rejected {
        commands.trigger(ChatError);
    }
}

/// Puts `words` in `hub`'s field as quoted lines, after what is
/// typed.
pub(crate) fn quote_in_field(
    world: &mut World,
    hub: Entity,
    words: &str,
) {
    if words.trim().is_empty() {
        return;
    }
    let quoted = format!("{}\n", quote::quoting(words));
    field::with_field(world, hub, |mut field| {
        let mut pieces = field.pieces();
        let lead = match pieces.last() {
            None => "",
            Some(Piece::Text(text)) if text.ends_with('\n') => "",
            Some(_) => "\n",
        };
        pieces.push(Piece::Text(format!("{lead}{quoted}")));
        field.set_content(pieces);
    });
    focus(world, hub);
}

/// Sends `text` and its attachments to `hub`'s chat, if it can.
pub(crate) fn send(
    world: &mut World,
    hub: Entity,
    text: String,
    places: &attach::Places,
) -> bool {
    let text = text.trim().to_string();
    let owner = attach::Owner::of(world, hub);
    let ready = world
        .get_resource::<attach::Attached>()
        .map_or(Some((Vec::new(), Vec::new())), |attached| {
            attached.ready(owner, places)
        });
    let Some((images, files)) = ready else {
        tell(
            world,
            Some(hub),
            "Something attached is still being sent, waits to be \
             sent, or was not kept.",
        );
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
    let Some((to, agent)) = convo::of(world, hub)
        .and_then(|c| Some((c.to(world)?, c.agent)))
    else {
        return false;
    };
    to_end(world, hub);
    crate::chief::spoke(world, hub, agent);
    let request = protocol::RequestId::new();
    world.trigger(ChiefMessage {
        request: request.clone(),
        to,
        text,
        images,
        files,
    });
    asked_in(world, request, hub);
    if let Some(mut attached) =
        world.get_resource_mut::<attach::Attached>()
    {
        attached.clear(owner);
    }
    true
}

/// A chief's command about a hamlet, addressed to its lead.
pub(crate) fn decree(world: &mut World, hamlet: &str, text: String) {
    if let Some(lead) = agents::lead_of(world, hamlet) {
        command_to(world, lead, text);
    }
}

/// A chief's command about the realm, addressed to the Hand.
pub(crate) fn command(world: &mut World, text: String) {
    if let Some(hand) = agents::hand(world) {
        command_to(world, hand, text);
    }
}

/// Sends a command the window made, its rejection toasted.
fn command_to(world: &mut World, agent: Entity, text: String) {
    if let Some(to) = agents::id_of(world, agent) {
        let message = ChiefMessage::words(to, text);
        crate::toasts::watch(world, message.request.clone());
        world.trigger(message);
    }
}

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

/// Sends what is typed in `hub`'s field, as Enter does.
pub(crate) fn send_typed(world: &mut World, hub: Entity) {
    let Some(plain) = field::typed(world, hub) else {
        return;
    };
    focus(world, hub);
    if plain.text.trim_start().starts_with('/') {
        tell(
            world,
            Some(hub),
            "Commands are not typed here. Use the menus, or \
             right-click an agent or a hamlet.",
        );
        return;
    }
    if without_model(world, hub) {
        // Kept in the field, to send once a model is set up.
        tell(
            world,
            Some(hub),
            "No model is set up yet. Set one up to start.",
        );
        return;
    }
    tell(world, Some(hub), "");
    // Places move with the trimmed words and the reply's line.
    let lead = plain.text.len() - plain.text.trim_start().len();
    let words = plain.text.trim().to_string();
    let (text, shift) = reply::attach(world, hub, words.clone());
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
    let sent = send(world, hub, text, &places);
    if sent && shift > 0 {
        reply::reply_in(world, hub, None);
    }
    if sent || empty {
        field::with_field(world, hub, |mut field| field.clear());
    }
}

/// Whether the agent `hub`'s chat is with has no model set up.
pub(crate) fn without_model(world: &World, hub: Entity) -> bool {
    convo::of(world, hub)
        .is_some_and(|convo| has_no_model(world, convo.agent))
}

/// Whether `agent` has no model set up.
pub(crate) fn has_no_model(world: &World, agent: Entity) -> bool {
    world
        .get::<agents::Account>(agent)
        .is_some_and(|account| account.model.is_empty())
}

/// The furthest the chat can be scrolled, in pixels.
pub(crate) fn furthest(node: &ComputedNode) -> f32 {
    let seen = node.size().y - node.scrollbar_size.y;
    ((node.content_size().y - seen) * node.inverse_scale_factor())
        .max(0.0)
}

/// Whether a Hub's chat is shown away from its end.
#[derive(Component, Default)]
pub struct AwayFromEnd(pub bool);

/// Whether a Hub's chat is to be taken to its end.
#[derive(Component, Default)]
pub struct ToEnd(bool);

fn to_end(world: &mut World, hub: Entity) {
    if let Some(mut to_end) = world.get_mut::<ToEnd>(hub) {
        to_end.0 = true;
    }
}

/// Takes each Hub's chat to its end when asked.
pub fn keep_end(
    mut hubs: Query<(&mut ToEnd, &mut AwayFromEnd)>,
    mut lists: Query<(
        &ChatScroll,
        &VirtualList,
        &ComputedNode,
        &mut ScrollPosition,
        &mut ScrollGoal,
    )>,
) {
    for (scroll, list, node, mut at, mut goal) in &mut lists {
        let Ok((mut to_end, mut away)) = hubs.get_mut(scroll.0)
        else {
            continue;
        };
        let jump = to_end.0;
        if jump {
            to_end.0 = false;
        }
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

/// The gap between the composer's buttons, in theme gaps.
pub(crate) const TOOL_GAP: f32 = 0.25;

/// A round icon button of the composer's toolbar.
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

/// `hub`'s field and `tools` under it in one rounded card.
pub fn message_bar(
    hub: Entity,
    tools: impl View + 'static,
) -> impl View {
    with_cx(move |cx| {
        let space = &cx.theme().space;
        // Keeps the corners of the card and its controls concentric.
        let inset = space.soft - space.round;
        message_bar_in(hub, space.gap, inset, tools)
    })
}

fn message_bar_in(
    hub: Entity,
    gap: f32,
    inset: f32,
    tools: impl View + 'static,
) -> impl View {
    let hint = derived(move |world: &World| {
        if without_model(world, hub) {
            return "Set up a model to start".to_string();
        }
        match convo::of(world, hub).map(|c| &c.with) {
            Some(Some(name)) => format!("Message {name}"),
            Some(None) => "Message the Hand".to_string(),
            None => "Message".to_string(),
        }
    })
    .when(every_frame());
    let field = row((hinted(
        rich_field()
            .enter(Enter::Submit)
            .on_submit(move |world, _| send_typed(world, hub))
            .on_change(move |world, pieces| {
                crate::hub::activate(world, hub);
                attach::reconcile(world, hub, &pieces);
                field::chip_references(world, hub, &pieces);
                field::keep_draft(world, hub, &pieces);
            })
            .max_lines(MOST_LINES)
            .grow(1.0)
            .padding(UiRect {
                top: px(LINE_ROOM),
                bottom: px(LINE_ROOM),
                ..UiRect::horizontal(px(FIELD_SIDE))
            })
            .rules(in_card)
            // The card shows the focus.
            .tagged(Variant::Subtle)
            .tagged(ChatField(hub)),
        hint,
    ),))
    .grow(1.0)
    .min_width(px(FIELD_LEAST))
    .overflow(Overflow::clip());
    let card = column((
        commands::list(hub),
        reply::preview(hub),
        attach::indicator(hub),
        attach::holds(hub),
        field,
        tools,
    ))
    .gap(gap / 2.0)
    .padding(UiRect::all(px(inset)))
    .rules(card_look)
    .tagged(crate::world_menu::Hook::Composer);
    column((
        held::line(hub),
        card,
        // One line kept, so a notice does not lift the field.
        column((note(component::<Draft, _>(hub, |draft| {
            draft
                .map(|draft| draft.notice.clone())
                .unwrap_or_default()
        })),))
        .min_height(px(15.0)),
    ))
    .gap(gap / 2.0)
    .tagged(FileDrop)
}

/// The composer's card, with a text field's look.
fn card_look(cx: &mut Cx) {
    style::<HamletTheme>()
        .with(text_input)
        .frame(|f, t| f.radius(t.space.soft))
        .apply(cx);
}

/// The field inside the card, with no frame of its own.
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
    use bevy::ecs::query::With;
    use bevy::input::keyboard::Key;
    use bevy::input_focus::{FocusCause, InputFocus};
    use bevy_fynix::views::RichField;
    use hamlet_client::outbox::Outbox;
    use hamlet_realm::sound::Cue;

    use super::*;
    use crate::testing::convo;

    /// The first Hub, which the chat tests type and read in.
    fn hub(app: &App) -> Entity {
        crate::hub::following(app.world()).unwrap()
    }

    fn sounding_chat() -> App {
        let mut app = crate::testing::panel_app();
        app.add_message::<Cue>()
            .init_resource::<crate::menu::OpenMenu>()
            .init_resource::<crate::world_menu::Open>()
            .init_resource::<bevy_fynix::dock::DockDrag>()
            .init_resource::<crate::hover::Card>()
            .insert_resource(crate::reference::links())
            .add_plugins((plugin, crate::sounds::plugin));
        let hub = hub(&app);
        bevy_fynix::mount::<HamletTheme>(
            app.world_mut(),
            message_bar(hub, bevy_fynix::views::frame()),
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
    fn a_line_is_sent_with_its_attachments_where_their_chips_are() {
        use hamlet_protocol::client::{AttachedImage, ImageId};

        let mut app = sounding_chat();
        let image = ImageId::try_from("a".repeat(64)).unwrap();
        // A chip after "  Which", in words typed with white space
        // around them, and a reply that puts a line before them.
        let hub = hub(&app);
        let owner = attach::Owner::of(app.world(), hub);
        let pieces = app
            .world_mut()
            .resource_mut::<attach::Attached>()
            .give_back(
                owner,
                &[AttachedImage {
                    at: Some(7),
                    id: image.clone(),
                }],
                &[],
                "  Which one is it  ",
                0,
            );
        field::set(app.world_mut(), hub, pieces);
        let chat = convo(&app).chat;
        let answers = reply::Answers {
            by: "Ada".into(),
            text: "Pear or plum?".into(),
        };
        reply::reply_in(app.world_mut(), hub, Some((chat, answers)));
        send_typed(app.world_mut(), hub);
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
        // Nothing is left for the next line.
        let world = app.world_mut();
        let field = world
            .query_filtered::<&RichField, With<ChatField>>()
            .single(world)
            .unwrap();
        assert!(field.is_empty());
        let attached = world.resource::<attach::Attached>();
        assert_eq!(
            attached.ready(owner, &attach::Places::new()),
            Some((Vec::new(), Vec::new()))
        );
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
        let (hub, chat) = (hub(&app), convo(&app).chat);
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
                transcript(hub),
                message_bar(hub, bevy_fynix::views::frame()),
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
            request: protocol::RequestId::new(),
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
            .init_resource::<crate::hover::Card>()
            .add_plugins((plugin, crate::sounds::plugin));
        app.update();
        let hub = hub(&app);
        focus(app.world_mut(), hub);
        app.update();
        assert!(app.world().get::<Draft>(hub).unwrap().focus);
        bevy_fynix::mount::<HamletTheme>(
            app.world_mut(),
            message_bar(hub, bevy_fynix::views::frame()),
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
        assert!(!app.world().get::<Draft>(hub).unwrap().focus);
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
        let hub = hub(&app);
        send(app.world_mut(), hub, "/agent ada".into(), &places);
        send(app.world_mut(), hub, "/hamlet home".into(), &places);
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
        // A latest page of 40, with six older items before it.
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
        let shown = transcript(hub(&app));
        bevy_fynix::mount::<HamletTheme>(app.world_mut(), shown);
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
        let shown = transcript(hub(&app));
        bevy_fynix::mount::<HamletTheme>(app.world_mut(), shown);
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
            let chat = convo(&app).chat;
            app.world_mut().entity_mut(chat).insert(said(40));
        }
        app.update();
        // Kept from chat to chat, as the hub keeps it.
        let shown = transcript(hub(&app));
        bevy_fynix::mount::<HamletTheme>(app.world_mut(), shown);
        // Nothing is laid out here; the list is sized as it is built.
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
        let chat = convo(&app).chat;
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
        let chat = convo(&app).chat;
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
        let shown = transcript(hub(&app));
        bevy_fynix::mount::<HamletTheme>(app.world_mut(), shown);
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
