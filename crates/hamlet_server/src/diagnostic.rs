//! The words for a panic, to log or answer with.

use std::any::Any;

/// The words a handler that panicked gives for it.
pub(crate) fn panicked(panic: &(dyn Any + Send)) -> String {
    let words = panic
        .downcast_ref::<&str>()
        .map(|words| (*words).to_owned())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    format!("the handler panicked: {words}")
}
