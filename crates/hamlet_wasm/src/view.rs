//! A plugin panel's contents, drawn as the native panels are.

use bevy_fynix::views::column;
use hamlet_sdk::{AnyView, dock};

/// The contents of the plugin panel of this dock id.
pub fn panel(_id: &str) -> AnyView {
    dock::panel(column(()))
}
