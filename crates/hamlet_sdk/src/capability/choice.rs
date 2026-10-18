//! The chief's choice for one capability of one plugin.

/// What the chief chose for a capability.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum Choice {
    /// The plugin may use it.
    Allow,
    /// The plugin may not use it until the chief is asked. Allowed
    /// for the rest of a run once asked, and asked again at the next.
    AskNextTime,
    Deny,
}

impl Choice {
    pub const ALL: [Choice; 3] =
        [Choice::Allow, Choice::AskNextTime, Choice::Deny];

    pub fn words(self) -> &'static str {
        match self {
            Choice::Allow => "Allow",
            Choice::AskNextTime => "Ask next time",
            Choice::Deny => "Don't allow",
        }
    }
}
