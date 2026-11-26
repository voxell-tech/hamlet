//! A chief's dream over a profile or a hire's branch: the `dream`
//! table, its steps and its errand. One dream is open in the realm
//! at a time.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use hamlet_actor::{Action, BindParam, Ctx, Handler};
use hamlet_messages::Message;
use hamlet_messages::address::Profile;
use hamlet_messages::agent::BuiltIn;
use hamlet_messages::profile::{
    DreamId, DreamOf, EndDream, PrepareDream, Prepared,
};
use hamlet_messages::realm::budget::Payer;
use hamlet_messages::realm::profiles::{
    Dream, DreamAskedRefusal, DreamRefusal, Dreamer, Dreamt,
};
use hamlet_messages::realm::workers::{BUILT_IN, Ended};
use hamlet_messages::realm::{Asked, ProfileId, Refusable};
use hamlet_messages::sender::{Client, Hand};
use hamlet_messages::workspace::reach::{Reach, SecretsVersion};
use hamlet_messages::workspace::{AgentNumber, ErrandNumber};
use serde::{Deserialize, Serialize};

use super::act::{Act, Decided, Decision};
use super::change::{self, Affected, Rows, Writer, integer, number};
use super::peer::When;
use super::steps::{self, Cx, Kind, StepId, Steps};
use super::tree::tools::off_for;
use super::workers::errands::{self, ErrandInput, Purpose};
use super::{
    Realm, calls, documents, oversight, profiles, record, tasks,
    versions,
};

/// The most a dream may spend, in millionths of a dollar, as today's
/// default.
const COST: u64 = 200_000;

/// The input of a dream's errand, in today's words.
const DREAM_NOW: &str = "Dream now.";

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// A dream over `profile`'s `main`, and `hire`'s branch for a
    /// hire's, with its steps.
    Begin {
        dream: DreamId,
        profile: ProfileId,
        hire: Option<AgentNumber>,
    },
    /// Errand `errand` dreams in `main`, and `hire` for a hire's
    /// dream, the checkouts under the realm folder.
    Errand {
        dream: DreamId,
        errand: ErrandNumber,
        main: String,
        hire: Option<String>,
    },
    /// The dream is to end, `done` with `summary` or not, when its
    /// end was not decided.
    Ending {
        dream: DreamId,
        done: bool,
        summary: String,
    },
    ErrandEnded {
        errand: ErrandNumber,
        ended: Ended,
    },
}

pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::Begin {
            dream,
            profile,
            hire,
        } => {
            writer
                .execute(
                    "INSERT INTO dream (id, profile, hire, at) \
                     VALUES (?1, ?2, ?3, ?4)",
                    vec![
                        id(dream),
                        BindParam::Integer(i64::from(profile.0)),
                        hire.map_or(BindParam::Null, number),
                        BindParam::Integer(writer.at().0),
                    ],
                )
                .await?;
            steps::apply(
                writer,
                steps::Change::Begin { row: row(dream) },
            )
            .await
        }
        Change::Errand {
            dream,
            errand,
            main,
            hire,
        } => {
            writer
                .execute(
                    "UPDATE dream SET errand = ?2, main_folder = \
                     ?3, hire_folder = ?4 WHERE id = ?1",
                    vec![
                        id(dream),
                        integer(errand.get())?,
                        BindParam::Text(main),
                        hire.map_or(BindParam::Null, BindParam::Text),
                    ],
                )
                .await?;
            Ok(Affected::default())
        }
        Change::Ending {
            dream,
            done,
            summary,
        } => ending(writer, dream, done, summary).await,
        Change::ErrandEnded { errand, ended } => {
            let dream = writer
                .rows()
                .value::<u32>(
                    "SELECT id FROM dream WHERE errand = ?1",
                    vec![integer(errand.get())?],
                )
                .await?
                .with_context(|| {
                    format!("errand {} dreams no dream", errand.get())
                })?;
            let (done, summary) = match ended {
                Ended::Answered { text } => (
                    true,
                    text.lines()
                        .next()
                        .unwrap_or_default()
                        .to_owned(),
                ),
                Ended::Failed { .. } => (false, String::new()),
            };
            ending(writer, DreamId(dream), done, summary).await
        }
    }
}

/// Keeps how the dream ends and begins the step that ends it, once.
async fn ending(
    writer: &Writer<'_>,
    dream: DreamId,
    done: bool,
    summary: String,
) -> Result<Affected> {
    let decided = writer
        .execute(
            "UPDATE dream SET done = ?2, summary = ?3 WHERE id = ?1 \
             AND done IS NULL",
            vec![
                id(dream),
                BindParam::Integer(done.into()),
                BindParam::Text(summary),
            ],
        )
        .await?;
    if decided == 0 {
        return Ok(Affected::default());
    }
    steps::apply(
        writer,
        steps::Change::Begin {
            row: end_row(dream),
        },
    )
    .await
}

fn id(dream: DreamId) -> BindParam {
    BindParam::Integer(i64::from(dream.0))
}

fn row(dream: DreamId) -> StepId {
    StepId {
        kind: Kind::Dream,
        n: u64::from(dream.0),
    }
}

fn end_row(dream: DreamId) -> StepId {
    StepId {
        kind: Kind::DreamEnd,
        n: u64::from(dream.0),
    }
}

/// The dream that has not ended, with its profile.
pub(super) async fn open(
    rows: &Rows<'_>,
) -> Result<Option<(DreamId, ProfileId)>> {
    Ok(rows
        .row::<(u32, u32)>(
            "SELECT id, profile FROM dream WHERE NOT EXISTS (SELECT \
             1 FROM step WHERE step.kind = 'dream_end' AND step.n = \
             dream.id AND step.closed = 1)",
            Vec::new(),
        )
        .await?
        .map(|(dream, profile)| (DreamId(dream), ProfileId(profile))))
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
    type Output = Dreamt;
    type Loaded = Result<(DreamId, Change), DreamAskedRefusal>;
    type Done = DreamId;

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Self::Loaded> {
        if open(rows).await?.is_some() {
            return Ok(Err(DreamRefusal::Running.into()));
        }
        let dream =
            DreamId(u32::try_from(rows.next_id("dream").await?)?);
        Ok(dreamt_over(rows, &self.of).await?.map(
            |(profile, hire)| {
                (
                    dream,
                    Change::Begin {
                        dream,
                        profile,
                        hire,
                    },
                )
            },
        ))
    }

    fn decide(
        &self,
        _by: &Client,
        loaded: Self::Loaded,
    ) -> Decision<DreamId, Self::Output> {
        match loaded {
            Ok((dream, begin)) => {
                Decision::Change(Decided::new(dream).change(begin))
            }
            Err(why) => Decision::Answer(Dreamt::refused(why)),
        }
    }

    fn answers_after(&self, &dream: &DreamId) -> Vec<StepId> {
        vec![row(dream)]
    }

    fn answer(&self, dream: DreamId, _made: &calls::Made) -> Dreamt {
        Dreamt::Began { dream }
    }
}

/// `chief_dream`, which answers once the dream's folders are checked
/// out: `Nothing` when its profile had nothing to dream about, which
/// ends the dream with no errand and no failed step.
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub(super) struct ChiefDream(pub Asked<Dream>);

impl Action for ChiefDream {
    type Output = <Asked<Dream> as Action>::Output;
    const NAME: &'static str = <Asked<Dream> as Action>::NAME;
}

impl Message for ChiefDream {
    type To = <Asked<Dream> as Message>::To;
    type From = <Asked<Dream> as Message>::From;
}

impl Handler<ChiefDream> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        ChiefDream(asked): ChiefDream,
    ) -> Result<Dreamt> {
        let answer = self.handle(ctx.clone(), asked).await?;
        let Dreamt::Began { dream } = answer else {
            return Ok(answer);
        };
        let rows = Rows::outside(&ctx)?;
        let ended_unprepared = rows
            .value::<bool>(
                "SELECT errand IS NULL AND done IS NOT NULL FROM \
                 dream WHERE id = ?1",
                vec![id(dream)],
            )
            .await?
            .unwrap_or(false);
        let nothing = ended_unprepared
            && steps::first_failed(&rows, &[row(dream)])
                .await?
                .is_none();
        Ok(if nothing { Dreamt::Nothing } else { answer })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    /// The profile checks the dream's folders out, and its errand
    /// starts.
    Prepare,
}

impl Steps for Step {
    const ALL: &'static [Self] = &[Self::Prepare];

    fn name(self) -> &'static str {
        match self {
            Self::Prepare => "prepare",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EndStep {
    /// The profile commits the dream's folders when it is done, and
    /// removes them.
    End,
}

impl Steps for EndStep {
    const ALL: &'static [Self] = &[Self::End];

    fn name(self) -> &'static str {
        match self {
            Self::End => "end",
        }
    }
}

/// Does `step` of the dream row `n`, and returns the changes
/// committed with its progress.
pub(super) async fn run(
    cx: &Cx<'_>,
    n: u64,
    Step::Prepare: Step,
) -> Result<Vec<change::Change>> {
    let dream = DreamId(u32::try_from(n)?);
    let (profile, hire) = cx
        .rows()?
        .row::<(u32, Option<u32>)>(
            "SELECT profile, hire FROM dream WHERE id = ?1",
            vec![id(dream)],
        )
        .await?
        .with_context(|| format!("dream {n} has no row"))?;
    let profile = ProfileId(profile);
    let hire = hire.map(AgentNumber);
    let of = match hire {
        Some(agent) => DreamOf::Hire { agent },
        None => DreamOf::Profile,
    };
    let prepared = {
        let _alone = cx.realm.purging(profile).write_owned().await;
        cx.peers()
            .call(
                &Profile(profile),
                &PrepareDream { dream, of },
                When::AfterAnswer,
            )
            .await?
    };
    match prepared {
        Prepared::Nothing => Ok(vec![
            Change::Ending {
                dream,
                done: false,
                summary: String::new(),
            }
            .into(),
        ]),
        Prepared::Ready { main, hire: side } => {
            start(cx, dream, profile, hire, &main, side.as_deref())
                .await
        }
    }
}

/// The changes that start the errand of `dream`, over `profile`'s
/// `main` and `hire`'s branch, checked out at `main` and `side` under
/// the realm folder.
async fn start(
    cx: &Cx<'_>,
    dream: DreamId,
    profile: ProfileId,
    hire: Option<AgentNumber>,
    main: &str,
    side: Option<&str>,
) -> Result<Vec<change::Change>> {
    let rows = cx.rows()?;
    let n = errands::next(&rows).await?;
    let task = tasks::fresh(&rows).await?;
    let label = profiles::label_of(&rows, profile).await?;
    let home = &cx.realm.folder;
    let checkout = home.join(main);
    let (instructions, agent, payer, model) = match hire {
        Some(hire) => {
            let hire_side = home.join(
                side.context("a hire's dream has no hire side")?,
            );
            let who = record::agent_name(&rows, hire).await?.name;
            let payer =
                tasks::own_payer(&rows, hire).await?.with_context(
                    || format!("hire {} pays nothing", hire.0),
                )?;
            let model = documents::model(&rows, hire)
                .await?
                .map(|chosen| chosen.model);
            let instructions = format!(
                "You are dreaming, between your work as `{who}`, a \
                 hire of the scholar `{label}`. Your lodge at {} \
                 holds what you learned as this hire: your memory/ \
                 and skills/. `{label}`'s shared knowledge, its \
                 main, is checked out at {}; every hire of \
                 `{label}` starts from it and takes it in.\n\nBring \
                 home what any hire of `{label}` would want to \
                 know: move or merge it into the checkout's memory/ \
                 and skills/, written for a reader who was not \
                 there. Keep in your lodge what only this hire \
                 needs. Tidy as you go: one fact in one place, \
                 nothing stale, the root memory files short, and a \
                 true `description:` on every memory file. Do not \
                 change CHARACTER.md. When done, answer in one line \
                 what you brought home, or `Nothing to bring home.`",
                hire_side.display(),
                checkout.display()
            );
            (instructions, hire, payer, model)
        }
        None => {
            let instructions = format!(
                "You are tidying the knowledge of the scholar \
                 `{label}`, checked out at {}: its memory/ and \
                 skills/. Merge what is said twice, correct or \
                 remove what is stale, keep the root memory files \
                 short and every memory file's `description:` true, \
                 and make each memory folder's MEMORY.md describe \
                 it. Do not change CHARACTER.md. Answer in one line \
                 what you changed, or `Nothing to tidy.`",
                checkout.display()
            );
            (instructions, n.agent(), Payer::Realm, None)
        }
    };
    let input = ErrandInput {
        instructions: Some(instructions),
        text: DREAM_NOW.to_owned(),
        task,
        model,
        cap: COST,
    };
    Ok(vec![
        tasks::Change::Open {
            task,
            origin: tasks::Origin::Dream,
            agent,
            payer,
        }
        .into(),
        errands::start(Purpose::Dream, input, n).into(),
        Change::Errand {
            dream,
            errand: n,
            main: main.to_owned(),
            hire: side.map(str::to_owned),
        }
        .into(),
    ])
}

/// The changes that end the dream row `n`, whose step failed for
/// good, as not done.
pub(super) fn failed(n: u64) -> Result<Vec<change::Change>> {
    let dream = DreamId(u32::try_from(n)?);
    Ok(vec![
        steps::Change::Close { row: row(dream) }.into(),
        Change::Ending {
            dream,
            done: false,
            summary: String::new(),
        }
        .into(),
    ])
}

/// The changes that end the dream of errand `n`, which ended as
/// `ended`, and its reach.
pub(super) fn ended(
    n: ErrandNumber,
    ended: &Ended,
) -> Vec<change::Change> {
    vec![
        Change::ErrandEnded {
            errand: n,
            ended: ended.clone(),
        }
        .into(),
        versions::Change::WantReach(n.agent()).into(),
    ]
}

pub(super) async fn hire_of(
    rows: &Rows<'_>,
    n: ErrandNumber,
) -> Result<Option<AgentNumber>> {
    Ok(rows
        .value::<Option<u32>>(
            "SELECT hire FROM dream WHERE errand = ?1",
            vec![integer(n.get())?],
        )
        .await?
        .flatten()
        .map(AgentNumber))
}

/// The built-in tools of errand `n`: each one its hire's lead has not
/// turned off.
pub(super) async fn tools(
    rows: &Rows<'_>,
    n: ErrandNumber,
) -> Result<Vec<BuiltIn>> {
    let off = match hire_of(rows, n).await? {
        Some(hire) => off_for(rows, hire).await?,
        None => Vec::new(),
    };
    Ok(BUILT_IN
        .iter()
        .filter(|&&name| !off.iter().any(|tool| tool.name == name))
        .map(|&name| BuiltIn {
            name: name.to_owned(),
        })
        .collect())
}

/// The checkouts errand `n` dreams in while it has not ended: main's
/// and a hire's, under `home`, with the hire.
async fn checkouts(
    rows: &Rows<'_>,
    n: ErrandNumber,
    home: &Path,
) -> Result<Option<(PathBuf, Option<PathBuf>, Option<AgentNumber>)>> {
    Ok(rows
        .row::<(String, Option<String>, Option<u32>)>(
            "SELECT dream.main_folder, dream.hire_folder, \
             dream.hire FROM dream JOIN errand ON errand.n = \
             dream.errand WHERE dream.errand = ?1 AND errand.ended \
             IS NULL",
            vec![integer(n.get())?],
        )
        .await?
        .map(|(main, side, hire)| {
            (
                home.join(main),
                side.map(|side| home.join(side)),
                hire.map(AgentNumber),
            )
        }))
}

/// The folder errand `n` works in, its main's checkout under `home`,
/// while it has not ended.
pub(super) async fn folder(
    rows: &Rows<'_>,
    n: ErrandNumber,
    home: &Path,
) -> Result<Option<PathBuf>> {
    Ok(checkouts(rows, n, home).await?.map(|(main, ..)| main))
}

/// The reach of errand `n` while it has not ended: its dream's
/// checkouts under `home`, with the network of its hire, or of the
/// Hand for a profile's dream.
pub(super) async fn reach(
    rows: &Rows<'_>,
    n: ErrandNumber,
    home: &Path,
    secrets: SecretsVersion,
) -> Result<Reach> {
    let Some((main, side, hire)) = checkouts(rows, n, home).await?
    else {
        return Ok(Reach::Ended);
    };
    let dreamer = hire.unwrap_or(Hand::NUMBER);
    let network =
        oversight::footing(rows, dreamer, secrets).await?.network;
    Ok(Reach::Dream {
        main,
        hire: side,
        network,
        secrets,
    })
}

/// Ends the dream row `n` at its profile, and returns the changes
/// committed with its progress.
pub(super) async fn end(
    cx: &Cx<'_>,
    n: u64,
    EndStep::End: EndStep,
) -> Result<Vec<change::Change>> {
    let dream = DreamId(u32::try_from(n)?);
    let rows = cx.rows()?;
    let (profile, hire, done, summary) = rows
        .row::<(u32, Option<u32>, bool, String)>(
            "SELECT profile, hire, done, summary FROM dream WHERE \
             id = ?1",
            vec![id(dream)],
        )
        .await?
        .with_context(|| format!("dream {n} has no end"))?;
    let hire_name = match hire {
        Some(hire) => Some(
            record::agent_name(&rows, AgentNumber(hire)).await?.name,
        ),
        None => None,
    };
    let end = EndDream {
        dream,
        done,
        summary,
        hire_name,
    };
    let profile = ProfileId(profile);
    cx.peers()
        .call(&Profile(profile), &end, When::AfterAnswer)
        .await?;
    Ok(profiles::holds::reread(cx.peers(), &rows, profile)
        .await
        .into_iter()
        .collect())
}
