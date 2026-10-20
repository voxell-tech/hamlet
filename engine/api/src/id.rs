//! The ids of plugins and of what they add.
//!
//! A name is lowercase ASCII: words of letters, digits and `_`, each
//! starting with a letter, joined by `.` (`hamlet.demo`). What a
//! plugin adds is named within it, and its full id is the owner and
//! that name joined by `/` (`hamlet.demo/notes`).

use core::fmt;

use serde::{Deserialize, Serialize};

/// The owner of what the engine adds itself. No plugin may take it.
pub const ENGINE: &str = "engine";

/// The longest a name may be, in bytes.
const LONGEST: usize = 64;

/// An id that is not well formed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdError {
    /// What kind of id it was meant to be.
    pub kind: &'static str,
    pub id: String,
}

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} is not a valid {}", self.id, self.kind)
    }
}

impl std::error::Error for IdError {}

/// Whether `name` follows the grammar of a name.
pub fn is_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= LONGEST
        && name.split('.').all(|word| {
            word.starts_with(|c: char| c.is_ascii_lowercase())
                && word.chars().all(|c| {
                    c.is_ascii_lowercase()
                        || c.is_ascii_digit()
                        || c == '_'
                })
        })
}

/// Whether `id` is an owner and a name joined by `/`.
fn is_owned(id: &str) -> bool {
    id.split_once('/')
        .is_some_and(|(owner, name)| is_name(owner) && is_name(name))
}

macro_rules! string_id {
    ($(#[$doc:meta])* $id:ident, $kind:literal, $valid:expr) => {
        $(#[$doc])*
        #[derive(
            Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug,
            Serialize, Deserialize,
        )]
        #[serde(try_from = "String", into = "String")]
        pub struct $id(String);

        impl $id {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $id {
            type Error = IdError;

            fn try_from(id: String) -> Result<Self, IdError> {
                let valid: fn(&str) -> bool = $valid;
                if valid(&id) {
                    Ok(Self(id))
                } else {
                    Err(IdError { kind: $kind, id })
                }
            }
        }

        impl TryFrom<&str> for $id {
            type Error = IdError;

            fn try_from(id: &str) -> Result<Self, IdError> {
                Self::try_from(id.to_owned())
            }
        }

        impl From<$id> for String {
            fn from(id: $id) -> String {
                id.0
            }
        }

        impl fmt::Display for $id {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

pub(crate) use string_id;

string_id!(
    /// A plugin's id, as its manifest gives it.
    PluginId,
    "plugin id",
    |id| is_name(id) && id != ENGINE
);

string_id!(
    /// A panel's full id: its owner and its name.
    PanelId,
    "panel id",
    is_owned
);

impl PanelId {
    /// The panel `name` of `owner`, a plugin's id or [`ENGINE`].
    pub fn new(owner: &str, name: &str) -> Result<Self, IdError> {
        Self::try_from(format!("{owner}/{name}"))
    }
}

string_id!(
    /// A command's full id: its owner and its name.
    CommandId,
    "command id",
    is_owned
);

impl CommandId {
    /// The command `name` of `owner`, a plugin's id or [`ENGINE`].
    pub fn new(owner: &str, name: &str) -> Result<Self, IdError> {
        Self::try_from(format!("{owner}/{name}"))
    }
}
