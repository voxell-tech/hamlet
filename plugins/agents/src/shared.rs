//! What the plugin's parts share: the agents service as they read it
//! (the window's focus, the roster, what waits for the chief, the
//! library), and the views of a message, a change, a card of
//! questions, a confirmation, Markdown, the controls of a limit, a
//! breadcrumb and the rows of a list.

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

pub(crate) use crate::shared::breadcrumb::breadcrumb;
pub(crate) use crate::shared::change::change_view;
pub(crate) use crate::shared::client::{
    AgentsWorldExt, Chiefed, Focused, Heard, Rostered, Waits, plugin,
    show_chat, start,
};
pub(crate) use crate::shared::confirm::{
    CONFIRM, Confirm, confirmation,
};
pub(crate) use crate::shared::entry::entry_view;
pub(crate) use crate::shared::library::library_list;
pub(crate) use crate::shared::limits::{
    name, note, part, setting, whole,
};
pub(crate) use crate::shared::markdown::markdown;
pub(crate) use crate::shared::questions::card;
pub(crate) use crate::shared::rows::{link_row, link_rows};
pub(crate) use crate::shared::signals::{either, moved};
pub(crate) use crate::shared::words::{
    FailureExt, clip, hour, narrowed, short, since, tag_of, titled,
};
