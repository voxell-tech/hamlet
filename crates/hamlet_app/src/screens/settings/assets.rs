//! The Assets page: the folder the chief keeps assets in, shown in
//! the system's file browser or chosen again, and whether the realm's
//! houses are built with them. The ASSETS panel shows the same while
//! no asset is selected.

use hamlet_ui::View;
use hamlet_ui::styles::parts;

use super::field;
use crate::assets_folder::{in_realm, kept_at};

pub(super) fn page() -> impl View {
    parts((
        field("Folder", kept_at()),
        field("Use in the realm", in_realm()),
    ))
}
