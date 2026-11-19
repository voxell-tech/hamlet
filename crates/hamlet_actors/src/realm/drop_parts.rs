//! The parts an ended hire leaves in the actors that keep them: the
//! save under way in its profile, and the offers under way in the
//! library. A step row drops them once the hire ends.

use anyhow::Result;
use hamlet_messages::address::{Library, Profile};
use hamlet_messages::library::DropOfferParts;
use hamlet_messages::profile::DropSaveParts;
use hamlet_messages::realm::ProfileId;
use hamlet_messages::workspace::AgentNumber;

use super::change::{self, number};
use super::hire;
use super::peer::When;
use super::steps::{self, Cx, Kind, StepId, Steps};

/// The row that drops what `agent` leaves.
pub(super) fn row(agent: AgentNumber) -> StepId {
    StepId {
        kind: Kind::DropParts,
        n: u64::from(agent.0),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    /// Its profile drops its save under way.
    Profile,
    /// The library drops its offers under way.
    Library,
}

impl Steps for Step {
    const ALL: &'static [Self] = &[Self::Profile, Self::Library];

    fn name(self) -> &'static str {
        match self {
            Self::Profile => "profile",
            Self::Library => "library",
        }
    }
}

/// Does `step` of the row of the ended agent `n`, and returns the
/// changes committed with its progress.
pub(super) async fn run(
    cx: &Cx<'_>,
    n: u64,
    step: Step,
) -> Result<Vec<change::Change>> {
    let agent = AgentNumber(u32::try_from(n)?);
    match step {
        Step::Profile => {
            let rows = cx.rows()?;
            // A hire ended before its Profile step was filled from no
            // profile and saved nothing; its profile may have no
            // actor.
            let filled = steps::row(&rows, hire::row(agent))
                .await?
                .is_none_or(|row| row.done.is_some());
            let profile = rows
                .value::<u32>(
                    "SELECT profile FROM hire WHERE agent = ?1",
                    vec![number(agent)],
                )
                .await?;
            if let Some(profile) = profile.filter(|_| filled) {
                cx.peers()
                    .call(
                        &Profile(ProfileId(profile)),
                        &DropSaveParts { agent },
                        When::AfterAnswer,
                    )
                    .await?;
            }
        }
        Step::Library => {
            cx.peers()
                .call(
                    &Library,
                    &DropOfferParts { agent },
                    When::AfterAnswer,
                )
                .await?;
        }
    }
    Ok(Vec::new())
}
