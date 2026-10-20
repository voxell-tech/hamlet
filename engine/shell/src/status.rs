//! The status line at the foot of the window: the workspace shown and
//! the log's last line.

use bevy::app::{App, Update};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemCondition};
use bevy::ecs::system::{Res, ResMut};
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{FrameProps, label, row};
use bevy_fynix::{ModifierExt, ViewExt, keyed, resource};
use hamlet_engine_core::{Log, LogLevel};

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

/// The status line, for the foot of the window.
pub fn view() -> AnyView {
    AnyView::new(|cx: &mut Cx| {
        let theme = cx.theme();
        let (height, gap, fill) =
            (theme.space.status, theme.space.gap, theme.colors.bar);
        cx.build(
            keyed(
                resource::<StatusLine, _>(Clone::clone),
                |status| {
                    let (tone, text) = match &status.last {
                        Some((LogLevel::Error, text)) => {
                            (Tone::Critical, text)
                        }
                        Some((LogLevel::Warn, text)) => {
                            (Tone::Accent, text)
                        }
                        Some((LogLevel::Info, text)) => {
                            (Tone::Dim, text)
                        }
                        None => (Tone::Dim, &String::new()),
                    };
                    row((
                        label(status.workspace.clone())
                            .tone(Tone::Faint)
                            .wrap(false),
                        label(text.clone())
                            .tone(tone)
                            .wrap(false)
                            .grown(1.0),
                    ))
                    .boxed()
                },
            )
            .within(
                row(())
                    .height(px(height))
                    .align(AlignItems::Center)
                    .padding(UiRect::axes(px(gap), px(0.0)))
                    .fill(fill),
            ),
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
