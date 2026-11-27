//! How a row of the rail is drawn.

use bevy::ecs::world::World;
use bevy::ui::{
    AlignItems, Display, JustifyContent, Overflow, PositionType,
    UiRect, Val, percent, px,
};
use bevy_fynix::tokens::{MotionTokens, SurfaceTokens, Tone};
use bevy_fynix::views::{
    self, BehaviorExt, FrameProps, TooltipExt, button, column, frame,
    label, row,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, every_frame, style};
use hamlet_ui::styles::{
    ICON, STATUS_PAD, description_rules, note, note_rules, quoted,
    status_icon_at, transparent,
};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::effects::{Edge, Variant, Wash};
use hamlet_ui::widgets::ellipsis::{cut, one_line};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, View, with_cx};

use super::lines::{Group, Line, Target, age};
use super::{
    CHEVRON, DIMMED, GAP, ROW_DOWN, ROW_INSET, Rail, STEP_IN,
    TITLE_SHOWN, WORKER_ICON, go, row_height,
};
use crate::standing::Standing;
use crate::status::now_ms;
use crate::words;
use crate::world_menu::Asks;

pub(super) fn row_rules(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(transparent(t.palette.button))
                .border(t.space.border)
                .border_color(transparent(t.palette.accent))
                .radius(t.space.round)
                .justify(JustifyContent::FlexStart)
                .align(AlignItems::Stretch)
                .padding(UiRect::axes(px(ROW_INSET), px(ROW_DOWN)))
        })
        .pressed(|s| s.frame(|f, t| f.fill(t.pressed())))
        .transition(HamletTheme::interact)
        .apply(cx);
}

/// `row` on a frame that is washed with light while `held`, and less
/// under the pointer: the row's own gradient is its glow, which would
/// take the wash back.
pub(super) fn washed(
    shown: impl View + 'static,
    held: bool,
) -> impl View {
    with_cx(move |cx| {
        row((shown,))
            .grow(1.0)
            .min_width(px(0.0))
            .radius(cx.theme().space.round)
            .tagged(Wash::new(held, Edge::Left))
    })
}

/// The mark at the end of a row or a header, in its place whether or
/// not there is one, so that marks line up from row to row.
pub(super) fn mark_icon(mark: Option<Standing>) -> AnyView {
    match mark {
        Some(standing) => status_icon_at(
            standing.glyph(),
            standing.word(),
            standing.kind(),
            ICON,
        )
        .boxed(),
        None => frame().width(px(ICON + STATUS_PAD.0 * 2.0)).boxed(),
    }
}

/// The chevron that folds `group`, pointing down while open and to
/// the right while folded, in the rail's one column of chevrons.
pub(super) fn chevron(
    group: Group,
    shut: bool,
    says: &'static str,
) -> AnyView {
    button(
        glyph(Glyph::Chevron, 10.0)
            .tone(if group == Group::Left {
                Tone::Dim
            } else {
                Tone::Faint
            })
            .rotation(if shut { 90.0 } else { 180.0 }),
    )
    .width(px(CHEVRON))
    .shrink(0.0)
    .padding(UiRect::all(px(0.0)))
    .rules(views::tint)
    .tooltip(move || note(says))
    .on_activate(move |world| {
        world.resource_mut::<Rail>().toggle(group);
    })
    .boxed()
}

/// A row: its icon, then its name with how long ago and its mark at
/// the end, over what it is doing, which runs the row's whole width
/// and wraps rather than being cut. A worker's row is one short line
/// with what it is doing beside its name, cut at the rail's edge.
/// A `dimmed` row, of those that left, is set in grey.
pub(super) fn expanded(line: &Line, dimmed: bool) -> AnyView {
    let (target, since, worker) =
        (line.target, line.since, line.worker);
    let name_tone = if line.current {
        Tone::Accent
    } else if dimmed || worker {
        Tone::Dim
    } else {
        Tone::Body
    };
    let title = cut(&line.title, TITLE_SHOWN);
    let clipped = |shown: AnyView| {
        column((shown,))
            .min_width(px(0.0))
            .overflow(Overflow::clip())
    };
    let name = clipped(
        one_line(
            label(line.name.clone())
                .tone(name_tone)
                .bold(!(worker || dimmed)),
        )
        .boxed(),
    );
    let words = if worker {
        row((
            name,
            clipped(one_line(label(title.clone())).boxed())
                .grow(1.0)
                .rules(note_rules),
        ))
        .gap(6.0)
        .align(AlignItems::Center)
        .grow(1.0)
        .min_width(px(0.0))
        .boxed()
    } else {
        name.grow(1.0).boxed()
    };
    // The icon on the middle of the name's line.
    let head = row((
        glyph(line.glyph, if worker { WORKER_ICON } else { ICON })
            .tone(Tone::Dim)
            .opacity(if dimmed { DIMMED } else { 1.0 }),
        row((
            words,
            label(
                derived(move |_: &World| age(now_ms(), since))
                    .when(every_frame()),
            )
            .wrap(false)
            .rules(note_rules),
            mark_icon(line.mark),
        ))
        .gap(6.0)
        .align(AlignItems::Center)
        .grow(1.0)
        .min_width(px(0.0)),
    ))
    .gap(if worker { 6.0 } else { 8.0 })
    .align(AlignItems::Center);
    // Under it, from the row's own edge rather than the name's, what
    // it is doing. A row with nothing more to say is one line.
    let doing = column((if title.contains('`') {
        quoted(&title)
    } else {
        label(title.clone()).boxed()
    },))
    .min_width(px(0.0))
    .display(if title.is_empty() || worker {
        Display::None
    } else {
        Display::Flex
    })
    .rules(description_rules);
    let content = column((head, doing))
        .gap(0.0)
        .justify(JustifyContent::Center)
        .min_height(px(row_height(line) - 2.0 * ROW_DOWN))
        .width(percent(100.0));
    let address = line.address.clone();
    let pressed = button(content)
        .grow(1.0)
        .min_width(px(0.0))
        .rules(row_rules)
        .tagged(Variant::Default)
        .on_activate(move |world| go(world, target));
    // The right button opens its menu, as on the thing in the world.
    let pressed = match target.thing() {
        Some(thing) => pressed.tagged(Asks(thing)).boxed(),
        None => pressed.boxed(),
    };
    let shown = washed(pressed, line.current).boxed();
    let shown = match tip_of(line, address) {
        Some(tip) => shown.tooltip(move || note(tip.clone())).boxed(),
        None => shown.boxed(),
    };
    // The chevron of a lead, outside the row's press so that folding
    // does not open the thread, then the step in of a row under one.
    let fold = match line.fold {
        Some((group, shut)) => chevron(
            group,
            shut,
            match (group, shut) {
                (Group::Workers(_), true) => "Show the workers",
                (Group::Workers(_), false) => "Fold the workers",
                (_, true) => "Show the hires under it",
                (_, false) => "Fold the hires under it",
            },
        ),
        None => frame().width(px(CHEVRON)).shrink(0.0).boxed(),
    };
    let step = frame()
        .width(px(f32::from(line.depth) * STEP_IN))
        .shrink(0.0);
    let mut parts = vec![step.boxed(), fold, shown];
    parts.extend(tree_lines(line, title.is_empty() || worker));
    row(parts)
        .gap(0.0)
        .align(AlignItems::Center)
        .width(percent(100.0))
        .boxed()
}

/// The tree's lines over a row, on a grid of [`STEP_IN`]: for each
/// level above it, the guide of an agent that has more under it after
/// this row, then the elbow from its own lead, which runs on down
/// while more follow. Every guide runs under the middle of its
/// agent's chevron, so the tree lines up however deep it goes, and
/// an elbow stops short of the row it leads to. A row of `one_line`
/// has its icon on its middle, one of more on its first.
fn tree_lines(line: &Line, one_line: bool) -> Vec<AnyView> {
    if line.depth == 0 {
        return Vec::new();
    }
    // Where the guide of the agent at `level` runs.
    let guide =
        |level: usize| level as f32 * STEP_IN + CHEVRON / 2.0 - 0.5;
    let lead = usize::from(line.depth) - 1;
    // Where the elbow meets the row, and where it stops.
    let elbow = ROW_DOWN
        + if one_line {
            (row_height(line) - 2.0 * ROW_DOWN) / 2.0
        } else {
            2.0 + ICON / 2.0
        };
    // A line laid over the row, `left` and `top` in from its edges;
    // one with no height runs to its foot.
    let over =
        |left: f32, top: f32, width: f32, height: Option<f32>| {
            with_cx(move |cx| {
                frame()
                    .width(px(width))
                    .height(height.map_or(Val::Auto, px))
                    .fill(cx.theme().palette.gold_6)
                    .position(PositionType::Absolute)
                    .inset(UiRect::new(
                        px(left),
                        Val::Auto,
                        px(top),
                        if height.is_some() {
                            Val::Auto
                        } else {
                            px(0.0)
                        },
                    ))
            })
            .boxed()
        };
    // The guides of the levels above that carry on past the row.
    let mut lines = line
        .rails
        .iter()
        .enumerate()
        .filter(|(_, on)| **on)
        .map(|(level, _)| over(guide(level), 0.0, 1.0, None))
        .collect::<Vec<_>>();
    // The elbow from its lead, stopping short of its own chevron, or
    // of the row's edge where it has no chevron, so it never runs
    // into the row.
    let start = f32::from(line.depth) * STEP_IN;
    let end = match line.fold {
        Some(_) => start - GAP,
        None => start + CHEVRON - GAP,
    };
    let x = guide(lead);
    lines.push(over(x, 0.0, 1.0, Some(elbow + 1.0)));
    lines.push(over(x, elbow, end - x, Some(1.0)));
    if !line.last {
        lines.push(over(x, elbow, 1.0, None));
    }
    lines
}

/// What a group's header names, in plain words.
pub(super) fn group_meaning(group: Group) -> Option<&'static str> {
    match group {
        Group::Needs => Some(words::NEEDS),
        Group::Hamlet(_) => Some(words::HAMLET),
        Group::City => Some(words::CITY),
        Group::Harbor => Some(words::HARBOR),
        Group::Left => Some(words::LEFT),
        Group::Top | Group::Lead(_) | Group::Workers(_) => None,
    }
}

/// What a row's tooltip says: the address it goes by, where that is
/// not its name, over what it is in plain words.
fn tip_of(line: &Line, address: String) -> Option<String> {
    let meaning = match line.target {
        Target::Realm => Some(words::REALM),
        Target::Library => Some(words::LIBRARY),
        Target::Hand => Some(words::HAND),
        Target::Agent(_) | Target::Gone(_) => match line.glyph {
            Glyph::Mayor => Some(words::MAYOR),
            Glyph::Worker => Some(words::WORKER),
            _ => Some(words::HIRE),
        },
        Target::Fold(Group::Workers(_)) => Some(words::WORKER),
        Target::Profile(_) => Some(words::PROFILE),
        Target::Hamlet(_) | Target::Fold(_) => None,
    };
    let address = (!address.is_empty()
        && address != line.name
        && line.target != Target::Hand)
        .then_some(address);
    match (address, meaning) {
        (Some(address), Some(meaning)) => {
            Some(format!("{address}\n{meaning}"))
        }
        (Some(address), None) => Some(address),
        (None, meaning) => meaning.map(str::to_string),
    }
}
