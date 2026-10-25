use std::ffi::{OsStr, OsString};
use std::io;

/// One name in a directory, as the system keeps it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Name(OsString);

/// A text that is not a [`Name`] or a [`TreePath`].
#[derive(Debug, thiserror::Error)]
#[error("invalid tree path")]
pub struct BadName;

impl From<BadName> for io::Error {
    fn from(bad: BadName) -> Self {
        Self::new(io::ErrorKind::InvalidInput, bad)
    }
}

impl Name {
    /// `text` as one name: neither empty, `.` nor `..`, and without a
    /// `/` or a NUL. On Windows it also holds none of `\\:<>"|?*`,
    /// ends in no dot or space, and names no device, so the system
    /// keeps the very name given.
    pub fn new(text: &str) -> Result<Self, BadName> {
        if text.is_empty()
            || text == "."
            || text == ".."
            || text.contains(['/', '\0'])
            || cfg!(windows) && !windows_keeps(text)
        {
            return Err(BadName);
        }
        Ok(Self(text.into()))
    }

    /// `text` as one name of a house tree, which also holds no `\`.
    pub fn in_tree(text: &str) -> Result<Self, BadName> {
        if text.contains('\\') {
            return Err(BadName);
        }
        Self::new(text)
    }

    pub(crate) fn listed(name: OsString) -> Self {
        Self(name)
    }

    /// The name as text, where it is UTF-8.
    pub fn to_str(&self) -> Option<&str> {
        self.0.to_str()
    }

    /// The name as text, with what is not UTF-8 replaced.
    pub fn to_string_lossy(&self) -> std::borrow::Cow<'_, str> {
        self.0.to_string_lossy()
    }

    pub(crate) fn as_os(&self) -> &OsStr {
        &self.0
    }
}

/// Whether Windows keeps `text` as written. It strips a last dot or
/// space, reads `:` as a stream, and reads a name whose part before
/// the first dot is a device's, as `nul.txt`, as that device.
fn windows_keeps(text: &str) -> bool {
    let stem = text.split('.').next().unwrap_or(text);
    let stem = stem.trim_end_matches(' ').to_ascii_uppercase();
    let port = ["COM", "LPT"].iter().any(|port| {
        stem.strip_prefix(port).is_some_and(|number| {
            let mut digits = number.chars();
            digits.next().is_some_and(|digit| {
                digit.is_ascii_digit() || "¹²³".contains(digit)
            }) && digits.next().is_none()
        })
    });
    let device = port
        || matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        );
    !device
        && !text.contains(['\\', ':', '<', '>', '"', '|', '?', '*'])
        && !text.ends_with(['.', ' '])
}

/// A relative path in a house tree: one or more names joined by `/`,
/// each one a [`Name::in_tree`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreePath {
    folders: Vec<Name>,
    last: Name,
}

impl TreePath {
    pub fn parse(text: &str) -> Result<Self, BadName> {
        let mut folders = text
            .split('/')
            .map(Name::in_tree)
            .collect::<Result<Vec<_>, _>>()?;
        let last = folders.pop().ok_or(BadName)?;
        Ok(Self { folders, last })
    }

    /// The names before the last, and the last.
    pub fn split_last(&self) -> (&[Name], &Name) {
        (&self.folders, &self.last)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_refuses_dots_a_slash_and_nul() {
        for text in ["", ".", "..", "a/b", "a\0b"] {
            assert!(Name::new(text).is_err(), "{text:?}");
        }
        #[cfg(unix)]
        assert!(Name::new("a\\b").is_ok());
        assert!(Name::new(".hidden").is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn windows_names_refuse_device_stems_and_trailing_dots() {
        for text in [
            "a\\b", "a:b", "a<b", "a>b", "a\"b", "a|b", "a?b", "a*b",
            "end.", "end ", "CON", "nul.txt", "Com1", "lpt9.log",
            "COM¹", "aux .md", "CONIN$",
        ] {
            assert!(Name::new(text).is_err(), "{text:?}");
        }
        for text in ["COM10", "console", "nul_", ".hidden", "a.b"] {
            assert!(Name::new(text).is_ok(), "{text:?}");
        }
    }

    #[test]
    fn a_tree_path_refuses_a_backslash_dots_and_nul() {
        for text in
            ["", "a\\b", "a/../b", "./a", "a//b", "a/", "a\0b"]
        {
            assert!(TreePath::parse(text).is_err(), "{text:?}");
        }
        let path = TreePath::parse("skills/maps/SKILL.md").unwrap();
        let (folders, last) = path.split_last();
        assert_eq!(folders.len(), 2);
        assert_eq!(last.to_str(), Some("SKILL.md"));
    }
}
