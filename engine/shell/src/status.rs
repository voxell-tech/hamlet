//! The status line at the bottom of the window: the items plugins
//! add, in the order they came, then the workspace shown and the
//! log's last line, each behind its icon.

use bevy::app::{App, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::component::Component;
use bevy::ecs::lifecycle::{Add, RemovedComponents};
use bevy::ecs::observer::On;
use bevy::ecs::query::Changed;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::ui::{AlignItems, Display, Overflow, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{FrameProps, TooltipExt, label, row};
use bevy_fynix::{Prop, ViewExt, each, keyed, resource};
use hamlet_engine_api::PanelId;
use hamlet_engine_core::{Log, LogLevel};

use crate::icons::{Glyph, glyph};
use crate::style::{note, raised};
use crate::workspace::Workspaces;
use crate::{StartRank, View, themed};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<StatusLine>()
        .init_resource::<Placed>()
        .add_observer(place)
        .add_systems(Update, follow);
}

/// An item of the status line: an icon and words in a tone, and words
/// shown while the pointer rests on it. One with no words is hidden,
/// unless it grows.
#[derive(Component, Clone, PartialEq, Debug)]
pub struct StatusItem {
    pub id: PanelId,
    pub icon: Option<Glyph>,
    pub text: String,
    pub tone: Tone,
    pub tip: String,
    /// It takes the room left on the line, and clips its words.
    pub grow: bool,
}

impl StatusItem {
    /// The item `id`, with nothing to show yet.
    pub fn new(id: PanelId, grow: bool) -> Self {
        Self {
            id,
            icon: None,
            text: String::new(),
            tone: Tone::Dim,
            tip: String::new(),
            grow,
        }
    }
}

/// On a status item: its plugin's rank, and when it came among all
/// of them.
#[derive(Component, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Order(Option<StartRank>, u64);

/// How many status items have come.
#[derive(Resource, Default)]
struct Placed(u64);

/// What the status line shows.
#[derive(Resource, Clone, PartialEq, Default)]
struct StatusLine {
    items: Vec<StatusItem>,
    workspace: String,
    last: Option<(LogLevel, String)>,
}

/// The status line, for the bottom of the window.
pub fn view() -> impl View {
    themed(|cx| {
        let theme = cx.theme();
        let space = &theme.space;
        let (padding_x, padding_y, gap, fill) = (
            space.status_padding_x,
            space.status_padding_y,
            space.status_gap,
            *theme.colors.background,
        );
        // Each part is built again only as what it is changes; words
        // that change are set in place.
        let items = each(
            resource::<StatusLine, _>(|status| status.items.clone()),
            Clone::clone,
            |shown| {
                let tip = shown.tip.clone();
                let item = item(
                    shown.icon,
                    one_line(&shown.text),
                    shown.tone,
                );
                let item = if shown.grow {
                    row((item,))
                        .grow(1.0)
                        .min_width(px(0.0))
                        .overflow(Overflow::clip())
                        .boxed()
                } else {
                    item.boxed()
                };
                if tip.is_empty() {
                    return item;
                }
                item.tooltip(move || note(tip.clone())).boxed()
            },
        )
        .within(
            row(())
                .gap(gap)
                .align(AlignItems::Center)
                .min_width(px(0.0))
                .grow(resource::<StatusLine, _>(|status| {
                    if status.items.iter().any(|item| item.grow) {
                        1.0
                    } else {
                        0.0
                    }
                }))
                .display(resource::<StatusLine, _>(|status| {
                    if status.items.is_empty() {
                        Display::None
                    } else {
                        Display::Flex
                    }
                })),
        );
        let workspace = item(
            Some(Glyph::Folder),
            resource::<StatusLine, _>(|status| {
                one_line(&status.workspace)
            }),
            Tone::Dim,
        );
        let last = keyed(
            resource::<StatusLine, _>(|status| {
                match status.last.as_ref().map(|(level, _)| level) {
                    Some(LogLevel::Error) => {
                        (Glyph::Struck, Tone::Critical)
                    }
                    Some(LogLevel::Warn) => {
                        (Glyph::Bell, Tone::Accent)
                    }
                    Some(LogLevel::Info) => (Glyph::Page, Tone::Dim),
                    None => (Glyph::Moon, Tone::Dim),
                }
            }),
            |&(which, tone)| {
                let text = resource::<StatusLine, _>(|status| {
                    status
                        .last
                        .as_ref()
                        .map(|(_, text)| one_line(text))
                        .unwrap_or_default()
                });
                item(Some(which), text, tone).boxed()
            },
        );
        row((row((items, workspace, last))
            .gap(gap)
            .grow(1.0)
            .align(AlignItems::Center)
            .min_width(px(0.0))
            .overflow(Overflow::clip()),))
        .align(AlignItems::Center)
        .padding(UiRect::axes(px(padding_x), px(padding_y)))
        .fill(fill)
    })
}

/// `text` on one line however many lines it came in.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// An icon, if any, and the words it stands before, both in `tone`.
fn item(
    which: Option<Glyph>,
    text: impl Into<Prop<String>>,
    tone: Tone,
) -> impl View {
    let text = text.into();
    themed(move |cx| {
        let theme = cx.theme();
        let (side, gap, size) = (
            theme.space.status_icon,
            theme.space.status_icon_gap,
            theme.text.small,
        );
        let words = label(text).size(size).tone(tone).wrap(false);
        match which {
            Some(which) => {
                row((raised(glyph(which, side).tone(tone)), words))
                    .gap(gap)
                    .align(AlignItems::Center)
                    .boxed()
            }
            None => row((words,)).align(AlignItems::Center).boxed(),
        }
    })
}

fn place(
    added: On<Add, StatusItem>,
    ranks: Query<&StartRank>,
    mut placed: ResMut<Placed>,
    mut commands: Commands,
) {
    placed.0 += 1;
    let rank = ranks.get(added.entity).ok().cloned();
    commands.entity(added.entity).insert(Order(rank, placed.0));
}

/// Gathers what the status line shows when an item, the workspaces
/// or the log changed.
fn follow(
    workspaces: Res<Workspaces>,
    log: Res<Log>,
    changed: Query<(), Changed<StatusItem>>,
    placed: Query<(), Changed<Order>>,
    mut gone: RemovedComponents<StatusItem>,
    items: Query<(&StatusItem, &Order)>,
    mut status: ResMut<StatusLine>,
) {
    let went = gone.read().count() > 0;
    if !workspaces.is_changed()
        && !log.is_changed()
        && changed.is_empty()
        && placed.is_empty()
        && !went
    {
        return;
    }
    let mut shown = items
        .iter()
        .filter(|(item, _)| item.grow || !item.text.trim().is_empty())
        .collect::<Vec<_>>();
    shown.sort_by_cached_key(|(_, order)| (*order).clone());
    let now = StatusLine {
        items: shown
            .into_iter()
            .map(|(item, _)| item.clone())
            .collect(),
        workspace: workspaces
            .list()
            .get(workspaces.shown())
            .map(|workspace| workspace.name.clone())
            .unwrap_or_default(),
        last: log
            .lines()
            .next_back()
            .map(|(_, line)| (line.level, line.text.clone())),
    };
    if *status != now {
        *status = now;
    }
}
