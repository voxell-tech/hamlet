//! The forms of names, scholars' (C10) and hamlets' (H36), of
//! hamlets' ids, and of versions.

use hamlet_store::ids::HamletId;
use uuid::Uuid;

use super::refusal::Refusal;

/// The most characters a name has.
pub(crate) const NAME_LENGTH: usize = 40;

/// Trims `name`, and refuses it unless it is one line of one to
/// `NAME_LENGTH` characters with no colon. A name is a label: it may
/// repeat and need not be a word.
pub(crate) fn check_name(name: &str) -> Result<&str, Refusal> {
    let name = name.trim();
    let shaped = (1..=NAME_LENGTH).contains(&name.chars().count())
        && !name.contains(char::is_control)
        && !name.contains(':');
    if shaped {
        Ok(name)
    } else {
        Err(Refusal::BadName { name: name.into() })
    }
}

/// A new hamlet's id, a UUID never given before (H36).
pub(crate) fn new_hamlet_id() -> HamletId {
    HamletId(Uuid::new_v4().to_string())
}

/// The hamlet's id `text` is, in its one written form, if it is one.
pub(crate) fn hamlet_id(text: &str) -> Option<HamletId> {
    Uuid::try_parse(text.trim())
        .ok()
        .map(|id| HamletId(id.hyphenated().to_string()))
}

/// Refuses `version` unless it is a version's name.
pub(crate) fn check_version(version: &str) -> Result<(), Refusal> {
    let shaped = (1..=32).contains(&version.len())
        && version.bytes().all(|b| {
            b.is_ascii_alphanumeric() || b == b'.' || b == b'-'
        })
        && version.as_bytes()[0].is_ascii_alphanumeric()
        && !version.contains("..")
        && !version.ends_with('.');
    if shaped {
        Ok(())
    } else {
        Err(Refusal::BadVersion {
            version: version.into(),
        })
    }
}
