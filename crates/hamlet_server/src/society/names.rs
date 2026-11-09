//! The forms of names: scholars' (C10), hamlets' (H36) and versions'.

use super::refusal::Refusal;

/// The most characters a scholar's name has.
pub(crate) const SCHOLAR_NAME: usize = 40;

/// The words that begin an address, which no hamlet's id may be.
const ADDRESS_WORDS: [&str; 4] =
    ["hand", "mayor", "scholar", "worker"];

/// Trims `name`, and refuses it unless it is one line of one to
/// `SCHOLAR_NAME` characters with no colon. A name is a label: it may
/// repeat and need not be a word.
pub(crate) fn check_name(name: &str) -> Result<&str, Refusal> {
    let name = name.trim();
    let shaped = (1..=SCHOLAR_NAME).contains(&name.chars().count())
        && !name.contains(char::is_control)
        && !name.contains(':');
    if shaped {
        Ok(name)
    } else {
        Err(Refusal::BadName { name: name.into() })
    }
}

/// Refuses `id` unless it is a hamlet's id.
pub(crate) fn check_hamlet_id(id: &str) -> Result<(), Refusal> {
    let shaped = (1..=32).contains(&id.len())
        && id.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'
        })
        && !id.starts_with('-')
        && !ADDRESS_WORDS.contains(&id);
    if shaped {
        Ok(())
    } else {
        Err(Refusal::BadHamletId { id: id.into() })
    }
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
