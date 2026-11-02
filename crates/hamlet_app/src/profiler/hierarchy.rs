//! The hierarchy: the spans of the frame shown as a tree that opens
//! row by row, each with its total and self time and how many times
//! it ran, sorted by either time.

use std::cmp::Reverse;
use std::time::Duration;

use bevy::app::{App, PreUpdate};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_exists;
use bevy::ecs::system::{Local, Res, ResMut};
use bevy::ecs::world::World;
use bevy::math::Rot2;
use bevy::ui::{
    AlignItems, JustifyContent, Overflow, UiRect, UiTransform, px,
};
use bevy::utils::prelude::ShortName;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    self, BehaviorExt, FrameProps, column, frame, label, menu_item,
    row,
};
use bevy_fynix::{ScopedExt, ViewExt, keyed, resource};
use hamlet_profiler::{Frame, Lane, NameId, Profiler, Span};
use hamlet_ui::styles::{Fills, choice, scroll_area, small_rules};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::platform::MonoText;
use hamlet_ui::{AnyView, View, built};

use super::{Sort, Watch};
use crate::PanelSystems;

/// The width of each column of numbers, in logical pixels.
const NUMBER: f32 = 64.0;

/// How far each level of the tree is set in, in logical pixels.
const INDENT: f32 = 12.0;

/// The side of a row's chevron.
const CHEVRON: f32 = 10.0;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Tree>().add_systems(
        PreUpdate,
        bind.in_set(PanelSystems::Bind)
            .run_if(resource_exists::<Profiler>),
    );
}

/// The rows of the frame shown, worked out from the records.
#[derive(Resource, Default)]
pub(super) struct Tree {
    pub(super) summary: String,
    rows: Vec<Row>,
    /// Moved each time the rows change.
    version: u64,
}

#[derive(Clone, PartialEq)]
struct Row {
    /// The names from the root down to this row's.
    path: Vec<NameId>,
    depth: u16,
    name: String,
    total: Duration,
    self_time: Duration,
    calls: u32,
    /// `None` when it has no children.
    open: Option<bool>,
}

/// Works out the rows again when the frame shown, its records or the
/// way it is shown have changed.
fn bind(
    profiler: Res<Profiler>,
    watch: Res<Watch>,
    mut tree: ResMut<Tree>,
    mut seen: Local<Option<(Option<u64>, [bool; 3])>>,
) {
    let recorder = profiler.lock();
    let shown = watch.shown.and_then(|number| recorder.frame(number));
    let filled = shown.map_or([false; 3], |frame| {
        [
            frame.main.is_some(),
            frame.render.is_some(),
            frame.gpu.is_some(),
        ]
    });
    let key = (watch.shown, filled);
    if *seen == Some(key) && !watch.is_changed() {
        return;
    }
    *seen = Some(key);
    // Copied out, so the lock that every span takes is held only as
    // long as that.
    let frame = shown.map(|frame| {
        let names = |lane| {
            frame.timing(lane).map_or_else(Vec::new, |timing| {
                timing
                    .spans
                    .iter()
                    .map(|span| {
                        ShortName(recorder.name(span.name))
                            .to_string()
                    })
                    .collect::<Vec<_>>()
            })
        };
        (frame.clone(), [names(Lane::Main), names(Lane::Render)])
    });
    drop(recorder);
    let (summary, rows) = match &frame {
        Some((frame, names)) => {
            (summary(frame), rows(frame, names, &watch))
        }
        None => ("No frame recorded yet".into(), Vec::new()),
    };
    if tree.summary != summary {
        tree.summary = summary;
    }
    if tree.rows != rows {
        tree.rows = rows;
        tree.version += 1;
    }
}

fn ms(took: Duration) -> String {
    format!("{:.2}", took.as_secs_f64() * 1000.0)
}

fn summary(frame: &Frame) -> String {
    let time = |took: Option<Duration>| took.map_or("-".into(), ms);
    let total = |lane| frame.timing(lane).map(|timing| timing.total);
    let timings = [Lane::Main, Lane::Render]
        .into_iter()
        .filter_map(|lane| frame.timing(lane));
    let cost = timings.clone().map(|timing| timing.cost).sum();
    let entered = timings.map(|timing| timing.entered).sum::<u32>();
    format!(
        "Frame {}   main {} ms   render {} ms   GPU {} ms   \
         profiler {} ms over {entered} spans",
        frame.number,
        time(total(Lane::Main)),
        time(total(Lane::Render)),
        time(frame.gpu),
        ms(cost),
    )
}

/// The rows of `frame` open in `watch`, its threads' roots first.
fn rows(
    frame: &Frame,
    names: &[Vec<String>; 2],
    watch: &Watch,
) -> Vec<Row> {
    let mut rows = Vec::new();
    for (at, lane, title) in [
        (0, Lane::Main, "Main thread"),
        (1, Lane::Render, "Render thread"),
    ] {
        let Some(timing) = frame.timing(lane) else {
            continue;
        };
        let lines = Lines {
            spans: &timing.spans,
            names: &names[at],
            children: children(&timing.spans),
            watch,
        };
        let mut path = Vec::new();
        lines.add(0, title, &mut path, &mut rows);
    }
    rows
}

/// The children of each span of a tree laid out depth first.
fn children(spans: &[Span]) -> Vec<Vec<usize>> {
    let mut children = vec![Vec::new(); spans.len()];
    let mut above = Vec::<usize>::new();
    for (at, span) in spans.iter().enumerate() {
        above.truncate(span.depth as usize);
        if let Some(&parent) = above.last() {
            children[parent].push(at);
        }
        above.push(at);
    }
    children
}

/// One thread's tree, to be turned into rows.
struct Lines<'a> {
    spans: &'a [Span],
    names: &'a [String],
    children: Vec<Vec<usize>>,
    watch: &'a Watch,
}

impl Lines<'_> {
    /// Adds the row of the span at `at`, named `name`, and its open
    /// children's below it.
    fn add(
        &self,
        at: usize,
        name: &str,
        path: &mut Vec<NameId>,
        rows: &mut Vec<Row>,
    ) {
        let span = self.spans[at];
        path.push(span.name);
        let children = &self.children[at];
        let open =
            (span.depth == 0) != self.watch.toggled.contains(path);
        rows.push(Row {
            path: path.clone(),
            depth: span.depth,
            name: name.to_string(),
            total: span.total,
            self_time: span.self_time,
            calls: span.calls,
            open: (!children.is_empty()).then_some(open),
        });
        if open {
            let mut children = children.clone();
            children.sort_by_key(|&child| {
                let child = self.spans[child];
                Reverse(match self.watch.sort {
                    Sort::Total => child.total,
                    Sort::SelfTime => child.self_time,
                })
            });
            for child in children {
                self.add(child, &self.names[child], path, rows);
            }
        }
        path.pop();
    }
}

fn toggle(world: &mut World, path: &[NameId]) {
    let mut watch = world.resource_mut::<Watch>();
    if !watch.toggled.remove(path) {
        watch.toggled.insert(path.to_vec());
    }
}

fn sort_by(
    sort: Sort,
) -> impl Fn(&mut World) + Send + Sync + 'static {
    move |world| world.resource_mut::<Watch>().sort = sort
}

/// The column heads and the rows below them.
pub(super) fn hierarchy() -> impl View {
    let head =
        keyed(resource::<Watch, _>(|watch| watch.sort), |&sort| {
            let heading = |text: &'static str, by: Sort| {
                views::button(label(text).wrap(false))
                    .width(px(NUMBER))
                    .shrink(0.0)
                    .justify(JustifyContent::End)
                    .rules(choice(sort == by))
                    .on_activate(sort_by(by))
                    .boxed()
            };
            row((
                row((label("Span").tone(Tone::Faint).wrap(false),))
                    .fills_width(),
                heading("Total ms", Sort::Total),
                heading("Self ms", Sort::SelfTime),
                number("Calls".into(), Tone::Faint),
            ))
            .align(AlignItems::Center)
            .boxed()
        })
        .within(column(()));
    let body =
        keyed(resource::<Tree, _>(|tree| tree.version), |_| {
            built(|cx| {
                let rows = cx
                    .world
                    .resource::<Tree>()
                    .rows
                    .iter()
                    .map(line)
                    .collect::<Vec<_>>();
                cx.build(column(rows).gap(0.0))
            })
            .boxed()
        })
        .within(column(()));
    column((head, scroll_area((body,))))
        .gap(4.0)
        .fills_height()
        .rules(small_rules)
}

/// A number in its column, right aligned.
fn number(text: String, tone: Tone) -> AnyView {
    row((label(text).tone(tone).wrap(false).tagged(MonoText),))
        .width(px(NUMBER))
        .shrink(0.0)
        .justify(JustifyContent::End)
        .boxed()
}

/// A row of the tree. One with children opens or shuts when pressed.
fn line(row_of: &Row) -> AnyView {
    let mark = match row_of.open {
        Some(open) => {
            let turn = if open { 180.0 } else { 90.0 };
            row((glyph(Glyph::Chevron, CHEVRON).tone(Tone::Faint),))
                .with(UiTransform::from_rotation(Rot2::degrees(turn)))
                .boxed()
        }
        None => {
            frame().width(px(CHEVRON)).height(px(CHEVRON)).boxed()
        }
    };
    let tone = if row_of.depth == 0 {
        Tone::Body
    } else {
        Tone::Dim
    };
    let content = row((
        frame()
            .width(px(f32::from(row_of.depth) * INDENT))
            .shrink(0.0),
        mark,
        row((label(row_of.name.clone()).tone(tone).wrap(false),))
            .fills_width()
            .overflow(Overflow::clip()),
        number(ms(row_of.total), tone),
        number(ms(row_of.self_time), tone),
        number(row_of.calls.to_string(), tone),
    ))
    .gap(4.0)
    .align(AlignItems::Center);
    match row_of.open {
        Some(_) => {
            let path = row_of.path.clone();
            menu_item(content)
                .on_activate(move |world| toggle(world, &path))
                .boxed()
        }
        None => content.padding(UiRect::vertical(px(2.0))).boxed(),
    }
}
