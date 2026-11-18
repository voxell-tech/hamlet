//! What the field lists while the word at its caret starts with a
//! [`Sign`]: agents after `@`, commands after `/`, skills after `$`
//! and files after `#`. Each kind is a [`Source`] added to the app
//! with [`AddSource::add_source`]. The arrows choose, Enter or Tab
//! picks, and Escape closes the list.

pub(super) mod commands;
mod files;
mod mentions;
mod skills;

use std::ops::Range;
use std::path::PathBuf;

use bevy::app::{App, PreUpdate};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Query};
use bevy::ecs::world::World;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::FocusedInput;
use bevy::ui::{AlignItems, JustifyContent, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    self, Atom, BehaviorExt, FrameProps, Piece, button, column,
    frame, label, row,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, every_frame, keyed};
use hamlet_protocol::signs::{self, Sign};
use hamlet_ui::styles::{bare, small_rules};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::{AnyView, Cx, View};

use super::attach::{self, Incoming};
use super::{ChatField, field};
use crate::PanelSystems;

/// The most entries listed at once.
const MOST: usize = 8;

/// The keys the field leaves to the list while it is open.
const STEERING: [Key; 5] = [
    Key::ArrowUp,
    Key::ArrowDown,
    Key::Enter,
    Key::Tab,
    Key::Escape,
];

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Sources>()
        .add_source::<mentions::Mentions>()
        .add_source::<commands::Commands>()
        .add_source::<skills::Skills>()
        .add_source::<files::Files>()
        .add_plugins(files::plugin)
        .add_observer(steer)
        .add_systems(PreUpdate, follow.in_set(PanelSystems::Bind));
}

/// A kind of thing the field lists after its sign.
pub(super) trait Source: 'static {
    const SIGN: Sign;
    /// What it lists, as the field's hint names it.
    const LISTS: &str;

    /// What matches `rest`, the word after the sign in `hub`'s field,
    /// best first.
    fn entries(world: &World, hub: Entity, rest: &str) -> Vec<Entry>;
}

/// Adds a [`Source`] to an app.
pub(super) trait AddSource {
    /// Adds `S`. Two sources of one sign are a defect.
    fn add_source<S: Source>(&mut self) -> &mut Self;
}

impl AddSource for App {
    fn add_source<S: Source>(&mut self) -> &mut Self {
        let mut sources = self.world_mut().resource_mut::<Sources>();
        assert!(
            sources.find(S::SIGN).is_none(),
            "two sources of {:?}",
            S::SIGN
        );
        sources.0.push(Added {
            sign: S::SIGN,
            lists: S::LISTS,
            entries: S::entries,
        });
        self
    }
}

/// The sources added, each with its sign.
#[derive(Resource, Default)]
struct Sources(Vec<Added>);

struct Added {
    sign: Sign,
    lists: &'static str,
    entries: fn(&World, Entity, &str) -> Vec<Entry>,
}

impl Sources {
    fn find(&self, sign: Sign) -> Option<&Added> {
        self.0.iter().find(|added| added.sign == sign)
    }
}

/// Each sign with what it lists, as in `@ agents, / commands`.
pub(super) fn hint(world: &World) -> String {
    world
        .resource::<Sources>()
        .0
        .iter()
        .map(|added| format!("{} {}", added.sign.char(), added.lists))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A line of the list.
pub(super) struct Entry {
    pub title: String,
    pub detail: String,
    pub pick: Pick,
}

/// What picking an [`Entry`] does with the word it was listed for.
pub(super) enum Pick {
    /// Puts the chip of what `address` names in its place, sent as
    /// `words`.
    Reference { address: String, words: String },
    /// Puts a chip in its place, showing `shown` and sent as `sent`.
    Chip { shown: String, sent: String },
    /// Puts these words in its place.
    Words(String),
    /// Takes it out and attaches this file or folder.
    Attach(PathBuf),
}

/// The word a Hub's list is open for, and what it lists.
#[derive(Component, Default)]
struct Listing {
    /// The sign of the word, and its bytes in the field's text.
    word: Option<(Sign, Range<usize>)>,
    rest: String,
    entries: Vec<Entry>,
    chosen: usize,
    /// The start of a word Escape closed the list for.
    closed: Option<usize>,
    /// Whether the field leaves [`STEERING`] to the list.
    steering: bool,
    /// The number of the next chip, which keys it.
    next: u64,
    /// How many times the Hub's folder was read when it was listed.
    reads: u64,
}

impl Listing {
    fn is_open(&self) -> bool {
        self.word.is_some() && !self.entries.is_empty()
    }
}

/// Follows the word at the caret of each Hub's field, and lists what
/// its sign's source has for it.
fn follow(world: &mut World) {
    let mut fields = world.query::<&ChatField>();
    let mut hubs =
        fields.iter(world).map(|field| field.0).collect::<Vec<_>>();
    hubs.sort();
    hubs.dedup();
    for hub in hubs {
        follow_in(world, hub);
    }
}

/// Follows the word at the caret of `hub`'s field.
fn follow_in(world: &mut World, hub: Entity) {
    let Ok(mut held) = world.get_entity_mut(hub) else {
        return;
    };
    held.entry::<Listing>().or_default();
    let typed = field::with_field(world, hub, |field| {
        field.caret().map(|caret| (field.plain().text, caret))
    })
    .flatten();
    let word = typed.as_ref().and_then(|(text, caret)| {
        let token = signs::at_caret(text, *caret)?;
        Some((token.sign, token.start..*caret, token.rest.to_owned()))
    });
    let Some(listing) = world.get::<Listing>(hub) else {
        return;
    };
    // A folder read anew lists its files again, at the same word too.
    let reads = files::reads(world, hub);
    let same = listing.reads == reads
        && match (&word, &listing.word) {
            (Some((sign, range, rest)), Some((was, range_was))) => {
                sign == was
                    && range == range_was
                    && *rest == listing.rest
            }
            (None, None) => true,
            _ => false,
        };
    if !same {
        let entries = word
            .as_ref()
            .filter(|(_, range, _)| {
                listing.closed != Some(range.start)
            })
            .and_then(|(sign, _, rest)| {
                let find = world.resource::<Sources>().find(*sign)?;
                let mut entries = (find.entries)(world, hub, rest);
                entries.truncate(MOST);
                Some(entries)
            })
            .unwrap_or_default();
        let Some(mut listing) = world.get_mut::<Listing>(hub) else {
            return;
        };
        if word.as_ref().map(|(_, range, _)| range.start)
            != listing.closed
        {
            listing.closed = None;
        }
        listing.chosen = 0;
        listing.entries = entries;
        listing.reads = reads;
        (listing.word, listing.rest) = match word {
            Some((sign, range, rest)) => (Some((sign, range)), rest),
            None => (None, String::new()),
        };
    }
    let Some(mut listing) = world.get_mut::<Listing>(hub) else {
        return;
    };
    let open = listing.is_open();
    if open != listing.steering {
        listing.steering = open;
        field::with_field(world, hub, |mut field| match open {
            true => field.pass_keys(STEERING),
            false => field.pass_keys([]),
        });
    }
}

/// Moves the choice with the arrows, picks it with Enter or Tab, and
/// closes the list with Escape.
fn steer(
    mut key: On<FocusedInput<KeyboardInput>>,
    fields: Query<&ChatField>,
    mut listings: Query<&mut Listing>,
    mut commands: Commands,
) {
    if key.event_target() != key.focused_entity
        || !key.input.state.is_pressed()
    {
        return;
    }
    let Ok(&ChatField(hub)) = fields.get(key.focused_entity) else {
        return;
    };
    let Ok(mut listing) = listings.get_mut(hub) else {
        return;
    };
    if !listing.is_open() {
        return;
    }
    let count = listing.entries.len();
    match key.input.logical_key {
        Key::ArrowDown => {
            listing.chosen = (listing.chosen + 1) % count;
        }
        Key::ArrowUp => {
            listing.chosen = (listing.chosen + count - 1) % count;
        }
        Key::Enter | Key::Tab => {
            let chosen = listing.chosen;
            commands.queue(move |world: &mut World| {
                pick(world, hub, chosen);
            });
        }
        Key::Escape => {
            listing.closed =
                listing.word.as_ref().map(|(_, range)| range.start);
            listing.entries.clear();
        }
        _ => return,
    }
    key.propagate(false);
}

/// Picks the entry `index` for the word `hub`'s list is open for.
fn pick(world: &mut World, hub: Entity, index: usize) {
    let Some(mut listing) = world.get_mut::<Listing>(hub) else {
        return;
    };
    let Some((sign, range)) = listing.word.clone() else {
        return;
    };
    if index >= listing.entries.len() {
        return;
    }
    let entry = listing.entries.swap_remove(index);
    listing.entries.clear();
    let key = format!("{}{}", sign.char(), listing.next);
    listing.next += 1;
    let chip = |world: &mut World, atom| {
        let with = vec![Piece::Atom(atom), Piece::Text(" ".into())];
        field::replace(world, hub, range.clone(), with);
    };
    match entry.pick {
        Pick::Reference { address, words } => {
            if let Some(atom) =
                field::reference(world, hub, &address, &words)
            {
                chip(world, atom);
            }
        }
        Pick::Chip { shown, sent } => {
            chip(world, token_atom(key, sent, shown));
        }
        Pick::Words(words) => {
            field::replace(
                world,
                hub,
                range,
                vec![Piece::Text(words)],
            );
        }
        Pick::Attach(path) => {
            field::replace(world, hub, range, Vec::new());
            attach::attach(world, hub, vec![Incoming::File(path)]);
        }
    }
    // A press on the list took the focus from the field.
    super::focus(world, hub);
}

/// The list over `hub`'s field, while it is open.
pub(super) fn list(hub: Entity) -> impl View {
    let lines = move |world: &World| {
        let Some(listing) = world.get::<Listing>(hub) else {
            return Vec::new();
        };
        listing
            .entries
            .iter()
            .enumerate()
            .map(|(at, entry)| {
                (
                    entry.title.clone(),
                    entry.detail.clone(),
                    at == listing.chosen,
                )
            })
            .collect::<Vec<_>>()
    };
    keyed(derived(lines).when(every_frame()), move |lines| {
        if lines.is_empty() {
            return frame().boxed();
        }
        column(
            lines
                .iter()
                .cloned()
                .enumerate()
                .map(|(at, (title, detail, chosen))| {
                    line(hub, at, title, detail, chosen)
                })
                .collect::<Vec<_>>(),
        )
        .gap(0.0)
        .padding(UiRect::bottom(px(6.0)))
        .boxed()
    })
    .within(bare())
}

/// An entry as it is listed, filled while it is the one chosen.
fn line(
    hub: Entity,
    at: usize,
    title: String,
    detail: String,
    chosen: bool,
) -> AnyView {
    let mut words = vec![
        label(title)
            .bold(true)
            .wrap(false)
            .rules(small_rules)
            .boxed(),
    ];
    if !detail.is_empty() {
        words.push(
            label(detail)
                .tone(Tone::Dim)
                .wrap(false)
                .rules(small_rules)
                .boxed(),
        );
    }
    let line = button(
        row(words)
            .gap(8.0)
            .align(AlignItems::Center)
            .justify(JustifyContent::FlexStart),
    )
    .padding(UiRect::axes(px(6.0), px(4.0)))
    .justify(JustifyContent::FlexStart)
    .rules(views::tint)
    .on_activate(move |world| pick(world, hub, at));
    match chosen {
        true => AnyView::new(move |cx: &mut Cx| {
            let fill = cx.theme().palette.selected;
            let radius = cx.theme().space.round;
            cx.build(column((line,)).fill(fill).radius(radius))
        }),
        false => line.boxed(),
    }
}

/// The chip of a token, showing `shown`, which is `sent` where it is
/// sent or copied.
fn token_atom(key: String, sent: String, shown: String) -> Atom {
    Atom::new::<HamletTheme>(key, sent, move || {
        token_chip(shown.clone())
    })
}

/// A chip of words alone, filled as an agent's chip is.
fn token_chip(shown: String) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let chip = row((label(shown.clone())
            .size(12.0)
            .wrap(false)
            .tone(Tone::Reference),))
        .align(AlignItems::Center)
        .padding(UiRect::axes(px(6.0), px(1.0)))
        .fill(theme.palette.status.info.fill)
        .radius(theme.space.round);
        cx.build(chip)
    })
}
