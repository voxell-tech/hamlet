//! The code of an actor's error.

use hamlet_actor_error::ActorError;

/// The error's `group.code`, as `actor.duplicate_key`.
pub fn code(error: &anyhow::Error) -> String {
    let error = ActorError::extract(error);
    format!("{}.{}", error.group(), error.code())
}
