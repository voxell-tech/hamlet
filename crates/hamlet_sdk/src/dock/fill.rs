//! What fills a dock panel.

use bevy::ui::{Overflow, UiRect, px};
use bevy_fynix::views::{BehaviorExt, FrameProps, column};

use crate::feel::Lit;
use crate::looks::Fills;
use crate::platform::{ClipInside, Glow};
use crate::{AnyView, Cx, View};

/// A dock panel's contents.
pub fn panel(view: impl View) -> AnyView {
    lit_panel(view, Lit::default())
}

/// A panel that light falls into as things happen in the hamlet.
pub fn lit_panel(view: impl View, lit: Lit) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let fill = cx.theme().palette.black;
        // The dock frames the area; this is what fills it.
        cx.build(
            column((view,))
                .fills_width()
                .min_height(px(0.0))
                .padding(UiRect::all(px(16.0)))
                .gap(0.0)
                .fill(fill)
                // Overflow is cut at the padding.
                .overflow(Overflow::clip())
                .tagged(ClipInside)
                .tagged(Glow)
                .tagged(lit),
        )
    })
}
