//! The core's sessions as a plugin reads them, the chief's message to
//! one, and the archive of past sessions.

pub use hamlet_core::session::{
    Entry, JobFinished, Seat, SessionKey, Status, Transcript, Usage,
    UserMessage, archive, house_at, log_of, prompt_of,
};
