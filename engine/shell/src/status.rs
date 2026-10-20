//! The status line at the bottom of the window: the workspace shown
//! and the log's last line, each behind its icon.

use bevy::app::{App, Update};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemCondition};
use bevy::ecs::system::{Res, ResMut};
use bevy::ui::{AlignItems, Overflow, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{FrameProps, label, row};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_engine_core::{Log, LogLevel};

use crate::icons::{Glyph, glyph};
use crate::workspace::Workspaces;
use crate::{AnyView, Cx};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<StatusLine>().add_systems(
        Update,
        follow.run_if(
            resource_changed::<Workspaces>
                .or_eager(resource_changed::<Log>),
        ),
    );
}

/// What the status line shows.
#[derive(Resource, Clone, PartialEq, Default)]
struct StatusLine {
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
                    row((
                        fact(
                            Glyph::Folder,
                            status.workspace.clone(),
                            Tone::Dim,
                        ),
                        fact(which, text, tone),
                    ))
                    .gap(gap)
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

/// An icon and the words it stands before, both in `tone`, the words
/// on one line however many lines they came in.
fn fact(which: Glyph, text: String, tone: Tone) -> AnyView {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (side, gap, size) = (
            theme.space.status_icon,
            theme.space.status_icon_gap,
            theme.text.small,
        );
        cx.build(
            row((
                glyph(which, side).tone(tone),
                label(text).size(size).tone(tone).wrap(false),
            ))
            .gap(gap)
            .align(AlignItems::Center),
        )
    })
}

fn follow(
    workspaces: Res<Workspaces>,
    log: Res<Log>,
    mut status: ResMut<StatusLine>,
) {
    let now = StatusLine {
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
