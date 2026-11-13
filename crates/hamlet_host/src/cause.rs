//! Why a message is sent: every message carries its task.

/// The task a message belongs to. A call carries it to the host's
/// log.
#[derive(Clone, Debug)]
pub struct Cause {
    task: String,
}

impl Cause {
    /// The cause of a message that no actor's message caused, such as
    /// a client's call: its task alone.
    pub fn root(task: impl Into<String>) -> Self {
        Self { task: task.into() }
    }

    pub fn task(&self) -> &str {
        &self.task
    }
}
