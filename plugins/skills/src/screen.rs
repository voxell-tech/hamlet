//! The panel: the agent's own skills over the library's.

use bevy_fynix::ViewExt;
use bevy_fynix::views::{FrameProps, column, scroll};
use hamlet_agents_api::convo::of_agent;
use hamlet_sdk::AnyView;
use hamlet_sdk::cards::heading;
use hamlet_sdk::dock::panel;
use hamlet_sdk::icons::Glyph;
use hamlet_sdk::looks::Fills;

use crate::library::library;
use crate::own::own;

/// The panel's contents.
pub(crate) fn screen() -> AnyView {
    panel(of_agent("ITS OWN SKILLS", lists))
}

/// The two lists, for the page of an agent as well.
// One scroll for both lists, so that a long one of its own does not
// push the library out of reach.
pub(crate) fn lists() -> AnyView {
    scroll((column((
        own(),
        heading(Glyph::Book, "THE LIBRARY, SHARED BY ALL"),
        library(),
    ))
    .gap(12.0),))
    .fills_height()
    .boxed()
}
