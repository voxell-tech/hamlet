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
    /// A call used more than its fuel.
    OutOfFuel,
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
            Fault::OutOfFuel => {
                write!(f, "a call used up its fuel")
            }
            Fault::OutOfMemory(why) => write!(f, "{why}"),
            Fault::Trap(why) => write!(f, "trapped: {why}"),
        }
    }
}

impl Fault {
    /// The error on one line, since it is shown in a row of Settings.
    pub(crate) fn load(error: wasmtime::Error) -> Self {
        let text = format!("{error:#}");
        Fault::Load(
            text.split_whitespace().collect::<Vec<_>>().join(" "),
        )
    }

    /// The fault a call's error stands for.
    pub(crate) fn call(error: wasmtime::Error) -> Self {
        let trap = error.downcast_ref::<Trap>();
        if trap == Some(&Trap::Interrupt) {
            Fault::OutOfTime
        } else if trap == Some(&Trap::OutOfFuel) {
            Fault::OutOfFuel
        } else if let Some(over) = error.downcast_ref::<OverLimit>() {
            Fault::OutOfMemory(over.to_string())
        } else {
            Fault::Trap(format!("{error:#}"))
        }
    }
}
