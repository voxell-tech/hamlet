//! What the agents' panels share: the agents service as they read it
//! (the window's focus, the roster, what waits for the chief, the
//! library), and the views of a message, a change, a card of
//! questions, a confirmation, Markdown, the controls of a limit, a
//! breadcrumb and the rows of a list. It is a library the panels'
//! plugins are built with, not a plugin of its own: each adds
//! [`plugin`].

mod breadcrumb;
mod change;
mod client;
mod confirm;
mod entry;
mod library;
mod limits;
mod markdown;
mod questions;
mod rows;
mod signals;
mod words;

pub use crate::breadcrumb::breadcrumb;
pub use crate::change::{Changes, change_view};
pub use crate::client::{
    AgentsWorldExt, Asked, Focused, Heard, Rostered, Shelves, Waits,
    plugin, show_chat, start,
};
pub use crate::confirm::{
    CONFIRM, Confirm, Confirming, confirmation,
};
pub use crate::entry::entry_view;
pub use crate::library::library_list;
pub use crate::limits::{name, note, part, setting, whole};
pub use crate::markdown::markdown;
pub use crate::questions::card;
pub use crate::rows::{link_row, link_rows};
pub use crate::signals::{either, moved};
pub use crate::words::{
    FailureExt, clip, hour, narrowed, short, since, tag_of, tail,
    titled,
};
