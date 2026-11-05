//! Names and office addresses.

use anyhow::Result;
use diesel::SqliteConnection;

use super::{Agent, Archived, DecisionKind, End, Hamlet, Reason};

/// A model's spelling normalized to the record's name spelling.
pub fn tidy_name(name: &str) -> String {
    let mut tidy = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_uppercase()
            && tidy.ends_with(|last: char| last.is_ascii_lowercase())
        {
            tidy.push('-');
        }
        if matches!(c, ' ' | '_' | '-') {
            if !tidy.ends_with('-') {
                tidy.push('-');
            }
        } else {
            tidy.push(c.to_ascii_lowercase());
        }
    }
    tidy.trim_matches('-').into()
}

/// The name alphabet and reserved addresses (C10).
pub fn check_name(name: &str) -> Option<Reason> {
    if !(1..=24).contains(&name.len())
        || !name.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')
    {
        Some(Reason::NameFormat)
    } else if matches!(name, "mayor" | "founder" | "all") {
        Some(Reason::NameReserved { name: name.into() })
    } else {
        None
    }
}

/// Why `name` cannot be a new scholar's, if it cannot: names belong
/// to living scholars (C10).
pub(super) fn available(
    db: &mut SqliteConnection,
    name: &str,
) -> Result<Option<Reason>> {
    if let Some(reason) = check_name(name) {
        return Ok(Some(reason));
    }
    let taken = crate::scholars::named(db, name)?.is_some();
    Ok(taken.then(|| Reason::NameTaken {
        name: name.into(),
        archive: None,
    }))
}

/// The historical fallback when neither a living nor an archived name
/// remains.
#[derive(Clone, Copy, Debug)]
pub enum NameFallback {
    Recorded,
    Created,
    AgentNumber,
}

impl Hamlet {
    /// A living agent addressed by name or by `mayor`.
    pub fn named(&self, name: &str) -> Option<&Agent> {
        self.agents.iter().find(|a| {
            if name == "mayor" {
                a.number == self.mayor
            } else {
                a.name == name
            }
        })
    }

    /// An agent's current office address or name.
    pub fn address_of(&self, number: u64) -> &str {
        if number == self.mayor {
            "mayor"
        } else {
            self.agents
                .iter()
                .find(|a| a.number == number)
                .map_or("", |a| &a.name)
        }
    }

    /// A living or archived name, then the selected historical
    /// fallback.
    pub fn name_of(
        &self,
        number: u64,
        fallback: NameFallback,
    ) -> String {
        if let Some(agent) =
            self.agents.iter().find(|a| a.number == number).or_else(
                || {
                    self.archive
                        .iter()
                        .rev()
                        .find(|a| a.agent.number == number)
                        .map(|a| &a.agent)
                },
            )
        {
            return agent.name.clone();
        }
        let named = |d: &super::Decision| match &d.kind {
            DecisionKind::Hired { agent, name, .. }
            | DecisionKind::Removed { agent, name, .. }
            | DecisionKind::Restored { agent, name, .. }
            | DecisionKind::Erased { agent, name, .. }
            | DecisionKind::MovedIn { agent, name, .. }
            | DecisionKind::MovedOut { agent, name, .. }
                if *agent == number =>
            {
                Some(name.clone())
            }
            _ => None,
        };
        match fallback {
            NameFallback::Recorded => self
                .decisions
                .iter()
                .rev()
                .find_map(named)
                .unwrap_or_else(|| number.to_string()),
            NameFallback::Created => self
                .decisions
                .iter()
                .filter(|d| {
                    matches!(d.kind, DecisionKind::Hired { .. })
                })
                .find_map(named)
                .unwrap_or_else(|| number.to_string()),
            NameFallback::AgentNumber => format!("agent #{number}"),
        }
    }

    /// A road end in words, preserving the office address.
    pub fn end_name(&self, end: End) -> String {
        match end {
            End::Office => "mayor".into(),
            End::Agent(n) => self.name_of(n, NameFallback::Recorded),
        }
    }

    /// A departed agent's refusal (H28).
    pub fn farewell(&self, entry: &Archived) -> String {
        let name = &entry.agent.name;
        let by = entry.by.words();
        let reason = &entry.reason;
        match &entry.merge {
            None => format!(
                "`{name}` was removed from the hamlet by {by} \
                 (\"{reason}\"). Its house is archived and its \
                 roads went with it; only a chief can restore it."
            ),
            Some(merge) => {
                let kept = self
                    .agents
                    .iter()
                    .find(|a| a.number == merge.kept.number)
                    .unwrap_or(&merge.kept);
                let kept = &kept.name;
                format!(
                    "`{name}` was merged into `{kept}` by {by} \
                     (\"{reason}\"). `{kept}` has its notes, skills \
                     and roads; talk to `{kept}`."
                )
            }
        }
    }
}
