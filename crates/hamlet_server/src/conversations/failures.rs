//! Why a turn failed, in words, from the reason its runtime gave.

use hamlet_protocol::client::SetupCause;
use serde_json::Value;

/// A failed turn's cause.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Failure {
    /// What went wrong, as a clause: "it has no model it can use".
    pub words: String,
    pub setup: Option<SetupCause>,
}

/// The words a provider uses when it refuses a call's credentials.
const REFUSED: [&str; 8] = [
    "401",
    "403",
    "unauthorized",
    "api key",
    "api_key",
    "authentication",
    "credential",
    "sign in",
];

/// The failure of a turn whose runtime gave `reason`, and `detail`
/// as the JSON it keeps.
pub(crate) fn of(
    reason: Option<&str>,
    detail: Option<&str>,
) -> Failure {
    let detail = match detail.map(serde_json::from_str::<Value>) {
        Some(Ok(Value::String(detail))) => Some(detail),
        _ => None,
    };
    let plain = |words: &str| Failure {
        words: words.to_owned(),
        setup: None,
    };
    match reason {
        Some("no_model") => Failure {
            words: "it has no model it can use".into(),
            setup: Some(SetupCause::Model),
        },
        Some("model_error") => match detail {
            Some(detail) if is_refused(&detail) => Failure {
                words: "its model's provider refused its credentials"
                    .into(),
                setup: Some(SetupCause::Credentials),
            },
            Some(detail) => Failure {
                words: format!("its model failed: {detail}"),
                setup: None,
            },
            None => plain("its model failed"),
        },
        Some("aborted" | "stopped" | "cancelled") => {
            plain("its turn was stopped")
        }
        Some("overflow") => plain(
            "its conversation is longer than its model can read",
        ),
        Some("session_failed" | "session_closed") => {
            plain("its connection to its model failed")
        }
        _ => plain("its turn failed"),
    }
}

/// Whether a model's error `detail` says its provider refused the
/// credentials.
fn is_refused(detail: &str) -> bool {
    let detail = detail.to_lowercase();
    REFUSED.iter().any(|word| detail.contains(word))
}
