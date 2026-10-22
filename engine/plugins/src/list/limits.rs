//! A plugin's memory on its page: the value in use and where it comes
//! from, a number and a unit to set it, and a way back to what it
//! would be without the person.

use bevy::ecs::entity::Entity;
use bevy::ui::{AlignItems, px};
use bevy_fynix::ViewExt;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row,
};
use hamlet_engine_api::{Limits, PluginId, PluginLimits};
use hamlet_engine_shell::fonts::DisplayText;
use hamlet_engine_shell::icons::Glyph;
use hamlet_engine_shell::style::{
    ButtonStyle, dropdown, field, icon_button,
};
use hamlet_engine_shell::{AnyView, Cx};

use super::ListWorldExt;

/// A plugin's memory as its page shows it.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct MemoryLine {
    /// The bytes in use.
    bytes: u64,
    source: Source,
}

/// Where the value in use comes from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    Engine,
    Plugin,
    Person,
}

impl Source {
    fn text(self) -> &'static str {
        match self {
            Self::Engine => "The engine's",
            Self::Plugin => "Asked for by the plugin",
            Self::Person => "Yours",
        }
    }

    fn tone(self) -> Tone {
        match self {
            Self::Person => Tone::Accent,
            Self::Engine | Self::Plugin => Tone::Dim,
        }
    }
}

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

    /// `bytes` as a number of this unit, to two decimals at most.
    fn amount(self, bytes: u64) -> String {
        let text =
            format!("{:.2}", bytes as f64 / self.bytes() as f64);
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    }

    /// `text`, a number of this unit, in bytes, when it is above
    /// zero. `_` between digits is passed over.
    fn parse(self, text: &str) -> Option<u64> {
        let number =
            text.trim().replace('_', "").parse::<f64>().ok()?;
        let bytes = (number * self.bytes() as f64).round();
        // The cast saturates at u64::MAX.
        (bytes.is_finite() && bytes >= 1.0).then_some(bytes as u64)
    }
}

/// The memory in `used`, from the person's value in `chosen`, the
/// plugin's in `asked`, or the engine's.
pub(super) fn memory_line(
    used: &PluginLimits,
    asked: &Limits,
    chosen: Option<&Limits>,
) -> MemoryLine {
    let source = if chosen.and_then(|c| c.memory).is_some() {
        Source::Person
    } else if asked.memory.is_some() {
        Source::Plugin
    } else {
        Source::Engine
    };
    MemoryLine {
        bytes: used.memory,
        source,
    }
}

/// MEMORY and where its value comes from, over the value as a number
/// and a unit, with RESET when it is the person's, and why a value
/// typed was `refused`, dim while the plugin is not `on`.
pub(super) fn memory_view(
    plugin: Entity,
    id: &PluginId,
    line: MemoryLine,
    refused: Option<String>,
    on: bool,
) -> AnyView {
    let MemoryLine { bytes, source } = line;
    let unit = Unit::of(bytes);
    let amount = unit.amount(bytes);
    let shown = amount.clone();
    let typed_for = id.clone();
    let number =
        field(amount.clone(), String::new(), move |world, typed| {
            match unit.parse(&typed) {
                // Left as it was: not made the person's.
                Some(typed) if unit.amount(typed) == amount => {
                    world.refuse(plugin, None);
                }
                Some(typed) => {
                    world.choose(plugin, &typed_for, Some(typed))
                }
                None => world.refuse(
                    plugin,
                    Some("memory: a number above zero".into()),
                ),
            }
        });
    let picked_for = id.clone();
    let units = dropdown(
        Unit::ALL
            .iter()
            .map(|unit| unit.name().to_owned())
            .collect(),
        Unit::ALL.iter().position(|at| *at == unit).unwrap_or(0),
        move |world, at| {
            let Some(&picked) = Unit::ALL.get(at) else {
                return;
            };
            if picked != unit
                && let Some(bytes) = picked.parse(&shown)
            {
                world.choose(plugin, &picked_for, Some(bytes));
            }
        },
    );
    let reset_for = id.clone();
    let reset = (source == Source::Person).then(|| {
        icon_button(ButtonStyle::Quiet, Glyph::Restore, "RESET")
            .on_activate(move |world| {
                world.choose(plugin, &reset_for, None);
            })
            .boxed()
    });
    let refused = refused.map(|why| label(why).tone(Tone::Critical));
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (gap, size, side) =
            (theme.space.gap, theme.text.small, theme.space.row);
        let name = label("MEMORY")
            .size(size)
            .bold(true)
            .tone(Tone::Faint)
            .wrap(false)
            .tagged(DisplayText);
        let from = label(source.text())
            .size(size)
            .tone(if on { source.tone() } else { Tone::Dim })
            .wrap(false);
        let mut control = vec![
            column((number,))
                .gap(0.0)
                .grow(1.0)
                .min_width(px(0.0))
                .boxed(),
            column((units,)).gap(0.0).width(px(side * 3.0)).boxed(),
        ];
        control.extend(reset);
        let mut parts = vec![
            row((name, from)).gap(gap).boxed(),
            row(control).gap(gap).align(AlignItems::Center).boxed(),
        ];
        parts.extend(refused.map(|why| why.boxed()));
        cx.build(column(parts).gap(gap / 2.0))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_is_shown_and_read_in_its_largest_unit() {
        let shown = |bytes| {
            let unit = Unit::of(bytes);
            format!("{} {}", unit.amount(bytes), unit.name())
        };
        assert_eq!(shown(64 << 20), "64 MB");
        assert_eq!(shown(3 << 29), "1.5 GB");
        assert_eq!(shown(512), "0.5 KB");
        assert_eq!(Unit::Gb.parse("1.5"), Some(3 << 29));
        assert_eq!(Unit::Kb.parse("1_024"), Some(1 << 20));
        assert_eq!(Unit::Mb.parse("0"), None);
        assert_eq!(Unit::Mb.parse("-1"), None);
    }
}
