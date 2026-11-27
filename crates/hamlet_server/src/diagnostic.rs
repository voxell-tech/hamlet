//! The server's diagnostics, on stderr, which is its log.

use std::fmt::Display;

use serde_json::Value;

use crate::credentials::Credentials;

/// Writes `words` to the log as one line under the server's name,
/// with the secrets `credentials` know removed. The line is a JSON
/// string, so a newline or a terminal escape in the words cannot
/// break or forge it.
#[expect(clippy::print_stderr, reason = "stderr is the server's log")]
pub(crate) fn tell(credentials: &Credentials, words: impl Display) {
    let mut line = Value::String(format!("hamlet_server: {words}"));
    credentials.scrub(&mut line);
    eprintln!("{line}");
}
