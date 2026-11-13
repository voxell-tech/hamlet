//! Why a message is sent: every message carries its task, and a
//! message sent while another is handled names that one.

/// The task a message belongs to, and the message that caused it. A
/// call carries it to the host's log; a send keeps it in its row.
#[derive(Clone, Debug)]
pub struct Cause {
    task: String,
    cause: Option<String>,
}

impl Cause {
    /// The cause of a message that no actor's message caused, such as
    /// a client's call: its task alone.
    pub fn root(task: impl Into<String>) -> Self {
        Self {
            task: task.into(),
            cause: None,
        }
    }

    pub fn task(&self) -> &str {
        &self.task
    }

    /// The id of the message that caused this one, `None` for a root.
    pub fn cause(&self) -> Option<&str> {
        self.cause.as_deref()
    }
}
