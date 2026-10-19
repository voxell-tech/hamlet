//! Creating agents, their models and roads (H3, H27, H29), and the
//! chief's deeds on the record.

use anyhow::Result;
use serde::Deserialize;
use serde_json::Value;

use super::{Caller, Deed, outcome};
use crate::harness::Harnesses;
use crate::protocol::Refusal;
use crate::record::{
    self, Actor, Admit, Audience, Change, Decision, DecisionKind,
    RecordOperation, RecordResult, tidy_name,
};
use crate::service::Service;
use crate::{Outcome, society};

#[derive(Deserialize)]
struct Create {
    name: String,
    purpose: String,
    reason: String,
    #[serde(default)]
    roads: Vec<String>,
    model: Option<String>,
    character: Option<String>,
}

#[derive(Deserialize)]
struct SetModel {
    name: String,
    model: String,
    reason: String,
}

/// The arguments of a call, or the words of why they are not.
fn arguments<T: serde::de::DeserializeOwned>(
    tool: &str,
    args: &Value,
) -> std::result::Result<T, Value> {
    serde_json::from_value(args.clone()).map_err(|error| {
        outcome(
            &format!(
                "The arguments of {tool} are not valid: {error}."
            ),
            true,
        )
    })
}

/// The mayor's `create_agent`: an agent with its own house (H27).
pub(super) fn create(
    service: &mut Service,
    caller: &Caller<'_>,
    args: &Value,
) -> Result<Value> {
    let given = match arguments::<Create>("create_agent", args) {
        Ok(given) => given,
        Err(words) => return Ok(words),
    };
    let place = caller.place.agent.clone();
    let at = society::now();
    let outcome = society::once(
        service.home.store(),
        &caller.id,
        "create_agent",
        |db| {
            let operation = RecordOperation {
                hamlet: place.hamlet.clone(),
                actor: society::actor(db, &place)?,
                at,
                reason: given.reason.clone(),
                task: caller.task.clone(),
                change: Change::Admit(Admit {
                    name: given.name.clone(),
                    purpose: given.purpose.clone(),
                    model: given.model.clone(),
                    character: given
                        .character
                        .clone()
                        .unwrap_or_default(),
                    roads: given.roads.clone(),
                }),
            };
            let text = match record::change(db, &operation)? {
                Outcome::Done(RecordResult::Admitted {
                    road_refusals,
                    ..
                }) => {
                    let limits = record::read(db, &place.hamlet)?
                        .map(|r| r.limits.models)
                        .unwrap_or_default();
                    let mut words = vec![format!(
                        "Created `{}`. Give it work with ask_agent.",
                        tidy_name(&given.name)
                    )];
                    if let Some(model) =
                        given.model.as_ref().filter(|m| {
                            !m.is_empty() && !limits.contains(m)
                        })
                    {
                        words.push(format!(
                            "This hamlet may not run `{model}`; the \
                             agent runs the hamlet's own model."
                        ));
                    }
                    for road in road_refusals {
                        words.push(format!(
                            "Road to `{}` refused: {}.",
                            road.peer,
                            road.refusal.text(Audience::Agent)
                        ));
                    }
                    words.join("\n")
                }
                Outcome::Done(other) => {
                    anyhow::bail!("an agent admitted as {other:?}")
                }
                Outcome::Refused(refusal) => {
                    refusal.text(Audience::Agent)
                }
            };
            Ok(outcome(&text, false))
        },
    )?;
    let record = service.home.store().record(&place.hamlet)?;
    if let Some(agent) = record
        .as_ref()
        .and_then(|r| r.named(&tidy_name(&given.name)))
        && let Err(error) =
            service.houses.build(&place.hamlet, agent.number)
    {
        // The agent is recorded; its house is prepared again at the
        // next start.
        eprintln!(
            "hamlet-server: could not prepare the house of agent {} \
             in {}: {error}",
            agent.number, place.hamlet
        );
    }
    Ok(outcome)
}

/// The mayor's `set_model`: an allowed model for an agent, or the
/// hamlet's own (H29).
pub(super) fn set_model(
    service: &mut Service,
    caller: &Caller<'_>,
    args: &Value,
) -> Result<Value> {
    let given = match arguments::<SetModel>("set_model", args) {
        Ok(given) => given,
        Err(words) => return Ok(words),
    };
    let place = caller.place.agent.clone();
    let limits = service.limits_of(&place.hamlet)?;
    let settings = service.settings()?;
    let at = society::now();
    society::once(
        service.home.store(),
        &caller.id,
        "set_model",
        |db| {
            let refuse = |text: String| Ok(outcome(&text, false));
            let Some(mut r) = record::read(db, &place.hamlet)? else {
                return refuse("Only the mayor may do that.".into());
            };
            if let Some(refusal) = r
                .require_mayor(&place)
                .or_else(|| r.may_decide(at, record::Action::Mayor))
            {
                return refuse(refusal.text(Audience::Agent));
            }
            let Some(agent) = r
                .named(&given.name)
                .filter(|a| a.number != r.mayor)
                .cloned()
            else {
                return refuse(format!(
                    "no agent named `{}`",
                    given.name
                ));
            };
            if limits["pinned"]
                .get(&agent.name)
                .is_some_and(|pin| !pin.is_null())
            {
                return refuse(format!(
                    "the chief has set the model of `{}`; only the \
                     chief can change it",
                    agent.name
                ));
            }
            let models = limits["models"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>();
            if !given.model.is_empty()
                && !models.contains(&given.model.as_str())
            {
                return refuse(format!(
                    "this hamlet may not run `{}`. You may choose: \
                     {}",
                    given.model,
                    if models.is_empty() {
                        "the hamlet's own model".into()
                    } else {
                        models.join(", ")
                    }
                ));
            }
            let chosen = (!given.model.is_empty())
                .then(|| given.model.clone());
            for living in &mut r.agents {
                if living.number == agent.number {
                    living.model.clone_from(&chosen);
                }
            }
            let now = r
                .agents
                .iter()
                .find(|a| a.number == agent.number)
                .map(|a| {
                    crate::state::model(a, &limits, &settings, false)
                })
                .unwrap_or_default();
            r.decisions.push(Decision {
                at,
                by: "mayor".into(),
                reason: record::trim(&given.reason).into(),
                kind: DecisionKind::Model {
                    agent: agent.number,
                    model: now,
                },
            });
            record::write(db, &r)?;
            Ok(outcome(
                &format!(
                    "`{}` now runs {}.",
                    agent.name,
                    chosen
                        .as_deref()
                        .unwrap_or("the hamlet's own model")
                ),
                false,
            ))
        },
    )
}

/// Carries out the chief's deed `/<deed> <rest>` on `hamlet`, or
/// `None` when the society does not own it.
pub(crate) async fn deed(
    service: &mut Service,
    harnesses: &mut Harnesses,
    hamlet: &str,
    deed: &str,
    rest: &str,
) -> Result<Option<Deed>> {
    if let Some(result) =
        super::votes::deed(service, harnesses, hamlet, deed, rest)
            .await?
    {
        return Ok(Some(result));
    }
    if let Some(result) =
        crate::realm::deed(service, harnesses, deed, rest).await?
    {
        return Ok(Some(result));
    }
    if let Some(result) =
        super::leaving::deed(service, harnesses, hamlet, deed, rest)
            .await?
    {
        return Ok(Some(result));
    }
    Ok(Some(match deed {
        "merge" => {
            super::merging::deed(service, harnesses, hamlet, rest)
                .await?
        }
        "unshelve" => super::library::unshelve(service, rest)?,
        "road" | "unroad" => road(service, hamlet, deed, rest)?,
        "mayor" => {
            let done =
                mayor(service, harnesses, hamlet, rest).await?;
            society::reconfigure(service, harnesses).await?;
            done
        }
        _ => return Ok(None),
    }))
}

/// A chief's operation on the record of `hamlet`, under an identity
/// of its own.
fn chief_change(
    service: &mut Service,
    hamlet: &str,
    reason: &str,
    change: Change,
) -> Result<Outcome<RecordResult, record::Refusal>> {
    let operation = RecordOperation {
        hamlet: hamlet.into(),
        actor: Actor::Chief(service.chief.clone()),
        at: society::now(),
        reason: reason.into(),
        task: None,
        change,
    };
    let tx = service.home.store().db.transaction()?;
    let outcome = record::change(&tx, &operation)?;
    tx.commit()?;
    Ok(outcome)
}

/// `/road a b [reason]` and `/unroad a b [reason]`.
fn road(
    service: &mut Service,
    hamlet: &str,
    deed: &str,
    rest: &str,
) -> Result<Deed> {
    let mut words = rest.split_whitespace();
    let (Some(a), Some(b)) = (words.next(), words.next()) else {
        return Ok(Err(Refusal::Invalid(format!(
            "Use /{deed} a b [reason]."
        ))));
    };
    let reason = words.collect::<Vec<_>>().join(" ");
    let (a, b) = (a.to_owned(), b.to_owned());
    let laying = deed == "road";
    let change = if laying {
        Change::LayRoad {
            a: a.clone(),
            b: b.clone(),
        }
    } else {
        Change::TakeUpRoad {
            a: a.clone(),
            b: b.clone(),
        }
    };
    Ok(match chief_change(service, hamlet, &reason, change)? {
        Outcome::Done(_) if laying => {
            Ok(format!("Built a road between `{a}` and `{b}`."))
        }
        Outcome::Done(_) => Ok(format!(
            "Took up the road between `{a}` and `{b}`. Nothing is \
             refunded."
        )),
        Outcome::Refused(refusal) => {
            Err(Refusal::Invalid(refusal.text(Audience::Chief)))
        }
    })
}

/// `/mayor <agent> [reason]`: a chief hands the office over.
pub(super) async fn mayor(
    service: &mut Service,
    harnesses: &mut Harnesses,
    hamlet: &str,
    rest: &str,
) -> Result<Deed> {
    let mut words = rest.split_whitespace();
    let name = words.next().unwrap_or_default().to_owned();
    let reason = words.collect::<Vec<_>>().join(" ");
    let reason = if reason.is_empty() {
        "a chief asked".to_owned()
    } else {
        reason
    };
    let Some(r) = service.home.store().record(hamlet)? else {
        return Ok(Err(Refusal::Invalid(
            "Only a chief of this hamlet can name its mayor.".into(),
        )));
    };
    let Some(agent) = r.named(&name).cloned() else {
        return Ok(Err(Refusal::Invalid(format!(
            "no agent named `{name}`"
        ))));
    };
    let from = r.mayor;
    match chief_change(
        service,
        hamlet,
        &reason,
        Change::Seat {
            number: agent.number,
        },
    )? {
        Outcome::Done(_) => {}
        Outcome::Refused(refusal) => {
            return Ok(Err(Refusal::Invalid(
                refusal.text(Audience::Chief),
            )));
        }
    }
    let text = format!(
        "`{}` became mayor in place of `{}`. {reason}",
        agent.name,
        r.name_of(from, record::NameFallback::Recorded)
    );
    crate::chats::tell_chiefs(service, harnesses, hamlet, &text)
        .await?;
    Ok(Ok(format!("`{}` is now the mayor.", agent.name)))
}
