//! Why a call is made: every call carries its task.

/// The task a call belongs to. The call carries it to the host's
/// log.
#[derive(Clone, Debug)]
pub struct Cause {
    task: String,
}

impl Cause {
    /// The cause of a call that no actor's call caused, such as a
    /// client's call: its task alone.
    pub fn root(task: impl Into<String>) -> Self {
        Self { task: task.into() }
    }

    pub fn task(&self) -> &str {
        &self.task
    }
}
