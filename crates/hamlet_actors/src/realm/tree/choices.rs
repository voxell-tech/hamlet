//! The choices a lead or a chief makes for an agent: the `nickname`
//! and `model_choice` tables, `SetNickname` and `SetModel`.

use std::collections::BTreeSet;

use anyhow::Result;
use hamlet_actor::BindParam;
use hamlet_messages::agent::AgentKind;
use hamlet_messages::realm::choices::{
    ChoiceRefusal, ModelOf, SetModel, SetNickname,
};
use hamlet_messages::realm::{AgentName, HamletId, Named, Refusable};
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::Effort;

use super::super::act::{Act, Decided, Decision};
use super::super::callers::Lead;
use super::super::calls::Made;
use super::super::change::{self, Affected, Rows, Writer, number};
use super::super::documents::{self, Copied, Readers};
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

/// The start of an agent's choice's key, before its number.
const AGENT_KEY: &str = "agent/";

impl Chosen {
    fn key(&self) -> String {
        match self {
            Self::Agent(agent) => format!("{AGENT_KEY}{}", agent.0),
            Self::Hamlet(hamlet) => format!("hamlet/{}", hamlet.0),
        }
    }
}

/// SQL for the key of the choice of the agent whose number is
/// `column`.
pub(in crate::realm) fn agent_key(column: &str) -> String {
    format!("'{AGENT_KEY}' || {column}")
}

/// A model choice as kept: each part absent is the next choice up's.
#[derive(Debug, Default)]
pub(in crate::realm) struct Choice {
    pub model: Option<String>,
    pub effort: Option<Effort>,
    pub fast: Option<bool>,
}

impl Choice {
    /// Each part of this choice, else `up`'s.
    pub fn or(self, up: Self) -> Self {
        Self {
            model: self.model.or(up.model),
            effort: self.effort.or(up.effort),
            fast: self.fast.or(up.fast),
        }
    }
}

/// The choice kept for `of`.
pub(in crate::realm) async fn choice(
    rows: &Rows<'_>,
    of: &Chosen,
) -> Result<Choice> {
    let Some((model, effort, fast)) = rows
        .row::<(Option<String>, Option<String>, Option<bool>)>(
            "SELECT model, effort, fast FROM model_choice WHERE of \
             = ?1",
            vec![BindParam::Text(of.key())],
        )
        .await?
    else {
        return Ok(Choice::default());
    };
    Ok(Choice {
        model,
        effort: effort.map(change::from_case_name).transpose()?,
        fast,
    })
}

#[derive(Debug, PartialEq)]
pub(in crate::realm) enum Change {
    Nickname {
        agent: AgentNumber,
        name: Option<String>,
        by: record::By,
    },
    /// Sets `of`'s model, and its effort and fast tier when
    /// `thinking` is present; a part absent takes the next choice up.
    /// `by` chose the model when it is present.
    Model {
        of: Chosen,
        model: Option<String>,
        thinking: Option<Thinking>,
        by: record::By,
    },
}

/// How hard a model thinks: the parts of a choice only a chief sets.
#[derive(Debug, PartialEq)]
pub(in crate::realm) struct Thinking {
    pub effort: Option<Effort>,
    pub fast: Option<bool>,
}

pub(in crate::realm) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    let shown = match change {
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
                .await?
                > 0
        }
        Change::Nickname {
            agent,
            name: Some(name),
            by,
        } => {
            let was = nickname(&writer.rows(), agent).await?;
            let shown = was.as_ref() != Some(&name);
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
            shown
        }
        Change::Model {
            of,
            model,
            thinking,
            by,
        } => {
            let shown = match of {
                Chosen::Agent(_) => {
                    let was = choice(&writer.rows(), &of).await?;
                    was.model != model
                        || thinking.as_ref().is_some_and(|thinking| {
                            was.effort != thinking.effort
                                || was.fast != thinking.fast
                        })
                }
                Chosen::Hamlet(_) => false,
            };
            let whole = thinking.is_some();
            let Thinking { effort, fast } =
                thinking.unwrap_or(Thinking {
                    effort: None,
                    fast: None,
                });
            let effort = effort
                .map(|effort| change::case_name(&effort))
                .transpose()?;
            writer
                .execute(
                    "INSERT INTO model_choice (of, model, model_by, \
                     effort, fast, at) VALUES (?1, ?2, ?3, ?4, ?5, \
                     ?6) ON CONFLICT (of) DO UPDATE SET model = \
                     excluded.model, model_by = excluded.model_by, \
                     effort = IIF(?7, excluded.effort, effort), \
                     fast = IIF(?7, excluded.fast, fast), at = \
                     excluded.at",
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
                        BindParam::Integer(i64::from(whole)),
                    ],
                )
                .await?;
            let readers = match of {
                Chosen::Agent(agent) => BTreeSet::from([agent]),
                Chosen::Hamlet(hamlet) => {
                    chiefs::hamlet::agents_of(&writer.rows(), &hamlet)
                        .await?
                }
            };
            let copies = readers
                .into_iter()
                .map(|agent| Copied {
                    part: documents::Part::Model,
                    readers: Readers::Agent(agent),
                })
                .collect();
            return Ok(Affected {
                copies,
                ..shown_agents(shown)
            });
        }
    };
    Ok(shown_agents(shown))
}

fn shown_agents(shown: bool) -> Affected {
    if shown {
        Affected::seen([Part::Agents])
    } else {
        Affected::default()
    }
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

/// The agent a choice is for, when `by` may make it: a living agent
/// it leads, a hire when `by` is an agent and of a kind in
/// `chief_takes` when it is a chief.
fn led<By: Lead>(
    by: &By,
    named: &Named,
    lineage: Option<Lineage>,
    chief_takes: &[AgentKind],
) -> Result<AgentName, By::Why<ChoiceRefusal>> {
    let Some(lineage) = lineage else {
        return Err(By::refused(ChoiceRefusal::NoSuchAgent {
            address: named.to_string(),
        }));
    };
    let takes = match by.by() {
        record::By::Chief => chief_takes,
        record::By::Agent(_) => &[AgentKind::Hire],
    };
    let name = lineage.agent.name.clone();
    if !takes.contains(&lineage.agent.kind) {
        return Err(By::refused(ChoiceRefusal::NotAHire {
            name: name.name,
        }));
    }
    by.leads(&lineage)?;
    match lineage.agent.status {
        Status::Ended(left) => {
            Err(By::refused(ChoiceRefusal::Left { hire: name, left }))
        }
        Status::Hiring | Status::Live => Ok(name),
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
    type Output = By::OnAgent<By::Why<ChoiceRefusal>>;
    type Loaded = Option<Lineage>;
    type Done = AgentName;

    async fn load(
        &self,
        by: &By,
        rows: &Rows<'_>,
    ) -> Result<Option<Lineage>> {
        super::named(rows, by, &self.hire).await
    }

    fn decide(
        &self,
        by: &By,
        lineage: Option<Lineage>,
    ) -> Decision<AgentName, Self::Output> {
        let refuse =
            |why| Decision::Answer(Self::Output::refused(why));
        let shown = match led(
            by,
            &self.hire,
            lineage,
            &[AgentKind::Hire, AgentKind::Worker],
        ) {
            Ok(shown) => shown,
            Err(why) => return refuse(why),
        };
        let agent = shown.agent;
        let name = match given_nickname(self.name.as_deref()) {
            Ok(name) => name,
            Err(why) => return refuse(By::refused(why)),
        };
        Decision::Change(
            Decided::new(shown)
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

    fn answer(&self, agent: AgentName, _made: &Made) -> Self::Output {
        By::on_agent(agent)
    }
}

pub(in crate::realm) enum Target {
    Agent {
        hire: Named,
        lineage: Option<Box<Lineage>>,
        /// The model kept for it, and who chose it.
        kept: Option<(String, String)>,
    },
    Hamlet {
        hamlet: HamletId,
        found: bool,
    },
}

impl<By: Lead> Act<By> for SetModel {
    type Output = By::OnAgent<By::Why<ChoiceRefusal>>;
    type Loaded = Target;
    type Done = Option<AgentName>;

    async fn load(&self, by: &By, rows: &Rows<'_>) -> Result<Target> {
        Ok(match &self.of {
            ModelOf::Hire { hire } => {
                let lineage = super::named(rows, by, hire).await?;
                let kept = match &lineage {
                    Some(lineage) => {
                        let of = Chosen::Agent(lineage.agent.agent);
                        rows.row::<(String, String)>(
                            "SELECT model, model_by FROM \
                             model_choice WHERE of = ?1 AND model \
                             IS NOT NULL",
                            vec![BindParam::Text(of.key())],
                        )
                        .await?
                    }
                    None => None,
                };
                Target::Agent {
                    hire: hire.clone(),
                    lineage: lineage.map(Box::new),
                    kept,
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
    ) -> Decision<Option<AgentName>, Self::Output> {
        let refuse = |why| {
            Decision::Answer(Self::Output::refused(By::refused(why)))
        };
        let by_chief = by.by() == record::By::Chief;
        let mut shown = None;
        let mut kept_model = None;
        let of = match target {
            Target::Hamlet { .. } if !by_chief => {
                return refuse(ChoiceRefusal::HamletFromAnAgent);
            }
            Target::Hamlet {
                hamlet,
                found: false,
            } => {
                return refuse(ChoiceRefusal::NoSuchHamlet {
                    id: hamlet,
                });
            }
            Target::Hamlet {
                hamlet,
                found: true,
            } => Chosen::Hamlet(hamlet),
            Target::Agent {
                hire,
                lineage,
                kept,
            } => match led(
                by,
                &hire,
                lineage.map(|lineage| *lineage),
                &[
                    AgentKind::Hand,
                    AgentKind::Hire,
                    AgentKind::Worker,
                ],
            ) {
                Ok(hire)
                    if !by_chief
                        && kept.as_ref().is_some_and(
                            |(_, by)| *by == record::By::Chief.name(),
                        ) =>
                {
                    return refuse(ChoiceRefusal::ChiefChose {
                        hire,
                    });
                }
                Ok(agent) => {
                    let chosen = Chosen::Agent(agent.agent);
                    shown = Some(agent);
                    kept_model = kept.map(|(model, _)| model);
                    chosen
                }
                Err(why) => {
                    return Decision::Answer(Self::Output::refused(
                        why,
                    ));
                }
            },
        };
        let thinking = by_chief.then_some(Thinking {
            effort: self.effort,
            fast: self.fast,
        });
        let mut decided = Decided::new(shown);
        if let Chosen::Agent(agent) = &of
            && kept_model != self.model
        {
            decided = decided.record(record::Entry::Model {
                agent: *agent,
                by: by.by(),
                why: self.why.clone(),
            });
        }
        Decision::Change(decided.change(Change::Model {
            of,
            model: self.model.clone(),
            thinking,
            by: by.by(),
        }))
    }

    fn answer(
        &self,
        agent: Option<AgentName>,
        _made: &Made,
    ) -> Self::Output {
        answered::<By>(agent)
    }
}

/// The answer to a model set for `agent`, or for a hamlet when none.
fn answered<By: Lead>(
    agent: Option<AgentName>,
) -> By::OnAgent<By::Why<ChoiceRefusal>> {
    agent.map_or_else(By::hamlet_model, By::on_agent)
}
