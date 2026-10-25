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
    /// `/` or a NUL.
    pub fn new(text: &str) -> Result<Self, BadName> {
        if text.is_empty()
            || text == "."
            || text == ".."
            || text.contains(['/', '\0'])
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
        assert!(Name::new("a\\b").is_ok());
        assert!(Name::new(".hidden").is_ok());
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
