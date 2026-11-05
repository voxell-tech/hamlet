//! The forms of names: scholars' (C10), hamlets' (H36) and versions'.

use super::refusal::Refusal;

/// Names no scholar may take.
const RESERVED: [&str; 4] = ["mayor", "hand", "all", "founder"];

/// Refuses `name` unless it is a scholar's name that is not reserved.
pub(crate) fn check_name(name: &str) -> Result<(), Refusal> {
    let shaped = (1..=24).contains(&name.len())
        && name.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')
        && !name.starts_with('-')
        && !name.ends_with('-');
    if !shaped {
        return Err(Refusal::BadName { name: name.into() });
    }
    if RESERVED.contains(&name) {
        return Err(Refusal::ReservedName { name: name.into() });
    }
    Ok(())
}

/// `name` in a scholar's name's form: words in lowercase, joined by
/// hyphens, so `FelineBiologist` is `feline-biologist` (C10).
pub(crate) fn tidy_name(name: &str) -> String {
    let mut out = String::new();
    let mut previous = None::<char>;
    for c in name.trim().chars() {
        if c.is_ascii_alphabetic() {
            let starts_word = c.is_ascii_uppercase()
                && previous.is_some_and(|p| p.is_ascii_lowercase());
            if starts_word && !out.ends_with('-') {
                out.push('-');
            }
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
        previous = Some(c);
    }
    out.trim_end_matches('-').to_owned()
}

/// Refuses `id` unless it is a hamlet's id.
pub(crate) fn check_hamlet_id(id: &str) -> Result<(), Refusal> {
    let shaped = (1..=32).contains(&id.len())
        && id.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'
        })
        && !id.starts_with('-');
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_s_name_is_put_in_form() {
        assert_eq!(tidy_name("FelineBiologist"), "feline-biologist");
        assert_eq!(tidy_name("  web_scout 2 "), "web-scout");
        assert_eq!(tidy_name("already-fine"), "already-fine");
        assert!(check_name(&tidy_name("Feline Biologist")).is_ok());
    }
}
