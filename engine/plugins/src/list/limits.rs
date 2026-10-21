//! A plugin's limits on the plugin list: each one's value in use and
//! where it comes from, a field to set it and a way back to what it
//! would be without the person.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, px};
use bevy_fynix::ViewExt;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row,
};
use hamlet_engine_api::{
    Limit, LimitValue, Limits, PluginId, PluginLimits,
};
use hamlet_engine_core::LogLine;
use hamlet_engine_shell::fonts::DisplayText;
use hamlet_engine_shell::icons::Glyph;
use hamlet_engine_shell::style::{ButtonStyle, field, icon_button};
use hamlet_engine_shell::{AnyView, Cx};

use super::Rows;
use crate::limits::ChosenLimits;

/// One limit of a plugin as the list shows it.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct LimitLine {
    limit: Limit,
    /// The value in use.
    value: LimitValue,
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

/// Every limit in `used`, each from the person's value in `chosen`,
/// the plugin's in `asked`, or the engine's.
pub(super) fn lines(
    used: &PluginLimits,
    asked: &Limits,
    chosen: Option<&Limits>,
) -> Vec<LimitLine> {
    Limit::ALL
        .iter()
        .map(|&limit| {
            let source =
                if chosen.and_then(|c| c.get(limit)).is_some() {
                    Source::Person
                } else if asked.get(limit).is_some() {
                    Source::Plugin
                } else {
                    Source::Engine
                };
            LimitLine {
                limit,
                value: used.get(limit),
                source,
            }
        })
        .collect()
}

/// The limits of the plugin `id`, entity `plugin`, one under
/// another, with why a value typed was `refused`.
pub(super) fn limits_view(
    plugin: Entity,
    id: &PluginId,
    lines: &[LimitLine],
    refused: Option<String>,
) -> AnyView {
    let mut parts = lines
        .iter()
        .map(|line| line_view(plugin, id.clone(), *line))
        .collect::<Vec<_>>();
    if let Some(why) = refused {
        parts.insert(0, label(why).tone(Tone::Critical).boxed());
    }
    AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.gap;
        cx.build(column(parts).gap(gap))
    })
}

/// A limit's name and where its value comes from, over a field
/// with the value, and RESET when it is the person's.
fn line_view(
    plugin: Entity,
    id: PluginId,
    line: LimitLine,
) -> AnyView {
    let LimitLine {
        limit,
        value,
        source,
    } = line;
    let typed_for = id.clone();
    let value_field = field(
        value.to_string(),
        String::new(),
        move |world, typed| {
            match limit.parse(&typed) {
                // Left as it was: not made the person's.
                Some(typed) if typed == value => {
                    world.refuse(plugin, None);
                }
                Some(typed) => {
                    world.choose(
                        plugin,
                        &typed_for,
                        limit,
                        Some(typed),
                    );
                }
                None => {
                    let kind = if limit.whole() {
                        "a whole number"
                    } else {
                        "a number"
                    };
                    let why = format!(
                        "{}: {kind} above zero",
                        limit.name()
                    );
                    world.refuse(plugin, Some(why));
                }
            }
        },
    );
    let reset = (source == Source::Person).then(|| {
        icon_button(ButtonStyle::Quiet, Glyph::Restore, "RESET")
            .on_activate(move |world| {
                world.choose(plugin, &id, limit, None);
            })
            .boxed()
    });
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (gap, size) = (theme.space.gap, theme.text.small);
        let name = label(limit.name())
            .size(size)
            .bold(true)
            .tone(Tone::Faint)
            .wrap(false)
            .tagged(DisplayText);
        let from = label(source.text())
            .size(size)
            .tone(source.tone())
            .wrap(false);
        let mut control = vec![
            column((value_field,))
                .gap(0.0)
                .grow(1.0)
                .min_width(px(0.0))
                .boxed(),
        ];
        control.extend(reset);
        cx.build(
            column((
                row((name, from)).gap(gap),
                row(control).gap(gap).align(AlignItems::Center),
            ))
            .gap(gap / 2.0),
        )
    })
}

/// The plugin list's methods on [`World`].
trait LimitsWorldExt {
    /// Sets `limit` of the plugin `id` to `value`, or back for
    /// `None`, saying on the list why when it cannot be kept.
    fn choose(
        &mut self,
        plugin: Entity,
        id: &PluginId,
        limit: Limit,
        value: Option<LimitValue>,
    );

    /// Shows `why` under the limits of `plugin`, or nothing for
    /// `None`.
    fn refuse(&mut self, plugin: Entity, why: Option<String>);
}

impl LimitsWorldExt for World {
    fn choose(
        &mut self,
        plugin: Entity,
        id: &PluginId,
        limit: Limit,
        value: Option<LimitValue>,
    ) {
        let kept =
            self.resource_mut::<ChosenLimits>().set(id, limit, value);
        if let Err(why) = &kept {
            self.write_message(LogLine::error(
                "plugins",
                why.clone(),
            ));
        }
        self.refuse(plugin, kept.err());
    }

    fn refuse(&mut self, plugin: Entity, why: Option<String>) {
        let refused = why.map(|why| (plugin, why));
        let mut rows = self.resource_mut::<Rows>();
        if rows.refused != refused {
            rows.refused = refused;
        }
    }
}
