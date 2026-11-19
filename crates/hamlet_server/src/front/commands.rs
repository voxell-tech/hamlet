//! A chief's slash commands, each to one act of one actor or to the
//! door's own words.

mod parse;

use std::fmt::Display;

use anyhow::Result;
use hamlet_messages::address::{Agent, Realm};
use hamlet_messages::agent::chat::{
    Count, NewSession, ReadHistory, Reflect, Reflecting, Resume,
    ResumeAnswer, Retry, Stop,
};
use hamlet_messages::agent::copies::Price;
use hamlet_messages::realm::budget::{Payer, ResetBudget};
use hamlet_messages::realm::choices::{
    ModelOf, SetModel, SetNickname,
};
use hamlet_messages::realm::client::{Answer, Say, SayAnswer};
use hamlet_messages::realm::documents::{
    CardSetting, EditCatalogue, EditCharter, ImageLimits,
};
use hamlet_messages::realm::oversight::{Permission, Revoke};
use hamlet_messages::realm::workers::{SetAllowance, Started};
use hamlet_messages::realm::world::Status;
use hamlet_messages::realm::{
    Asked, ChiefRequest, Done, End, FoundAnswer, HireAnswer, LayRoad,
    Named, Seat, SetTools, TakeUpRoad, ToolName,
};
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::{self as protocol, Choice, RequestId};

use self::parse::{Act, Part, Target, parse};
use super::call::Calls;
use super::host::HAND;
use super::view::{self, View};
use super::words;
use crate::oversight::gate;
use crate::society::agents::Effort as Level;
use crate::society::refusal::Refusal;

/// What a command came to.
pub(crate) struct Outcome {
    /// The words of the command's reply, or of its refusal.
    pub(crate) reply: Result<String, Refusal>,
    /// The agent whose chat its watchers read again, as after
    /// `/new`.
    #[expect(dead_code, reason = "the door's hub reads it")]
    pub(crate) reread: Option<AgentNumber>,
}

impl Outcome {
    fn said(reply: Result<String, Refusal>) -> Self {
        Self {
            reply,
            reread: None,
        }
    }
}

pub(crate) async fn run(
    calls: &Calls,
    view: &View,
    say: &protocol::Say,
) -> Result<Outcome> {
    if !say.images.is_empty() || !say.files.is_empty() {
        return Ok(Outcome::said(Err(Refusal::CommandAttachments)));
    }
    let Ok(speaker) = u32::try_from(say.to.0).map(AgentNumber) else {
        return Ok(Outcome::said(Err(Refusal::NoOne {
            address: say.to.0.to_string(),
        })));
    };
    let here = view
        .agent(speaker)
        .filter(|seen| seen.worker.is_none())
        .and_then(|seen| seen.hamlet.clone());
    match parse(&say.text, here.as_ref()) {
        Ok(act) => perform(calls, view, &say.id, speaker, act).await,
        Err(refusal) => Ok(Outcome::said(Err(refusal))),
    }
}

/// Makes the edit `edit` asks for.
pub(crate) async fn edit(
    calls: &Calls,
    _view: &View,
    edit: &protocol::Edit,
) -> Result<Outcome> {
    let reply = match edit {
        protocol::Edit::Charter { id, text } => {
            let text =
                text.clone().filter(|text| !text.trim().is_empty());
            let answer = ask(calls, id, EditCharter { text }).await?;
            done(answer, || words::CHARTER_SAVED.into())
        }
        protocol::Edit::Catalogue { id, model, set } => {
            let act = EditCatalogue {
                model: model.clone(),
                set: card(set),
            };
            done(ask(calls, id, act).await?, || {
                words::CATALOGUE_SAVED.into()
            })
        }
        _ => Err(Refusal::Unsupported {
            what: "`edit`".into(),
        }),
    };
    Ok(Outcome::said(reply))
}

/// The parts of a model's card a catalogue edit sets.
fn card(set: &protocol::EditCatalogueSet) -> Vec<CardSetting> {
    let images =
        set.images.clone().map(|images| CardSetting::Images {
            images: images.map(|takes| ImageLimits {
                max_bytes: takes.max_bytes,
                max_height: takes.max_height,
                max_width: takes.max_width,
                per_line: takes.per_line,
            }),
        });
    let note =
        set.note.clone().map(|note| CardSetting::Note { note });
    let price = set.price.clone().map(|price| CardSetting::Price {
        price: price.map(|price| Price {
            input: price.input,
            output: price.output,
            cache_read: 0.0,
            cache_write: 0.0,
        }),
    });
    images.into_iter().chain(note).chain(price).collect()
}

/// The agent `target` names as the realm shows it, or the refusal
/// of an address that names none.
fn number(
    view: &View,
    target: &Target,
) -> Result<AgentNumber, Refusal> {
    let found = match &target.named {
        Named::Hand => Some(HAND),
        Named::MayorOf { hamlet } => view
            .world
            .hamlets
            .iter()
            .find(|place| place.hamlet == *hamlet)
            .map(|place| place.mayor),
        Named::Agent { agent } => {
            view.agent(*agent).map(|seen| seen.agent)
        }
        _ => None,
    };
    found.ok_or_else(|| Refusal::NoOne {
        address: target.text.clone(),
    })
}

/// A refusal of an actor, in its own words.
fn refused(why: impl Display) -> Refusal {
    Refusal::Usage {
        words: why.to_string(),
    }
}

/// The words of an act that answers only that it was done.
fn done<W: Display>(
    answer: Done<W>,
    words: impl FnOnce() -> String,
) -> Result<String, Refusal> {
    match answer {
        Done::Done => Ok(words()),
        Done::Refused { why } => Err(refused(why)),
    }
}

/// Asks the realm for the chief's `act` under the command's id.
async fn ask<T: ChiefRequest>(
    calls: &Calls,
    id: &RequestId,
    act: T,
) -> Result<T::Answer> {
    calls
        .call(
            &Realm,
            &Asked {
                id: id.clone(),
                act,
            },
        )
        .await
}

/// The id of the line `/to` gives another agent, the same for each
/// repeat of the command.
fn line_id(id: &RequestId) -> RequestId {
    let uuid = uuid::Uuid::parse_str(id.as_str())
        .map(|uuid| {
            uuid.as_u128() ^ u128::from(u16::from_be_bytes(*b"to"))
        })
        .unwrap_or_default();
    RequestId::try_from(uuid::Uuid::from_u128(uuid).to_string())
        .unwrap_or_else(|_| RequestId::new())
}

async fn perform(
    calls: &Calls,
    view: &View,
    id: &RequestId,
    speaker: AgentNumber,
    act: Act,
) -> Result<Outcome> {
    let to = protocol::AgentId(u64::from(speaker.0));
    let reply = match act {
        Act::Choose => Ok(words::CHOOSE.into()),
        Act::New => {
            let began = calls
                .call(
                    &Agent(speaker),
                    &NewSession(protocol::New { id: id.clone(), to }),
                )
                .await?;
            return Ok(Outcome {
                reply: Ok(words::began(began.number)),
                reread: Some(speaker),
            });
        }
        Act::Sessions => {
            let history =
                calls.call(&Agent(speaker), &ReadHistory).await?;
            Ok(words::sessions(
                history.sessions.iter().map(|session| session.number),
            ))
        }
        Act::Resume(number) => {
            let resume = protocol::Resume {
                id: id.clone(),
                number,
                to,
            };
            return Ok(
                match calls
                    .call(&Agent(speaker), &Resume(resume))
                    .await?
                {
                    ResumeAnswer::Back { number } => Outcome {
                        reply: Ok(words::back(number)),
                        reread: Some(speaker),
                    },
                    ResumeAnswer::Refused { why } => {
                        Outcome::said(Err(refused(why)))
                    }
                },
            );
        }
        Act::Retry => {
            let retry = protocol::Retry { id: id.clone(), to };
            done(
                calls.call(&Agent(speaker), &Retry(retry)).await?,
                || words::CARRYING_ON.into(),
            )
        }
        Act::Stop(named) => {
            let agent = match named {
                None => Ok(speaker),
                Some(target) => number(view, &target),
            };
            match agent {
                Ok(agent) => {
                    Ok(words::stopped(stop(calls, id, agent).await?))
                }
                Err(refusal) => Err(refusal),
            }
        }
        Act::StopAll => {
            let mut turns = 0;
            for seen in &view.world.agents {
                if !matches!(seen.status, Status::Ended { .. }) {
                    turns += stop(calls, id, seen.agent).await?;
                }
            }
            Ok(words::stopped(turns))
        }
        Act::To { to: target, text } => match number(view, &target) {
            Ok(agent) => {
                let line = protocol::Say {
                    files: Vec::new(),
                    id: line_id(id),
                    images: Vec::new(),
                    text,
                    to: view::wire(agent),
                };
                match calls.call(&Realm, &Say(line)).await? {
                    SayAnswer::Refused { why } => Err(refused(why)),
                    _ => Ok(words::told(&view.name(agent))),
                }
            }
            Err(refusal) => Err(refusal),
        },
        Act::Reflect => {
            let asked = calls.call(&Agent(speaker), &Reflect).await?;
            Ok(words::reflecting(
                &view.name(speaker),
                asked == Reflecting::Asked,
            ))
        }
        Act::Worker(start) => match ask(calls, id, start).await? {
            Started::Started { worker } => {
                Ok(words::worker_started(worker.0))
            }
            Started::Refused { why } => Err(refused(why)),
        },
        Act::Queue => {
            let waits = view
                .world
                .waits
                .iter()
                .map(|seen| {
                    (view::wait(view, seen), view.name(seen.agent))
                })
                .collect::<Vec<_>>();
            Ok(words::queue(&waits))
        }
        Act::Answer { choice, wait, text } => {
            answer_wait(calls, view, id, speaker, choice, wait, text)
                .await?
        }
        Act::Grants => {
            Ok(words::grants(view.world.grants.iter().map(|seen| {
                (view.name(seen.agent), seen.permission)
            })))
        }
        Act::Revoke { agent, permission } => {
            match number(view, &agent) {
                Ok(agent) => {
                    let name = view.name(agent);
                    let word = gate::word(&permission);
                    match permission {
                        protocol::Permission::Network => {
                            let act = Revoke {
                                agent,
                                permission: Permission::Network,
                            };
                            done(ask(calls, id, act).await?, || {
                                words::revoked(&name, word)
                            })
                        }
                        protocol::Permission::Unconfined => {
                            Ok(words::holds_no_grant(&name, word))
                        }
                    }
                }
                Err(refusal) => Err(refusal),
            }
        }
        Act::Hamlets => Ok(words::hamlets(
            view.world.hamlets.iter().map(|place| {
                (place.name.as_str(), place.hamlet.0.as_str())
            }),
        )),
        Act::Found(act) => {
            let shown = act.name.clone();
            match ask(calls, id, act).await? {
                FoundAnswer::Founded { hamlet, mayor } => {
                    Ok(words::founded(
                        &shown,
                        &hamlet.0,
                        &view.address(mayor),
                    ))
                }
                FoundAnswer::Refused { why } => Err(refused(why)),
            }
        }
        Act::NameHamlet(act) => {
            let said = words::hamlet_named(&act.hamlet.0, &act.name);
            done(ask(calls, id, act).await?, || said)
        }
        Act::MoveHamlet(act) => {
            let said =
                words::hamlet_set_down(&act.hamlet.0, act.x, act.z);
            done(ask(calls, id, act).await?, || said)
        }
        Act::Tier { set, most } => {
            let said = words::tier(&set.hamlet.0, most);
            done(ask(calls, id, set).await?, || said)
        }
        Act::Seat { hire, hamlet, why } => {
            let said = words::seated(&hire.text, &hamlet.0);
            let act = Seat {
                hire: hire.named,
                why,
            };
            done(ask(calls, id, act).await?, || said)
        }
        Act::Hire(act) => match ask(calls, id, act).await? {
            HireAnswer::Hired { agent } => {
                Ok(words::hired(&view.address(agent)))
            }
            HireAnswer::Refused { why } => Err(refused(why)),
        },
        Act::End { hire, why } => {
            let said = words::ended(&hire.text);
            let act = End {
                hire: hire.named,
                why,
            };
            done(ask(calls, id, act).await?, || said)
        }
        Act::Steer { hire, part } => {
            steer(calls, view, id, hire, part).await?
        }
        Act::Nickname { hire, name } => {
            let said = words::nickname(&hire.text, name.as_deref());
            let act = SetNickname {
                hire: hire.named,
                name,
                why: None,
            };
            done(ask(calls, id, act).await?, || said)
        }
        Act::Tools { hire, off, on } => {
            let said = words::tools_changed(&hire.text);
            let names = |tools: Vec<String>| {
                tools
                    .into_iter()
                    .map(|name| ToolName { name })
                    .collect()
            };
            let act = SetTools {
                hire: hire.named,
                off: names(off),
                on: names(on),
                why: None,
            };
            done(ask(calls, id, act).await?, || said)
        }
        Act::Allowance { hire, micros } => {
            let said = words::allowance(&hire.text, micros);
            let act = SetAllowance {
                hire: hire.named,
                micros: micros.map(|micros| {
                    Count(u64::try_from(micros).unwrap_or(0))
                }),
                why: None,
            };
            done(ask(calls, id, act).await?, || said)
        }
        Act::Road { a, b, why, lay } => {
            if lay {
                let act = LayRoad {
                    a: a.named,
                    b: b.named,
                    why,
                };
                done(ask(calls, id, act).await?, || {
                    words::ROAD_LAID.into()
                })
            } else {
                let act = TakeUpRoad {
                    a: a.named,
                    b: b.named,
                    why,
                };
                done(ask(calls, id, act).await?, || {
                    words::ROAD_TAKEN_UP.into()
                })
            }
        }
        Act::Folder(act) => {
            let folder = act
                .folder
                .as_ref()
                .map(|folder| folder.display().to_string());
            let said =
                words::works_on(&act.hamlet.0, folder.as_deref());
            done(ask(calls, id, act).await?, || said)
        }
        Act::Mode { set, word } => {
            let said = words::mode(&set.hamlet.0, &word);
            done(ask(calls, id, set).await?, || said)
        }
        Act::Budget(hamlet) => {
            let (payer, whose) = match hamlet {
                Some(hamlet) => (Payer::Hamlet { hamlet }, "Hamlet"),
                None => (Payer::Realm, "Realm"),
            };
            Ok(view::report(view.spend(&payer), whose).summary)
        }
        Act::ResetBudget(payer) => {
            done(ask(calls, id, ResetBudget { payer }).await?, || {
                words::BUDGET_RESET.into()
            })
        }
        Act::Unsupported(what) => Err(Refusal::Unsupported { what }),
    };
    Ok(Outcome::said(reply))
}

/// Stops the running turns of `agent`, giving how many.
async fn stop(
    calls: &Calls,
    id: &RequestId,
    agent: AgentNumber,
) -> Result<u32> {
    let stop = protocol::Stop {
        agent: Some(view::wire(agent)),
        id: id.clone(),
    };
    Ok(calls.call(&Agent(agent), &Stop(stop)).await?.turns)
}

/// Answers a wait with `choice`: the one `wait` names, else the
/// oldest of the agent spoken to, else the oldest of all.
async fn answer_wait(
    calls: &Calls,
    view: &View,
    id: &RequestId,
    speaker: AgentNumber,
    choice: Choice,
    wait: Option<String>,
    text: Option<String>,
) -> Result<Result<String, Refusal>> {
    let waits = &view.world.waits;
    let oldest = |of: Option<AgentNumber>| {
        waits
            .iter()
            .filter(|seen| of.is_none_or(|agent| seen.agent == agent))
            .min_by_key(|seen| seen.wait.0)
    };
    let picked = match wait {
        Some(named) => named,
        None => {
            match oldest(Some(speaker)).or_else(|| oldest(None)) {
                Some(seen) => seen.wait.0.to_string(),
                None => return Ok(Ok(words::NOTHING_WAITS.into())),
            }
        }
    };
    let line = waits
        .iter()
        .find(|seen| seen.wait.0.to_string() == picked)
        .map(|seen| {
            words::wait_line(
                &view::wait(view, seen),
                &view.name(seen.agent),
            )
        });
    let answer = protocol::Answer {
        choice: choice.clone(),
        id: id.clone(),
        picks: None,
        text,
        wait: picked,
    };
    Ok(done(calls.call(&Realm, &Answer(answer)).await?, || {
        words::answered(&choice, &line.unwrap_or_default())
    }))
}

/// Sets one part of an agent's model, the other two as the realm
/// shows them, since the realm sets all three at once.
async fn steer(
    calls: &Calls,
    view: &View,
    id: &RequestId,
    hire: Target,
    part: Part,
) -> Result<Result<String, Refusal>> {
    let seen = match number(view, &hire) {
        Ok(agent) => view.agent(agent),
        Err(refusal) => return Ok(Err(refusal)),
    };
    let mut act = SetModel {
        of: ModelOf::Hire {
            hire: hire.named.clone(),
        },
        model: seen.and_then(|seen| seen.model.clone()),
        effort: seen.and_then(|seen| seen.effort),
        fast: seen.and_then(|seen| seen.fast),
        why: None,
    };
    let said = match part {
        Part::Model { model, why } => {
            let said = words::runs(&hire.text, model.as_deref());
            act.model = model;
            act.why = why;
            said
        }
        Part::Effort(effort) => {
            act.effort = effort.map(Level::wire);
            words::thinks(&hire.text, effort)
        }
        Part::Fast(fast) => {
            act.fast = Some(fast);
            words::fast(&hire.text, if fast { "on" } else { "off" })
        }
    };
    Ok(done(ask(calls, id, act).await?, || said))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_told_on_has_one_id_for_each_command() {
        let id = RequestId::new();
        assert_eq!(line_id(&id), line_id(&id));
        assert_ne!(line_id(&id), id);
    }
}
