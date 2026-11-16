//! The choices a lead or a chief makes for an agent: the `nickname`
//! and `model_choice` tables, `SetNickname` and `SetModel`.

use anyhow::Result;
use hamlet_actor::BindParam;
use hamlet_messages::agent::AgentKind;
use hamlet_messages::realm::choices::{
    ChoiceRefusal, ModelOf, SetModel, SetNickname,
};
use hamlet_messages::realm::{Done, HamletId, Named, Refusable};
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::Effort;

use super::super::act::{Act, Decided, Decision};
use super::super::callers::Lead;
use super::super::calls::Made;
use super::super::change::{self, Affected, Rows, Writer, number};
use super::super::world::Part;
use super::super::{chiefs, record};
use super::{Lineage, Status};

/// The most letters a nickname has.
const NICKNAME_MOST: usize = 24;

/// What a model choice is for: an agent, or a hamlet's agents that
/// choose none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::realm) enum Chosen {
    Agent(AgentNumber),
    Hamlet(HamletId),
}

impl Chosen {
    fn key(&self) -> String {
        match self {
            Self::Agent(agent) => format!("agent/{}", agent.0),
            Self::Hamlet(hamlet) => format!("hamlet/{}", hamlet.0),
        }
    }
}

#[derive(Debug, PartialEq)]
pub(in crate::realm) enum Change {
    Nickname {
        agent: AgentNumber,
        name: Option<String>,
        by: record::By,
    },
    /// Sets the parts of `of`'s choice that are present, and keeps
    /// the others; `by` chose the model when it is present.
    Model {
        of: Chosen,
        model: Option<String>,
        effort: Option<Effort>,
        fast: Option<bool>,
        by: record::By,
    },
}

pub(in crate::realm) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::Nickname {
            agent,
            name: None,
            by: _,
        } => {
            writer
                .execute(
                    "DELETE FROM nickname WHERE agent = ?1",
                    vec![number(agent)],
                )
                .await?;
        }
        Change::Nickname {
            agent,
            name: Some(name),
            by,
        } => {
            writer
                .execute(
                    "INSERT INTO nickname (agent, name, by, at) \
                     VALUES (?1, ?2, ?3, ?4) ON CONFLICT (agent) DO \
                     UPDATE SET name = excluded.name, by = \
                     excluded.by, at = excluded.at",
                    vec![
                        number(agent),
                        BindParam::Text(name),
                        BindParam::Text(by.name()),
                        BindParam::Integer(writer.at().0),
                    ],
                )
                .await?;
        }
        Change::Model {
            of,
            model,
            effort,
            fast,
            by,
        } => {
            let effort = effort
                .map(|effort| change::case_name(&effort))
                .transpose()?;
            writer
                .execute(
                    "INSERT INTO model_choice (of, model, model_by, \
                     effort, fast, at) VALUES (?1, ?2, ?3, ?4, ?5, \
                     ?6) ON CONFLICT (of) DO UPDATE SET model = \
                     COALESCE(excluded.model, model), model_by = \
                     COALESCE(excluded.model_by, model_by), effort \
                     = COALESCE(excluded.effort, effort), fast = \
                     COALESCE(excluded.fast, fast), at = excluded.at",
                    vec![
                        BindParam::Text(of.key()),
                        model
                            .as_ref()
                            .map_or(BindParam::Null, |model| {
                                BindParam::Text(model.clone())
                            }),
                        model.map_or(BindParam::Null, |_| {
                            BindParam::Text(by.name())
                        }),
                        effort
                            .map_or(BindParam::Null, BindParam::Text),
                        fast.map_or(BindParam::Null, |fast| {
                            BindParam::Integer(i64::from(fast))
                        }),
                        BindParam::Integer(writer.at().0),
                    ],
                )
                .await?;
        }
    }
    Ok(Affected::seen([Part::Agents]))
}

pub(in crate::realm) async fn nickname(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Option<String>> {
    rows.value(
        "SELECT name FROM nickname WHERE agent = ?1",
        vec![number(agent)],
    )
    .await
}

/// The agent a choice names, with its name.
pub(in crate::realm) struct Subject {
    lineage: Box<Lineage>,
    name: String,
}

async fn subject<By: Lead>(
    rows: &Rows<'_>,
    by: &By,
    hire: &Named,
) -> Result<Option<Subject>> {
    let Some(lineage) = super::named(rows, by, hire).await? else {
        return Ok(None);
    };
    Ok(Some(Subject {
        name: record::name_of(rows, lineage.agent.agent).await?,
        lineage: Box::new(lineage),
    }))
}

/// The agent a choice is for, when `by` may make it: a living hire
/// it leads.
fn led<By: Lead>(
    by: &By,
    subject: Option<Subject>,
) -> Result<AgentNumber, By::Why<ChoiceRefusal>> {
    let Some(Subject { lineage, name }) = subject else {
        return Err(By::refused(ChoiceRefusal::NoSuchAgent));
    };
    let agent = lineage.agent.agent;
    by.leads(&lineage, agent)?;
    if lineage.agent.kind == AgentKind::Hand {
        return Err(By::refused(ChoiceRefusal::NotAHire { name }));
    }
    match lineage.agent.status {
        Status::Ended(left) => {
            Err(By::refused(ChoiceRefusal::Left {
                hire: agent,
                left,
            }))
        }
        Status::Hiring | Status::Live => Ok(agent),
    }
}

fn given_nickname(
    name: Option<&str>,
) -> Result<Option<String>, ChoiceRefusal> {
    match name.map(str::trim).filter(|name| !name.is_empty()) {
        Some(name)
            if name.chars().count() > NICKNAME_MOST
                || name.contains(char::is_control) =>
        {
            Err(ChoiceRefusal::BadNickname)
        }
        name => Ok(name.map(str::to_owned)),
    }
}

impl<By: Lead> Act<By> for SetNickname {
    type Output = Done<By::Why<ChoiceRefusal>>;
    type Loaded = Option<Subject>;
    type Done = ();

    async fn load(
        &self,
        by: &By,
        rows: &Rows<'_>,
    ) -> Result<Option<Subject>> {
        subject(rows, by, &self.hire).await
    }

    fn decide(
        &self,
        by: &By,
        subject: Option<Subject>,
    ) -> Decision<(), Self::Output> {
        let refuse = |why| Decision::Answer(Done::refused(why));
        let agent = match led(by, subject) {
            Ok(agent) => agent,
            Err(why) => return refuse(why),
        };
        let name = match given_nickname(self.name.as_deref()) {
            Ok(name) => name,
            Err(why) => return refuse(By::refused(why)),
        };
        Decision::Change(
            Decided::new(())
                .change(Change::Nickname {
                    agent,
                    name,
                    by: by.by(),
                })
                .record(record::Entry::Nicknamed {
                    agent,
                    by: by.by(),
                    why: self.why.clone(),
                }),
        )
    }

    fn answer(&self, (): (), _made: &Made) -> Self::Output {
        Done::Done
    }
}

pub(in crate::realm) enum Target {
    Agent {
        subject: Option<Subject>,
        chief_chose: bool,
    },
    Hamlet {
        hamlet: HamletId,
        found: bool,
    },
}

impl<By: Lead> Act<By> for SetModel {
    type Output = Done<By::Why<ChoiceRefusal>>;
    type Loaded = Target;
    type Done = ();

    async fn load(&self, by: &By, rows: &Rows<'_>) -> Result<Target> {
        Ok(match &self.of {
            ModelOf::Hire { hire } => {
                let subject = subject(rows, by, hire).await?;
                let chief_chose = match &subject {
                    Some(subject) => {
                        let of = Chosen::Agent(
                            subject.lineage.agent.agent,
                        );
                        rows.value::<String>(
                            "SELECT model_by FROM model_choice \
                             WHERE of = ?1 AND model_by IS NOT NULL",
                            vec![BindParam::Text(of.key())],
                        )
                        .await?
                            == Some(record::By::Chief.name())
                    }
                    None => false,
                };
                Target::Agent {
                    subject,
                    chief_chose,
                }
            }
            ModelOf::Hamlet { hamlet } => Target::Hamlet {
                hamlet: hamlet.clone(),
                found: chiefs::hamlet(rows, hamlet).await?.is_some(),
            },
        })
    }

    fn decide(
        &self,
        by: &By,
        target: Target,
    ) -> Decision<(), Self::Output> {
        let refuse =
            |why| Decision::Answer(Done::refused(By::refused(why)));
        let by_chief = by.by() == record::By::Chief;
        let of = match target {
            Target::Hamlet { .. } if !by_chief => {
                return refuse(ChoiceRefusal::HamletFromAnAgent);
            }
            Target::Hamlet { found: false, .. } => {
                return refuse(ChoiceRefusal::NoSuchHamlet);
            }
            Target::Hamlet {
                hamlet,
                found: true,
            } => Chosen::Hamlet(hamlet),
            Target::Agent {
                subject,
                chief_chose,
            } => match led(by, subject) {
                Ok(hire) if chief_chose && !by_chief => {
                    return refuse(ChoiceRefusal::ChiefChose {
                        hire,
                    });
                }
                Ok(agent) => Chosen::Agent(agent),
                Err(why) => {
                    return Decision::Answer(Done::refused(why));
                }
            },
        };
        // A lead's tool chooses only the model.
        let (effort, fast) = if by_chief {
            (self.effort, self.fast)
        } else {
            (None, None)
        };
        if self.model.is_none() && effort.is_none() && fast.is_none()
        {
            return Decision::Answer(Done::Done);
        }
        let mut decided = Decided::new(());
        if let (Chosen::Agent(agent), Some(_)) = (&of, &self.model) {
            decided = decided.record(record::Entry::Model {
                agent: *agent,
                by: by.by(),
                why: self.why.clone(),
            });
        }
        Decision::Change(decided.change(Change::Model {
            of,
            model: self.model.clone(),
            effort,
            fast,
            by: by.by(),
        }))
    }

    fn answer(&self, (): (), _made: &Made) -> Self::Output {
        Done::Done
    }
}
