//! A plugin's limits on its page: each off until the person switches
//! it on, a switch with its name beside a value and its unit, the
//! value kept while it is off.

use bevy::ecs::entity::Entity;
use bevy::ui::{AlignItems, px};
use bevy_fynix::ViewExt;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{FrameProps, column, label, row};
use hamlet_engine_api::{
    Limit, LimitKind, LimitUnit, Limits, PluginId,
};
use hamlet_engine_core::StartingLimits;
use hamlet_engine_shell::style::{
    dropdown, field, labelled_switch, note, section,
};
use hamlet_engine_shell::{AnyView, Cx};

use crate::list::ListWorldExt;

/// A unit of memory the page shows and takes it in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Unit {
    Kb,
    Mb,
    Gb,
}

impl Unit {
    const ALL: [Unit; 3] = [Unit::Kb, Unit::Mb, Unit::Gb];

    fn bytes(self) -> u64 {
        match self {
            Self::Kb => 1 << 10,
            Self::Mb => 1 << 20,
            Self::Gb => 1 << 30,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Kb => "KB",
            Self::Mb => "MB",
            Self::Gb => "GB",
        }
    }

    /// The largest unit `bytes` holds one of, KB at least.
    fn of(bytes: u64) -> Self {
        Self::ALL
            .into_iter()
            .rev()
            .find(|unit| bytes >= unit.bytes())
            .unwrap_or(Self::Kb)
    }
}

/// `value` of `per` as a number, to two decimals at most.
fn amount(value: u64, per: u64) -> String {
    let text = format!("{:.2}", value as f64 / per as f64);
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// `text`, a number of `per`, when it is at least one. `_` between
/// digits is passed over.
fn parse(text: &str, per: u64) -> Option<u64> {
    let number = text.trim().replace('_', "").parse::<f64>().ok()?;
    let value = (number * per as f64).round();
    // The cast saturates at u64::MAX.
    (value.is_finite() && value >= 1.0).then_some(value as u64)
}

/// LIMITS: each limit of `plugin`, of the id `id`, as the person set
/// it in `limits`, and why a value typed for `refused` was refused.
pub(super) fn limits_view(
    plugin: Entity,
    id: &PluginId,
    limits: &Limits,
    refused: Option<(LimitKind, String)>,
) -> AnyView {
    let none = LimitKind::ALL
        .iter()
        .all(|&kind| !limits.get(kind).is_some_and(|limit| limit.on));
    let limits = limits.clone();
    let id = id.clone();
    AnyView::new(move |cx: &mut Cx| {
        let starting = cx.world.resource::<StartingLimits>().clone();
        let gap = cx.theme().space.gap;
        let mut parts = vec![section("LIMITS")];
        if none {
            parts.push(note(
                "None is on, so nothing holds it back. Switch one \
                 on to hold it to that.",
            ));
        }
        for kind in LimitKind::ALL {
            let limit = limits.get(kind).unwrap_or(Limit {
                on: false,
                value: starting.of(kind),
            });
            let why = refused
                .as_ref()
                .filter(|(at, _)| *at == kind)
                .map(|(_, why)| why.clone());
            parts.push(limit_row(
                plugin,
                id.clone(),
                kind,
                limit,
                why,
            ));
        }
        cx.build(column(parts).gap(gap / 2.0))
    })
}

/// The switch of `kind` with its name, its value and unit, and why a
/// value typed was `refused`.
fn limit_row(
    plugin: Entity,
    id: PluginId,
    kind: LimitKind,
    limit: Limit,
    refused: Option<String>,
) -> AnyView {
    let switched_for = id.clone();
    let switch =
        labelled_switch(kind.name(), limit.on, move |world, on| {
            world.choose(
                plugin,
                &switched_for,
                kind,
                Limit { on, ..limit },
            );
        });
    let (per, unit) = match kind.unit() {
        LimitUnit::Bytes => {
            let unit = Unit::of(limit.value);
            (unit.bytes(), Some(unit))
        }
        LimitUnit::Milliseconds | LimitUnit::Count => (1, None),
    };
    let shown = amount(limit.value, per);
    let typed_for = id.clone();
    let left = shown.clone();
    let number =
        field(shown.clone(), String::new(), move |world, typed| {
            match parse(&typed, per) {
                // Left as it was.
                Some(_) if typed.trim() == left => {
                    world.refuse(plugin, None)
                }
                Some(value) => world.choose(
                    plugin,
                    &typed_for,
                    kind,
                    Limit { value, ..limit },
                ),
                None => world.refuse(
                    plugin,
                    Some((kind, "a number of one or more".into())),
                ),
            }
        });
    let after = match unit {
        Some(unit) => {
            let picked_for = id;
            dropdown(
                Unit::ALL
                    .iter()
                    .map(|at| at.name().to_owned())
                    .collect(),
                Unit::ALL
                    .iter()
                    .position(|at| *at == unit)
                    .unwrap_or(0),
                move |world, at| {
                    let Some(&picked) = Unit::ALL.get(at) else {
                        return;
                    };
                    if picked != unit
                        && let Some(value) =
                            parse(&shown, picked.bytes())
                    {
                        world.choose(
                            plugin,
                            &picked_for,
                            kind,
                            Limit { value, ..limit },
                        );
                    }
                },
            )
        }
        None => note(match kind.unit() {
            LimitUnit::Milliseconds => "ms",
            _ => "",
        }),
    };
    let refused = refused.map(|why| label(why).tone(Tone::Critical));
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (gap, side) = (theme.space.gap, theme.space.row);
        let line = row((
            column((switch,)).gap(0.0).grow(1.0).min_width(px(0.0)),
            column((number,)).gap(0.0).width(px(side * 4.0)),
            column((after,)).gap(0.0).width(px(side * 3.0)),
        ))
        .gap(gap)
        .align(AlignItems::Center);
        let mut parts = vec![line.boxed()];
        parts.extend(refused.map(|why| why.boxed()));
        cx.build(column(parts).gap(gap / 2.0))
    })
}
