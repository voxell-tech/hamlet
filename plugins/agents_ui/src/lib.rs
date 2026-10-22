//! What the agents' panels share: the agents service as they read it
//! (the window's focus, the roster, what waits for the chief, the
//! library), and the views of a message, a change, a card of
//! questions, a confirmation, Markdown and the controls of a limit.
//! It is a library the panels' plugins are built with, not a plugin
//! of its own.

mod change;
mod client;
mod confirm;
mod entry;
mod library;
mod limits;
mod markdown;
mod questions;
mod signals;
mod words;

pub use self::change::{Changes, change_view};
pub use self::client::{
    Asked, Focused, Heard, Shelves, Waits, ask, hear, init,
    show_chat, start,
};
pub use self::confirm::{
    CONFIRM, Confirm, Confirming, confirm, confirmation,
};
pub use self::entry::{entry_view, retry};
pub use self::library::{library_empty, library_list};
pub use self::limits::{name, part, setting, switch, whole};
pub use self::markdown::markdown;
pub use self::questions::card;
pub use self::signals::{either, moved};
pub use self::words::{
    clip, hour, short, since, tag_of, tail, titled,
};
