//! The core's sessions as a plugin reads them, the chief's message to
//! one, and the archive of past sessions.

pub use hamlet_core::session::{
    Began, Entry, Item, JobFinished, Reply, Rewind, Rewound, Seat,
    SessionKey, SessionOutput, Status, TalksTo, Transcript, Usage,
    UserMessage, archive, ensure, gather, house_at, log_of,
    prompt_of, reply_in,
};
