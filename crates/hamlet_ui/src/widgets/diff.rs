//! Fixed-height diff rows, collapsed context and viewport-sized
//! rendering.

use std::collections::BTreeSet;
use std::ops::Range;

use bevy::prelude::*;
use bevy_fynix::scroll::{
    OwnScrollbar, ScrollbarStyle, ScrollbarVisibility,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, frame, ghost, label,
    row, scroll,
};
use bevy_fynix::{ScopedExt, ViewExt, component, keyed};
use hamlet_rules::review::{Diff, Line, Mark};

use crate::looks::{Fills, bare, note};
use crate::platform::MonoText;
use crate::{AnyView, Cx};

const HEIGHT: f32 = 22.0;
const OVERSCAN: usize = 8;

pub fn plugin(app: &mut App) {
    app.add_systems(Update, viewport);
}

/// The projection of a file's old and new lines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layout {
    #[default]
    Unified,
    Split,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Row {
    Line(Option<usize>, Option<usize>),
    Gap(Range<usize>),
}

#[derive(Component)]
struct Document {
    diff: Diff,
    layout: Layout,
    expanded: BTreeSet<usize>,
    rows: Vec<Row>,
    window: Range<usize>,
    width: f32,
    code_width: f32,
    revision: usize,
}

fn project(
    diff: &Diff,
    layout: Layout,
    expanded: &BTreeSet<usize>,
) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut at = 0;
    while at < diff.lines.len() {
        if diff.lines[at].mark == Mark::Context {
            let start = at;
            while at < diff.lines.len()
                && diff.lines[at].mark == Mark::Context
            {
                at += 1;
            }
            let first = if start == 0 {
                start
            } else {
                (start + 3).min(at)
            };
            let last = if at == diff.lines.len() {
                at
            } else {
                at.saturating_sub(3).max(first)
            };
            for index in start..at {
                if last > first + 1
                    && !expanded.contains(&start)
                    && (first..last).contains(&index)
                {
                    if index == first {
                        rows.push(Row::Gap(start..at));
                    }
                } else {
                    rows.push(Row::Line(Some(index), Some(index)));
                }
            }
        } else {
            let mut deleted = Vec::new();
            let mut added = Vec::new();
            while at < diff.lines.len()
                && diff.lines[at].mark != Mark::Context
            {
                match diff.lines[at].mark {
                    Mark::Delete => deleted.push(at),
                    Mark::Add => added.push(at),
                    Mark::Context => unreachable!(),
                }
                at += 1;
            }
            match layout {
                Layout::Unified => {
                    rows.extend(
                        deleted
                            .into_iter()
                            .map(|i| Row::Line(Some(i), None)),
                    );
                    rows.extend(
                        added
                            .into_iter()
                            .map(|i| Row::Line(None, Some(i))),
                    );
                }
                Layout::Split => {
                    for i in 0..deleted.len().max(added.len()) {
                        rows.push(Row::Line(
                            deleted.get(i).copied(),
                            added.get(i).copied(),
                        ));
                    }
                }
            }
        }
    }
    rows
}

fn viewport(
    mut documents: Query<(
        &mut Document,
        &ScrollPosition,
        &ComputedNode,
    )>,
) {
    for (mut document, scroll, computed) in &mut documents {
        let size = (computed.size() - computed.scrollbar_size)
            * computed.inverse_scale_factor();
        let first = (scroll.y.max(0.0) / HEIGHT) as usize;
        let window = first
            .saturating_sub(OVERSCAN)
            .min(document.rows.len())
            ..(first + (size.y / HEIGHT).ceil() as usize + OVERSCAN)
                .min(document.rows.len());
        let panes = if document.layout == Layout::Split {
            2.0
        } else {
            1.0
        };
        let width = size.x.max(document.code_width * panes);
        if document.window != window || document.width != width {
            document.window = window;
            document.width = width;
        }
    }
}

/// A read-only viewer of prepared diff data. Git loading belongs to
/// its caller.
pub fn view(diff: Diff, layout: Layout) -> impl crate::View {
    AnyView::new(move |cx: &mut Cx| {
        if diff.lines.is_empty() {
            return cx.build(note(diff.description));
        }
        let rows = project(&diff, layout, &BTreeSet::new());
        let code_width = diff
            .lines
            .iter()
            .map(|l| {
                let characters = l
                    .text
                    .chars()
                    .map(|c| {
                        if c == '\t' {
                            4
                        } else if c.is_ascii() {
                            1
                        } else {
                            2
                        }
                    })
                    .sum::<usize>();
                (characters + if l.no_newline { 15 } else { 0 })
                    as f32
                    * 8.0
                    + 125.0
            })
            .fold(200.0, f32::max);
        let bars = OwnScrollbar(ScrollbarStyle {
            visibility: ScrollbarVisibility::WhenNeeded,
            floating: false,
            ..*cx.world.resource::<ScrollbarStyle>()
        });
        let root = cx.build(
            scroll(())
                .gap(0.0)
                .fills_height()
                .fills_width()
                .with(bars),
        );
        cx.world.entity_mut(root).insert(Document {
            diff,
            layout,
            expanded: BTreeSet::new(),
            rows,
            window: 0..0,
            width: code_width,
            code_width,
            revision: 0,
        });
        cx.under(root, |cx| {
            cx.build(
                keyed(
                    component::<Document, _>(root, |d| {
                        d.map(|d| {
                            (d.window.clone(), d.width, d.revision)
                        })
                    }),
                    move |_| {
                        AnyView::new(move |cx: &mut Cx| {
                            let document = cx
                                .world
                                .get::<Document>(root)
                                .unwrap();
                            let width = document.width;
                            let top =
                                document.window.start as f32 * HEIGHT;
                            let bottom = (document.rows.len()
                                - document.window.end)
                                as f32
                                * HEIGHT;
                            let mut views = vec![
                                frame()
                                    .height(px(top))
                                    .shrink(0.0)
                                    .boxed(),
                            ];
                            for item in &document.rows
                                [document.window.clone()]
                            {
                                views.push(draw_row(
                                    root, document, item,
                                ));
                            }
                            views.push(
                                frame()
                                    .height(px(bottom))
                                    .shrink(0.0)
                                    .boxed(),
                            );
                            cx.build(
                                column(views)
                                    .gap(0.0)
                                    .width(px(width))
                                    .shrink(0.0),
                            )
                        })
                    },
                )
                .within(bare().shrink(0.0)),
            );
        });
        root
    })
}

fn draw_row(
    root: Entity,
    document: &Document,
    item: &Row,
) -> AnyView {
    match item {
        Row::Gap(range) => {
            let start = range.start;
            let kept = usize::from(start > 0) * 3
                + usize::from(range.end < document.diff.lines.len())
                    * 3;
            button(
                label(format!(
                    "+  Expand {} unchanged lines",
                    range.len().saturating_sub(kept)
                ))
                .size(12.0)
                .tone(Tone::Faint)
                .wrap(false),
            )
            .height(px(HEIGHT))
            .shrink(0.0)
            .justify(JustifyContent::FlexStart)
            .padding(UiRect::left(px(8.0)))
            .rules(ghost)
            .on_activate(move |world| {
                if let Some(mut document) =
                    world.get_mut::<Document>(root)
                {
                    document.expanded.insert(start);
                    document.rows = project(
                        &document.diff,
                        document.layout,
                        &document.expanded,
                    );
                    document.revision += 1;
                }
            })
            .boxed()
        }
        Row::Line(old, new) => {
            let line = |at: Option<usize>| {
                at.map(|i| document.diff.lines[i].clone())
            };
            if document.layout == Layout::Unified {
                cell(line(old.or(*new)), None, document.width)
            } else {
                row((
                    cell(
                        line(*old),
                        Some(false),
                        document.width / 2.0,
                    ),
                    cell(
                        line(*new),
                        Some(true),
                        document.width / 2.0,
                    ),
                ))
                .gap(0.0)
                .height(px(HEIGHT))
                .shrink(0.0)
                .boxed()
            }
        }
    }
}

fn cell(
    line: Option<Line>,
    side: Option<bool>,
    width: f32,
) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let palette = &cx.theme().palette;
        let (fill, tone) = match line.as_ref().map(|l| l.mark) {
            Some(Mark::Delete) => (palette.red_deep, Tone::Critical),
            Some(Mark::Add) => (palette.moss_deep, Tone::Body),
            _ => (palette.black, Tone::Dim),
        };
        let number = |n: Option<usize>| {
            n.map(|n| n.to_string()).unwrap_or_default()
        };
        let (numbers, text) = line
            .map(|l| {
                let numbers = match side {
                    None => format!(
                        "{:>5} {:>5}",
                        number(l.old),
                        number(l.new)
                    ),
                    Some(false) => format!("{:>5}", number(l.old)),
                    Some(true) => format!("{:>5}", number(l.new)),
                };
                let mark = match l.mark {
                    Mark::Context => ' ',
                    Mark::Delete => '-',
                    Mark::Add => '+',
                };
                let text = format!(
                    "{mark} {}{}",
                    l.text.replace('\t', "    ").replace('\r', "\\r"),
                    if l.no_newline { " [no newline]" } else { "" }
                );
                (numbers, text)
            })
            .unwrap_or_default();
        cx.build(
            row((
                label(numbers)
                    .size(12.5)
                    .wrap(false)
                    .tone(Tone::Faint)
                    .tagged(MonoText),
                label(text)
                    .size(12.5)
                    .wrap(false)
                    .tone(tone)
                    .tagged(MonoText),
            ))
            .gap(12.0)
            .align(AlignItems::Center)
            .padding(UiRect::horizontal(px(8.0)))
            .width(px(width))
            .min_width(px(width))
            .height(px(HEIGHT))
            .shrink(0.0)
            .fill(fill),
        )
    })
}

#[cfg(test)]
mod tests {
    use bevy::time::TimePlugin;
    use bevy_fynix::{FynixPlugin, mount};
    use hamlet_assets::Tuning;

    use super::*;
    use crate::theme::{self, HamletTheme};

    #[test]
    fn a_large_file_builds_only_the_visible_lines_and_scrolls_to_the_end()
     {
        let mut app = App::new();
        app.add_plugins((
            TimePlugin,
            FynixPlugin::<HamletTheme>::default(),
            plugin,
        ));
        theme::insert_theme(app.world_mut(), HamletTheme::designed());
        let diff = Diff {
            lines: (1..=50_000)
                .map(|i| Line {
                    old: None,
                    new: Some(i),
                    mark: Mark::Add,
                    text: format!("source {i}"),
                    no_newline: false,
                })
                .collect(),
            description: String::new(),
        };
        let root = mount::<HamletTheme>(
            app.world_mut(),
            view(diff, Layout::Unified),
        );
        app.world_mut().entity_mut(root).insert(ComputedNode {
            size: Vec2::new(800.0, 220.0),
            content_size: Vec2::new(800.0, 1_100_000.0),
            inverse_scale_factor: 1.0,
            ..default()
        });
        let texts = |app: &mut App| {
            app.update();
            app.update();
            let world = app.world_mut();
            world
                .query::<&Text>()
                .iter(world)
                .map(|t| t.0.clone())
                .collect::<Vec<_>>()
        };
        let first = texts(&mut app);
        assert!(first.iter().any(|t| t == "+ source 1"));
        assert!(
            first.len() < 100,
            "offscreen rows should not create text nodes"
        );
        app.world_mut().get_mut::<ScrollPosition>(root).unwrap().y =
            1_099_780.0;
        let last = texts(&mut app);
        assert!(last.iter().any(|t| t == "+ source 50000"));
        assert!(!last.iter().any(|t| t == "+ source 1"));
        assert!(last.len() < 100);
    }

    #[test]
    fn split_pairs_unequal_changes_and_context_expands_without_losing_lines()
     {
        let mut diff = Diff::default();
        for i in 0..20 {
            diff.lines.push(Line {
                old: Some(i + 1),
                new: Some(i + 1),
                mark: Mark::Context,
                text: format!("line {i}"),
                no_newline: false,
            });
        }
        for mark in [
            Mark::Delete,
            Mark::Delete,
            Mark::Add,
            Mark::Add,
            Mark::Add,
        ] {
            diff.lines.push(Line {
                old: None,
                new: None,
                mark,
                text: String::new(),
                no_newline: false,
            });
        }
        let split = project(&diff, Layout::Split, &BTreeSet::new());
        assert_eq!(
            &split[4..],
            &[
                Row::Line(Some(20), Some(22)),
                Row::Line(Some(21), Some(23)),
                Row::Line(None, Some(24))
            ]
        );
        assert_eq!(split[0], Row::Gap(0..20));
        let all =
            project(&diff, Layout::Unified, &BTreeSet::from([0]));
        assert_eq!(all.len(), 25);
        assert_eq!(all[0], Row::Line(Some(0), Some(0)));
        assert_eq!(all[24], Row::Line(None, Some(24)));
    }
}
