//! The folders of the disk as a file explorer shows them: one
//! folder's entries at a time (`listing`), and word of a change in
//! the folders shown (`watch`).

mod listing;
mod watch;

pub use listing::{Entry, Kind, list};
pub use watch::Watch;
