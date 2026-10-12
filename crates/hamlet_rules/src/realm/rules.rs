//! The values votes and the dealings between hamlets are tuned by, as
//! the chief's limits set them.

use std::path::Path;

use hamlet_home::limits::Limits;

/// The values votes and the dealings between hamlets are tuned by.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rules {
    /// The share of a hamlet's agents a vote passes with, as a
    /// fraction.
    pub pass: (u32, u32),
    /// The fewest agents besides the mayor a vote can be held with.
    pub least_voters: usize,
    /// Count a yes only when its evidence was found in the voter's
    /// house.
    pub grounded_only: bool,
    /// Hide the ballots cast so far from each voter.
    pub sealed: bool,
    /// Words in a row of a ballot's evidence that must be found in
    /// the voter's house for it to count as grounded.
    pub ground_words: usize,
    /// Seconds a voter has to cast its ballot.
    pub ballot_secs: u64,
    /// Model steps a voter has to cast its ballot.
    pub ballot_steps: usize,
    /// A vote still open after this long has lapsed.
    pub open_ms: u64,
    /// Wait of an agent whose vote failed before it calls another of
    /// the same kind.
    pub retry_ms: u64,
    /// Messages of its own history a voter is shown.
    pub history_entries: usize,
    /// Characters of each that it is shown.
    pub history_chars: usize,
    /// The most hamlets the home may hold.
    pub most_hamlets: usize,
    /// True when agents of different hamlets may have roads at all.
    pub roads_between_hamlets: bool,
    /// Mayors of other hamlets it takes to ask for a hamlet's
    /// removal (H19).
    pub removal_mayors: usize,
}

impl Rules {
    /// The rules as the chief's limits set them. What the limits do
    /// not name is as it is by default.
    pub fn of(limits: &Limits) -> Self {
        let (votes, hamlets) = (limits.votes, limits.hamlets);
        Rules {
            pass: votes.pass,
            least_voters: votes.least_voters,
            grounded_only: votes.grounded_only,
            sealed: votes.sealed,
            ballot_secs: votes.ballot_minutes.saturating_mul(60),
            open_ms: votes.open_minutes.saturating_mul(60_000),
            retry_ms: votes.retry_minutes.saturating_mul(60_000),
            most_hamlets: hamlets.most,
            roads_between_hamlets: hamlets.roads_between,
            removal_mayors: hamlets.removal_mayors,
            // The asking and checking of a ballot is not the chief's
            // to tune.
            ground_words: 4,
            ballot_steps: 3,
            history_entries: 12,
            history_chars: 320,
        }
    }

    /// The rules of the home at `home`, read from its limits. A
    /// file that cannot be read leaves them as they are by default;
    /// spending is what such a file stops.
    pub fn read(home: &Path) -> Self {
        Limits::load(home).map_or_else(
            |_| Rules::default(),
            |limits| Rules::of(&limits),
        )
    }
}

impl Default for Rules {
    fn default() -> Self {
        Rules::of(&Limits::default())
    }
}
