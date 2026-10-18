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

use bevy::app::{App, PostUpdate, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
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
use hamlet_core::agents;
use hamlet_core::session::{
    Entry, Reply, SessionKey, SessionOutput, Transcript, UserMessage,
};
use hamlet_realm::orders::{Decree, Tell};
use hamlet_rules::chief::{self, Target};
use hamlet_rules::hamlet::HOME;

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
use crate::me::{self, Me, talk_to, talk_to_mayor_of};

/// The least width of the field a chief types in.
const FIELD_LEAST: f32 = 150.0;

pub fn plugin(app: &mut App) {
    app.init_resource::<Draft>()
        .init_resource::<ChatSettings>()
        .init_resource::<Changes>()
        .init_resource::<Unfolded>()
        .init_resource::<Lines>()
        .init_resource::<Reach>()
        .add_observer(keep_typed)
        .add_observer(decreed)
        .add_observer(told)
        .add_plugins((rewind::plugin, reply::plugin))
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
    // `/hamlet <name>` and `/agent <name>` change who the chat is
    // with.
    match chief::switch(&text) {
        Some(Target::Mayor(hamlet)) => {
            return talk_to_mayor_of(world, &hamlet);
        }
        Some(Target::Agent(name)) => {
            if let Some(agent) = agents::named(world, &name) {
                talk_to(world, agent);
            }
            return;
        }
        None => {}
    }
    let me = world.resource::<Me>();
    let (key, user) = (me.key.clone(), me.user.clone());
    world.trigger(UserMessage { key, user, text });
}

/// The conversation kept under `key`.
fn kept(world: &mut World, key: &SessionKey) -> Option<Entity> {
    world
        .query::<(Entity, &SessionKey)>()
        .iter(world)
        .find(|(_, kept)| *kept == key)
        .map(|(session, _)| session)
}

/// The notices and errors in `session` after its first `skip`
/// entries.
fn answers(
    world: &World,
    session: Entity,
    skip: usize,
) -> Vec<String> {
    world
        .get::<Transcript>(session)
        .into_iter()
        .flat_map(|said| said.items().iter().skip(skip))
        .filter_map(|item| match &item.entry {
            Entry::Notice(text) | Entry::Error(text) => {
                Some(text.clone())
            }
            _ => None,
        })
        .collect()
}

fn entries(world: &World, session: Option<Entity>) -> usize {
    session
        .and_then(|session| world.get::<Transcript>(session))
        .map_or(0, |said| said.items().len())
}

/// Gives a chief's command to the hamlet `hamlet`: in the open chat
/// when it is with that hamlet, and with that hamlet's mayor
/// otherwise. The answer is shown in the open chat either way.
pub(crate) fn decree(world: &mut World, hamlet: &str, text: String) {
    let me = world.resource::<Me>();
    let here = agents::hamlet_of(world, me.agent).unwrap_or(HOME);
    let (open, user) = (me.session, me.user.clone());
    let key = if here == hamlet {
        me.key.clone()
    } else {
        SessionKey::with_mayor("app", &user, hamlet)
    };
    let elsewhere = key != me.key;
    let there = kept(world, &key);
    let before = (entries(world, there), entries(world, Some(open)));
    world.trigger(UserMessage {
        key: key.clone(),
        user,
        text,
    });
    world.flush();
    if !elsewhere {
        return;
    }
    let Some(there) = kept(world, &key) else {
        return;
    };
    // The core tells every chat of some answers itself.
    let told = answers(world, open, before.1);
    let answers = answers(world, there, before.0);
    if let Some(mut said) = world.get_mut::<Transcript>(open) {
        for answer in answers {
            if !told.contains(&answer) {
                said.apply(&Reply::Notice(answer));
            }
        }
    }
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
        let Some((key, _)) = me::address(world, agent) else {
            return;
        };
        let user = world.resource::<Me>().user.clone();
        world.trigger(UserMessage { key, user, text });
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
        let me = world.resource::<Me>();
        let (session, key) = (me.session, me.key.clone());
        world.trigger(SessionOutput {
            session,
            key,
            reply: Reply::Notice(
                "Commands are not typed here. Use the menus, or \
                 right-click an agent or a hamlet."
                    .into(),
            ),
        });
    } else {
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
    ))
    .gap(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::me::MeOf;

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
