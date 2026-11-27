//! A profile a lead makes, waiting at the harbor: the newcomer wait
//! that asks a chief to let it in, or the errand in which the Hand
//! judges it (H45), as the realm's newcomers say.

use anyhow::{Context as _, Result, bail};
use hamlet_actor::BindParam;
use hamlet_messages::realm::ProfileId;
use hamlet_messages::realm::documents::Newcomers;
use hamlet_messages::realm::workers::Ended;
use hamlet_messages::realm::world::{WaitId, WaitKind};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::{AgentNumber, ErrandNumber};

use super::change::{self, Affected, Rows, Writer, integer};
use super::profiles::{self, Arrival, Kind, Status};
use super::waits::{self, Raised};
use super::workers::errands::{self, ErrandInput, Purpose};
use super::{documents, record, tasks};
use crate::cbor;

/// The Hand's part before every verdict.
const ROLE: &str =
    "You are the Hand, deciding for the chiefs which newcomers move \
     into the city. The city is where the realm's profiles live \
     between their work: any hamlet can hire a profile there, and \
     what it learns stays with it. Let a newcomer in when it is a \
     profile the realm will want again, distinct from those the \
     city has. Turn it away when it repeats one of them, is too \
     narrow to keep, or is unclear. A newcomer turned away still \
     works for the hamlet that made it. The chiefs can undo what \
     you decide.";

/// Who is asked to let in a profile an agent makes now.
pub(super) enum Asking {
    /// It moves into the city at once: the Hand made it, or the
    /// realm lets newcomers in.
    Nobody,
    Chief(Wait),
    Hand(Judging),
}

impl Asking {
    /// Who is asked of the profile `by` makes from `character`, under
    /// `newcomers`.
    pub(super) async fn read(
        rows: &Rows<'_>,
        by: AgentNumber,
        newcomers: Newcomers,
        profile: ProfileId,
        character: &str,
    ) -> Result<Self> {
        if by == Hand::NUMBER {
            return Ok(Self::Nobody);
        }
        Ok(match newcomers {
            Newcomers::Open => Self::Nobody,
            Newcomers::Chief => {
                Self::Chief(Wait::read(rows, profile).await?)
            }
            Newcomers::Hand => {
                Self::Hand(Judging::read(rows, by, character).await?)
            }
        })
    }

    pub(super) fn arrival(&self) -> Arrival {
        match self {
            Self::Nobody => Arrival::Housed,
            Self::Chief(_) | Self::Hand(_) => Arrival::Harbor,
        }
    }

    /// The changes that ask of `profile`, labelled `label`, which
    /// `by` made.
    pub(super) fn changes(
        self,
        by: AgentNumber,
        profile: ProfileId,
        label: String,
    ) -> Vec<change::Change> {
        match self {
            Self::Nobody => Vec::new(),
            Self::Chief(wait) => vec![wait.raise(by, label).into()],
            Self::Hand(judging) => judging.start(profile, &label),
        }
    }
}

/// The wait a profile at the harbor raises, for a chief to let it in.
pub(super) struct Wait {
    wait: WaitId,
    detail: Vec<u8>,
}

impl Wait {
    async fn read(
        rows: &Rows<'_>,
        profile: ProfileId,
    ) -> Result<Self> {
        Ok(Self {
            wait: waits::next(rows).await?,
            detail: cbor(&WaitKind::Newcomer { profile })?,
        })
    }

    fn raise(self, by: AgentNumber, label: String) -> waits::Change {
        waits::Change::Raise {
            wait: self.wait,
            raised: Raised {
                agent: by,
                kind: waits::Kind::Newcomer,
                task: None,
                conversation: None,
                call: None,
                subject: label,
                detail: self.detail,
            },
        }
    }
}

/// The open newcomer waits about `profile`.
pub(super) async fn waits_about(
    rows: &Rows<'_>,
    profile: ProfileId,
) -> Result<Vec<WaitId>> {
    let mut about = Vec::new();
    for (wait, detail) in rows
        .rows::<(u64, Vec<u8>)>(
            "SELECT id, detail FROM wait WHERE kind = 'newcomer' \
             AND closed_at IS NULL ORDER BY id",
            Vec::new(),
        )
        .await?
    {
        if of(&detail)? == profile {
            about.push(WaitId(wait));
        }
    }
    Ok(about)
}

/// The profile a newcomer wait's `detail` is about.
pub(super) fn of(detail: &[u8]) -> Result<ProfileId> {
    match ciborium::from_reader(detail)? {
        WaitKind::Newcomer { profile } => Ok(profile),
        other => bail!("a newcomer wait keeps {other:?}"),
    }
}

/// The errand in which the Hand judges a profile at the harbor, paid
/// by the Hand's standing task on the Hand's model.
pub(super) struct Judging {
    n: ErrandNumber,
    input: ErrandInput,
    /// Who made it, as its question names them.
    maker: String,
    character: String,
}

impl Judging {
    async fn read(
        rows: &Rows<'_>,
        by: AgentNumber,
        character: &str,
    ) -> Result<Self> {
        let living = rows
            .rows::<(u32, String)>(
                "SELECT id, name FROM profile WHERE kind = 'shared' \
                 AND status = 'living' AND arrival = 'housed' ORDER \
                 BY id",
                Vec::new(),
            )
            .await?
            .into_iter()
            .map(|(id, name)| {
                format!("- {}", profiles::label(ProfileId(id), &name))
            })
            .collect::<Vec<_>>();
        let city = if living.is_empty() {
            "(nobody yet)".to_owned()
        } else {
            living.join("\n")
        };
        Ok(Self {
            n: errands::next(rows).await?,
            input: ErrandInput {
                instructions: Some(format!(
                    "{ROLE}\n\n# The city\n{city}"
                )),
                text: String::new(),
                task: tasks::standing(rows, Hand::NUMBER).await?,
                model: documents::model(rows, Hand::NUMBER)
                    .await?
                    .map(|chosen| chosen.model),
                cap: errands::UNCAPPED,
            },
            maker: record::agent_name(rows, by).await?.name,
            character: character.trim().to_owned(),
        })
    }

    fn start(
        self,
        profile: ProfileId,
        label: &str,
    ) -> Vec<change::Change> {
        let Self {
            n,
            mut input,
            maker,
            character,
        } = self;
        input.text = format!(
            "{maker} made the profile `{label}`, whose character \
             is:\n\n{character}\n\nShould it move into the city? \
             Answer ACCEPT or TURN AWAY on the first line, then say \
             why in one line."
        );
        vec![
            errands::start(Purpose::Newcomer, input, n).into(),
            Change::Judging { errand: n, profile }.into(),
        ]
    }
}

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// Errand `errand` judges `profile`.
    Judging {
        errand: ErrandNumber,
        profile: ProfileId,
    },
    /// The Hand's errand `errand` ended as `ended`: its verdict lets
    /// the newcomer in or turns it away, unless the newcomer is gone
    /// or already where the verdict puts it.
    Judged { errand: ErrandNumber, ended: Ended },
}

pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::Judging { errand, profile } => {
            writer
                .execute(
                    "INSERT INTO newcomer (errand, profile) VALUES \
                     (?1, ?2)",
                    vec![
                        integer(errand.get())?,
                        BindParam::Integer(profile.0.into()),
                    ],
                )
                .await?;
            Ok(Affected::default())
        }
        Change::Judged { errand, ended } => {
            judged(writer, errand, ended).await
        }
    }
}

/// The changes the end of the Hand's errand `n` makes.
pub(super) fn ended(
    n: ErrandNumber,
    ended: &Ended,
) -> Vec<change::Change> {
    vec![
        Change::Judged {
            errand: n,
            ended: ended.clone(),
        }
        .into(),
    ]
}

/// A verdict's first line.
enum Verdict {
    Accept,
    TurnAway,
}

async fn judged(
    writer: &Writer<'_>,
    errand: ErrandNumber,
    ended: Ended,
) -> Result<Affected> {
    let Ended::Answered { text } = ended else {
        return Ok(Affected::default());
    };
    let mut lines = text.lines();
    let first =
        lines.next().unwrap_or_default().trim().to_uppercase();
    let verdict = if first.starts_with("ACCEPT") {
        Verdict::Accept
    } else if first.starts_with("TURN AWAY") {
        Verdict::TurnAway
    } else {
        return Ok(Affected::default());
    };
    let why = lines.collect::<Vec<_>>().join(" ").trim().to_owned();
    let why = (!why.is_empty()).then_some(why);
    let rows = writer.rows();
    let profile = ProfileId(
        rows.value::<u32>(
            "SELECT profile FROM newcomer WHERE errand = ?1",
            vec![integer(errand.get())?],
        )
        .await?
        .with_context(|| {
            format!("errand {} judges no newcomer", errand.get())
        })?,
    );
    let Some(row) = profiles::row(&rows, profile).await? else {
        return Ok(Affected::default());
    };
    if row.kind != Kind::Shared || row.status != Status::Living {
        return Ok(Affected::default());
    }
    let by = record::By::Agent(Hand::NUMBER);
    let (arrival, house, entry) = match verdict {
        Verdict::Accept if row.arrival != Arrival::Housed => {
            let taken =
                profiles::houses(&rows, Some(profile)).await?;
            (
                Arrival::Housed,
                Some(profiles::free_spot(&taken)?),
                record::Entry::Welcomed {
                    by: by.clone(),
                    why,
                },
            )
        }
        Verdict::TurnAway if row.arrival != Arrival::Turned => (
            Arrival::Turned,
            None,
            record::Entry::TurnedAway {
                by: by.clone(),
                why,
            },
        ),
        Verdict::Accept | Verdict::TurnAway => {
            return Ok(Affected::default());
        }
    };
    let placed = profiles::Change::Placed {
        profile,
        arrival,
        house,
        by,
    };
    let affected = profiles::apply(writer, placed).await?;
    record::write(writer, vec![entry]).await?;
    Ok(affected)
}
