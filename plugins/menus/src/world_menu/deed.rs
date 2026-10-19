//! The question asked before a deed, drawn over the world.

use bevy::math::{Rect, Vec2};
use bevy::ui::px;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, label, popup, row,
};
use bevy_fynix::{ScopedExt, ViewExt};
pub use hamlet_chat::deed::Deed;

use super::close;
use crate::feel::Mood;
use crate::looks::{brass, grim, lettered};
use crate::platform::DisplayText;
use crate::{AnyView, Cx};

/// The question before `deed`, with a warning for each agent that is
/// at work.
pub fn question(deed: Deed, at: Vec2) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let working = deed.working(cx.world);
        let warning = match working.as_slice() {
            [] => frame().boxed(),
            names => label(format!(
                "{} {} working right now. What it is doing will be \
                 cut short.",
                names.join(" and "),
                if names.len() == 1 { "is" } else { "are" }
            ))
            .size(13.0)
            .tone(Tone::Critical)
            .boxed(),
        };
        let done = deed.clone();
        let body = column((
            label(deed.question(cx.world))
                .size(13.0)
                .bold(true)
                .tagged(DisplayText),
            label(deed.outcome(cx.world)).size(13.0).tone(Tone::Dim),
            warning,
            row((
                lettered(deed.verb())
                    .rules(grim)
                    .tagged(Mood::Bad)
                    .on_activate(move |world| {
                        close(world);
                        done.run(world);
                    }),
                lettered("CANCEL").rules(brass).on_activate(close),
            ))
            .gap(8.0),
        ))
        .gap(10.0)
        .width(px(300.0));
        cx.build(
            popup(Rect::from_corners(at, at), (body,))
                .on_dismiss(|world, _| close(world))
                .padding(bevy::ui::UiRect::all(px(14.0))),
        )
    })
}
