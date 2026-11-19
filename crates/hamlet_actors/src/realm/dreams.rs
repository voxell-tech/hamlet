//! A chief's dream over a profile or a hire's branch: the `dream`
//! table and its steps. One dream is open in the realm at a time.

use anyhow::{Context as _, Result};
use hamlet_actor::BindParam;
use hamlet_messages::address::Profile;
use hamlet_messages::profile::{
    DreamId, DreamOf, EndDream, PrepareDream,
};
use hamlet_messages::realm::profiles::{
    Dream, DreamAskedRefusal, DreamRefusal, Dreamer,
};
use hamlet_messages::realm::{Done, ProfileId, Refusable};
use hamlet_messages::sender::Client;
use hamlet_messages::workspace::AgentNumber;

use super::act::{Act, Decided, Decision};
use super::change::{self, Affected, Rows, Writer, number};
use super::peer::When;
use super::steps::{self, Cx, Kind, StepId, Steps};
use super::{calls, profiles};

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// A dream over `profile`'s `main`, and `hire`'s branch for a
    /// hire's, with its steps.
    Begin {
        dream: DreamId,
        profile: ProfileId,
        hire: Option<AgentNumber>,
    },
}

pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    let Change::Begin {
        dream,
        profile,
        hire,
    } = change;
    writer
        .execute(
            "INSERT INTO dream (id, profile, hire, at) VALUES (?1, \
             ?2, ?3, ?4)",
            vec![
                BindParam::Integer(i64::from(dream.0)),
                BindParam::Integer(i64::from(profile.0)),
                hire.map_or(BindParam::Null, number),
                BindParam::Integer(writer.at().0),
            ],
        )
        .await?;
    steps::apply(writer, steps::Change::Begin { row: row(dream) })
        .await
}

fn row(dream: DreamId) -> StepId {
    StepId {
        kind: Kind::Dream,
        n: u64::from(dream.0),
    }
}

/// Whether a dream's steps are not done.
async fn is_open(rows: &Rows<'_>) -> Result<bool> {
    Ok(rows
        .value::<i64>(
            "SELECT 1 FROM step WHERE kind = 'dream' AND closed = 0",
            Vec::new(),
        )
        .await?
        .is_some())
}

/// The profile whose learning `dreamer` is, with the hire for a
/// hire's dream.
async fn dreamt_over(
    rows: &Rows<'_>,
    dreamer: &Dreamer,
) -> Result<Result<(ProfileId, Option<AgentNumber>), DreamAskedRefusal>>
{
    Ok(match *dreamer {
        Dreamer::Profile { profile } => {
            profiles::shared(rows, profile)
                .await?
                .map(|profile| (profile, None))
                .map_err(Into::into)
        }
        Dreamer::Hire { hire } => rows
            .value::<u32>(
                "SELECT profile FROM hire WHERE agent = ?1",
                vec![number(hire)],
            )
            .await?
            .map(|profile| (ProfileId(profile), Some(hire)))
            .ok_or_else(|| DreamRefusal::NoSuchHire { hire }.into()),
    })
}

impl Act<Client> for Dream {
    type Output = Done<DreamAskedRefusal>;
    type Loaded = Result<Change, DreamAskedRefusal>;
    type Done = ();

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Self::Loaded> {
        if is_open(rows).await? {
            return Ok(Err(DreamRefusal::Running.into()));
        }
        let dream =
            DreamId(u32::try_from(rows.next_id("dream").await?)?);
        Ok(dreamt_over(rows, &self.of).await?.map(
            |(profile, hire)| Change::Begin {
                dream,
                profile,
                hire,
            },
        ))
    }

    fn decide(
        &self,
        _by: &Client,
        loaded: Self::Loaded,
    ) -> Decision<(), Self::Output> {
        match loaded {
            Ok(begin) => {
                Decision::Change(Decided::new(()).change(begin))
            }
            Err(why) => Decision::Answer(Done::refused(why)),
        }
    }

    fn answer(&self, (): (), _made: &calls::Made) -> Self::Output {
        Done::Done
    }
}

/// A dream's steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    /// The profile checks the dream's folders out.
    Prepare,
    /// The profile removes them.
    End,
}

impl Steps for Step {
    const ALL: &'static [Self] = &[Self::Prepare, Self::End];

    fn name(self) -> &'static str {
        match self {
            Self::Prepare => "prepare",
            Self::End => "end",
        }
    }
}

/// Does `step` of the dream row `n`, and returns the changes
/// committed with its progress.
pub(super) async fn run(
    cx: &Cx<'_>,
    n: u64,
    step: Step,
) -> Result<Vec<change::Change>> {
    let dream = DreamId(u32::try_from(n)?);
    let (profile, hire) = cx
        .rows()?
        .row::<(u32, Option<u32>)>(
            "SELECT profile, hire FROM dream WHERE id = ?1",
            vec![BindParam::Integer(i64::from(dream.0))],
        )
        .await?
        .with_context(|| format!("dream {n} has no row"))?;
    let profile = Profile(ProfileId(profile));
    match step {
        Step::Prepare => {
            let of = match hire {
                Some(agent) => DreamOf::Hire {
                    agent: AgentNumber(agent),
                },
                None => DreamOf::Profile,
            };
            let _alone = cx.realm.purging.write().await;
            cx.peers()
                .call(
                    &profile,
                    &PrepareDream { dream, of },
                    When::AfterAnswer,
                )
                .await?;
        }
        Step::End => {
            let end = EndDream {
                dream,
                done: false,
                summary: String::new(),
            };
            cx.peers()
                .call(&profile, &end, When::AfterAnswer)
                .await?;
        }
    }
    Ok(Vec::new())
}
