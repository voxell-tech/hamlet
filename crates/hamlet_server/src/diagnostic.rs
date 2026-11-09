//! The server's diagnostics, on stderr, which is its log.

use std::any::Any;
use std::fmt::Display;

use serde_json::Value;

use crate::credentials::Credentials;

/// The secrets a diagnostic's words may hold.
pub(crate) enum Secrets<'a> {
    /// Any these credentials know.
    Of(&'a Credentials),
    /// None: the words are a fixed text.
    Fixed,
}

/// Writes `words` to the log as one line under the server's name,
/// with `secrets` removed. The line is a JSON string, so a newline or
/// a terminal escape in the words cannot break or forge it.
#[expect(clippy::print_stderr, reason = "stderr is the server's log")]
pub(crate) fn tell(secrets: Secrets<'_>, words: impl Display) {
    let mut line = Value::String(format!("hamlet_server: {words}"));
    if let Secrets::Of(credentials) = secrets {
        credentials.scrub(&mut line);
    }
    eprintln!("{line}");
}

/// The words a handler that panicked gives for it.
pub(crate) fn panicked(panic: &(dyn Any + Send)) -> String {
    let words = panic
        .downcast_ref::<&str>()
        .map(|words| (*words).to_owned())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    format!("the handler panicked: {words}")
}
