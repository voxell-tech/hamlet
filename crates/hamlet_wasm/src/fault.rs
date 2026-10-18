//! Why a plugin could not be loaded or a call did not finish.

use std::fmt;

use wasmtime::Trap;

use crate::limits::OverLimit;

/// A failure of a plugin. The host goes on; the plugin is marked
/// failed.
#[derive(Debug, PartialEq)]
pub enum Fault {
    /// The component could not be compiled or instantiated.
    Load(String),
    /// A call outlasted its time.
    OutOfTime,
    /// A call grew memory or a table past its limit.
    OutOfMemory(String),
    /// A call trapped, or returned what the interface does not allow.
    Trap(String),
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fault::Load(why) => write!(f, "could not load: {why}"),
            Fault::OutOfTime => write!(f, "a call ran out of time"),
            Fault::OutOfMemory(why) => write!(f, "{why}"),
            Fault::Trap(why) => write!(f, "trapped: {why}"),
        }
    }
}

impl Fault {
    pub(crate) fn load(error: wasmtime::Error) -> Self {
        Fault::Load(format!("{error:#}"))
    }

    /// The fault a call's error stands for.
    pub(crate) fn call(error: wasmtime::Error) -> Self {
        if error.downcast_ref::<Trap>() == Some(&Trap::Interrupt) {
            Fault::OutOfTime
        } else if let Some(over) = error.downcast_ref::<OverLimit>() {
            Fault::OutOfMemory(over.to_string())
        } else {
            Fault::Trap(format!("{error:#}"))
        }
    }
}
