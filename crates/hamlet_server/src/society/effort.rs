//! How hard an agent's model thinks.

use std::fmt;

/// How hard an agent's model thinks (H29a), weakest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Effort {
    Off,
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

impl Effort {
    /// Every effort, weakest first.
    pub const ALL: [Self; 7] = [
        Self::Off,
        Self::Minimal,
        Self::Low,
        Self::Medium,
        Self::High,
        Self::XHigh,
        Self::Max,
    ];

    /// The effort's word, as a chief writes it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Max => "max",
        }
    }

    /// The effort whose word is `text`.
    pub fn named(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|effort| effort.as_str() == text)
    }

    /// The effort as the protocols carry it.
    pub fn wire(self) -> hamlet_protocol::Effort {
        match self {
            Self::Off => hamlet_protocol::Effort::Off,
            Self::Minimal => hamlet_protocol::Effort::Minimal,
            Self::Low => hamlet_protocol::Effort::Low,
            Self::Medium => hamlet_protocol::Effort::Medium,
            Self::High => hamlet_protocol::Effort::High,
            Self::XHigh => hamlet_protocol::Effort::XHigh,
            Self::Max => hamlet_protocol::Effort::Max,
        }
    }

    /// The effort the protocols name.
    pub fn of(wire: hamlet_protocol::Effort) -> Self {
        match wire {
            hamlet_protocol::Effort::Off => Self::Off,
            hamlet_protocol::Effort::Minimal => Self::Minimal,
            hamlet_protocol::Effort::Low => Self::Low,
            hamlet_protocol::Effort::Medium => Self::Medium,
            hamlet_protocol::Effort::High => Self::High,
            hamlet_protocol::Effort::XHigh => Self::XHigh,
            hamlet_protocol::Effort::Max => Self::Max,
        }
    }

    /// The effort among `supported` nearest to this one, looking up
    /// first and then down; this one when `supported` is empty.
    pub fn clamped(self, supported: &[Self]) -> Self {
        let at = self as usize;
        Self::ALL[at..]
            .iter()
            .chain(Self::ALL[..at].iter().rev())
            .find(|e| supported.contains(e))
            .copied()
            .unwrap_or(self)
    }
}

impl fmt::Display for Effort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_effort_a_model_lacks_gives_way_to_the_nearest_it_has() {
        let has = [Effort::Off, Effort::Low, Effort::High];
        assert_eq!(Effort::Max.clamped(&has), Effort::High);
        assert_eq!(Effort::Minimal.clamped(&has), Effort::Low);
        assert_eq!(Effort::Low.clamped(&has), Effort::Low);
        assert_eq!(Effort::High.clamped(&[Effort::Off]), Effort::Off);
    }
}
