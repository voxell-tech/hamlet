//! How panels are laid out in the dock, as a workspace keeps it.

use serde::{Deserialize, Serialize};

use crate::id::PanelId;

/// A tree of areas, each holding panels as tabs.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Layout {
    /// One area, its panels as tabs and the one at `shown` in front.
    Tabs { panels: Vec<PanelId>, shown: usize },
    /// Two layouts along `axis`, `share` of the space to `first`.
    Split {
        axis: Axis,
        share: f32,
        first: Box<Layout>,
        second: Box<Layout>,
    },
}

impl Layout {
    /// One area with nothing in it.
    pub fn empty() -> Self {
        Self::Tabs {
            panels: Vec::new(),
            shown: 0,
        }
    }
}

#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize,
)]
pub enum Axis {
    /// Side by side, `first` on the left.
    Horizontal,
    /// One above the other, `first` on top.
    Vertical,
}
