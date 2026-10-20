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
use bevy::ui::{AlignItems, Overflow, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{FrameProps, TooltipExt, label, row};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_engine_api::PanelId;
use hamlet_engine_core::{Log, LogLevel};

use crate::icons::{Glyph, glyph};
use crate::style::note;
use crate::workspace::Workspaces;
use crate::{AnyView, Cx};

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

/// On a status item: when it came, among all of them.
#[derive(Component, Clone, Copy)]
struct Order(u64);

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
pub fn view() -> AnyView {
    AnyView::new(|cx: &mut Cx| {
        let theme = cx.theme();
        let space = &theme.space;
        let (padding_x, padding_y, gap, fill) = (
            space.status_padding_x,
            space.status_padding_y,
            space.status_gap,
            *theme.colors.background,
        );
        cx.build(
            keyed(
                resource::<StatusLine, _>(Clone::clone),
                move |status| {
                    let (which, tone, text) = match &status.last {
                        Some((LogLevel::Error, text)) => (
                            Glyph::Struck,
                            Tone::Critical,
                            text.clone(),
                        ),
                        Some((LogLevel::Warn, text)) => {
                            (Glyph::Bell, Tone::Accent, text.clone())
                        }
                        Some((LogLevel::Info, text)) => {
                            (Glyph::Page, Tone::Dim, text.clone())
                        }
                        None => {
                            (Glyph::Moon, Tone::Dim, String::new())
                        }
                    };
                    let mut items = status
                        .items
                        .iter()
                        .map(|shown| {
                            let tip = shown.tip.clone();
                            let item = item(
                                shown.icon,
                                shown.text.clone(),
                                shown.tone,
                            );
                            let item = if shown.grow {
                                row((item,))
                                    .grow(1.0)
                                    .min_width(px(0.0))
                                    .overflow(Overflow::clip())
                                    .boxed()
                            } else {
                                item
                            };
                            if tip.is_empty() {
                                return item;
                            }
                            item.tooltip(move || note(tip.clone()))
                                .boxed()
                        })
                        .collect::<Vec<_>>();
                    items.extend([
                        item(
                            Some(Glyph::Folder),
                            status.workspace.clone(),
                            Tone::Dim,
                        ),
                        item(Some(which), text, tone),
                    ]);
                    row(items)
                        .gap(gap)
                        .grow(1.0)
                        .align(AlignItems::Center)
                        .min_width(px(0.0))
                        .overflow(Overflow::clip())
                        .boxed()
                },
            )
            .within(
                row(())
                    .align(AlignItems::Center)
                    .padding(UiRect::axes(
                        px(padding_x),
                        px(padding_y),
                    ))
                    .fill(fill),
            ),
        )
    })
}

/// An icon, if any, and the words it stands before, both in `tone`,
/// the words on one line however many lines they came in.
fn item(which: Option<Glyph>, text: String, tone: Tone) -> AnyView {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (side, gap, size) = (
            theme.space.status_icon,
            theme.space.status_icon_gap,
            theme.text.small,
        );
        let words = label(text).size(size).tone(tone).wrap(false);
        match which {
            Some(which) => cx.build(
                row((glyph(which, side).tone(tone), words))
                    .gap(gap)
                    .align(AlignItems::Center),
            ),
            None => cx.build(row((words,)).align(AlignItems::Center)),
        }
    })
}

fn place(
    added: On<Add, StatusItem>,
    mut placed: ResMut<Placed>,
    mut commands: Commands,
) {
    placed.0 += 1;
    commands.entity(added.entity).insert(Order(placed.0));
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
    shown.sort_by_key(|(_, order)| order.0);
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
