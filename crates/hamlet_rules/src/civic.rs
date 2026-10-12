//! The news a chief is told when the government, or the home of
//! agents, has changed.

use crate::vote::Choice;

/// Something that changed who governs or where agents live. A chief
/// is told of each.
#[derive(Clone, Debug, PartialEq)]
pub enum Civic {
    VoteCalled {
        hamlet: String,
        id: u64,
        by: String,
        motion: String,
    },
    Ballot {
        hamlet: String,
        voter: String,
        choice: Option<Choice>,
    },
    VoteClosed {
        hamlet: String,
        id: u64,
        passed: bool,
        summary: String,
    },
    MayorChanged {
        hamlet: String,
        from: String,
        to: String,
    },
    HamletFounded {
        name: String,
        mayor: String,
    },
    HamletRemoved {
        name: String,
    },
    /// A mayor agreed to a road between hamlets. `built` once both
    /// have.
    RoadAgreed {
        a: String,
        b: String,
        built: bool,
    },
    AgentMoved {
        agent: String,
        from: String,
        to: String,
    },
    /// Mayors ask for a hamlet's removal. `ready` when enough have
    /// for the chief to be asked to confirm.
    RemovalAsked {
        hamlet: String,
        by: String,
        ready: bool,
    },
}

impl Civic {
    /// The line a chief is told.
    pub fn notice(&self) -> String {
        match self {
            Civic::VoteCalled {
                hamlet,
                id,
                by,
                motion,
            } => format!(
                "`{by}` called vote {id} in `{hamlet}`: {motion}. \
                 /votes shows it, /overturn {id} reverses it."
            ),
            Civic::Ballot {
                hamlet,
                voter,
                choice,
            } => format!(
                "`{voter}` of `{hamlet}` {}",
                match choice {
                    Some(c) => format!("voted {}", c.word()),
                    None => "did not vote".to_string(),
                }
            ),
            Civic::VoteClosed {
                hamlet,
                id,
                passed,
                summary,
            } => format!(
                "Vote {id} in `{hamlet}` {}: {summary}",
                if *passed { "passed" } else { "failed" }
            ),
            Civic::MayorChanged { hamlet, from, to } => format!(
                "`{to}` is now the mayor of `{hamlet}`; `{from}` \
                 stays as an ordinary agent."
            ),
            Civic::HamletFounded { name, mayor } => format!(
                "The hamlet `{name}` was founded under the mayor \
                 `{mayor}`. /hamlet {name} talks to it."
            ),
            Civic::HamletRemoved { name } => format!(
                "The hamlet `{name}` was removed and its houses \
                 archived."
            ),
            Civic::RoadAgreed { a, b, built: true } => format!(
                "A road now joins `{a}` and `{b}` across hamlets."
            ),
            Civic::RoadAgreed { a, b, built: false } => format!(
                "One mayor agreed to a road between `{a}` and \
                 `{b}`; the other has not yet."
            ),
            Civic::AgentMoved { agent, from, to } => format!(
                "`{agent}` moved from `{from}` to `{to}` with its \
                 house."
            ),
            Civic::RemovalAsked { hamlet, by, ready } => format!(
                "The mayor of `{by}` asks for the hamlet `{hamlet}` \
                 to be removed. {}",
                if *ready {
                    format!(
                        "Enough have asked: /remove-hamlet {hamlet} \
                         confirms it."
                    )
                } else {
                    "Not enough mayors have asked yet.".to_string()
                }
            ),
        }
    }
}
