//! A chief's slash commands, each to one act of one actor or to the
//! door's own words.

pub(super) mod parse;
mod versions;

use std::fmt::Display;

use anyhow::Result;
use hamlet_messages::address::{Agent, Realm};
use hamlet_messages::agent::chat::{
    Count, NewSession, ReadHistory, Reflect, Reflecting, Resume,
    ResumeAnswer, Retry,
};
use hamlet_messages::profile::VersionName;
use hamlet_messages::realm::budget::{Payer, ResetBudget};
use hamlet_messages::realm::choices::{
    ModelOf, SetModel, SetNickname,
};
use hamlet_messages::realm::client::{Answer, ReadState};
use hamlet_messages::realm::hamlets::{ReadRecord, Record};
use hamlet_messages::realm::hires::{
    EraseHire, Merge, Restore, Restored,
};
use hamlet_messages::realm::oversight::{
    Permission, Revoke, Revoked,
};
use hamlet_messages::realm::profiles::{
    Dream, Dreamer, Dreamt as Dreaming, Forked, Lifecycle,
    LifecycleRefusal, Made, MoveHouse, Site,
};
use hamlet_messages::realm::workers::{SetAllowance, Started};
use hamlet_messages::realm::world::Status;
use hamlet_messages::realm::{
    Asked, ChiefRequest, Done, End, FoundAnswer, HireAnswer, LayRoad,
    Named, Seat, SetTools, TakeUpRoad, ToolName,
};
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::{
    self as protocol, AgentKind, Choice, RequestId,
};

use self::parse::{Act, Dreamt, Part, Target, parse};
use super::call::{Calls, Said};
use super::host::HAND;
use super::view::{self, View};
use super::words;
use crate::oversight;
use crate::society::addresses::Reach;
use crate::society::effort::Effort as Level;
use crate::society::refusal::Refusal;

/// What a command came to.
pub(crate) struct Outcome {
    /// The words of the command's reply, or of its refusal.
    pub(crate) reply: Result<String, Refusal>,
    /// The agent whose chat its watchers read again.
    pub(crate) reread: Option<AgentNumber>,
}

impl Outcome {
    pub(crate) fn said(reply: Result<String, Refusal>) -> Self {
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

fn number(
    view: &View,
    target: &Target,
    reach: Reach,
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
            view.of_kind(*agent, AgentKind::Hire)
        }
        Named::Worker { worker } => {
            view.of_kind(*worker, AgentKind::Worker)
        }
        Named::Mayor => None,
    };
    found
        .filter(|agent| {
            reach == Reach::Kept || view.is_living(*agent)
        })
        .ok_or_else(|| Refusal::NoOne {
            address: target.text.clone(),
        })
}

/// The first of `targets` that is not a hire, once each is found
/// within `reach`; the realm answers the rest.
fn not_hires(
    view: &View,
    reach: Reach,
    targets: &[&Target],
) -> Option<Refusal> {
    let found = targets
        .iter()
        .map(|target| number(view, target, reach).ok())
        .collect::<Option<Vec<_>>>()?;
    found
        .into_iter()
        .find_map(|agent| as_hire(view, agent).err())
}

fn as_hire(
    view: &View,
    agent: AgentNumber,
) -> Result<AgentNumber, Refusal> {
    view.of_kind(agent, AgentKind::Hire).ok_or_else(|| {
        Refusal::NotHire {
            name: view.name(agent),
        }
    })
}

/// The hire `target` names, ended or not.
fn hire_of(
    view: &View,
    target: &Target,
) -> Result<AgentNumber, Refusal> {
    as_hire(view, number(view, target, Reach::Kept)?)
}

fn restored(answer: Restored, hire: &str) -> Result<String, Refusal> {
    match answer {
        Restored::Back => Ok(words::hire_back(hire)),
        Restored::Split => Ok(words::hire_split(hire)),
        Restored::Refused { why } => Err(refused(why)),
    }
}

fn forked(
    answer: Forked,
    at: Option<&VersionName>,
    name: &str,
) -> Result<String, Refusal> {
    match answer {
        Forked::Forked { from, .. } => {
            Ok(words::forked(&from, at.map(|at| at.0.as_str()), name))
        }
        Forked::Refused { why } => Err(refused(why)),
    }
}

fn lived(
    answer: Lifecycle,
    words: impl FnOnce(&str) -> String,
) -> Result<String, Refusal> {
    match answer {
        Lifecycle::Done { label } => Ok(words(&label)),
        Lifecycle::Refused { why } => Err(refused(why)),
    }
}

/// A refusal of an actor, in its own words.
pub(super) fn refused(why: impl Display) -> Refusal {
    Refusal::Usage {
        words: why.to_string(),
    }
}

/// The words of an act that answers only that it was done.
pub(super) fn done<W: Display>(
    answer: Done<W>,
    words: impl FnOnce() -> String,
) -> Result<String, Refusal> {
    match answer {
        Done::Done => Ok(words()),
        Done::Refused { why } => Err(refused(why)),
    }
}

/// Asks the realm for the chief's `act` under the command's id.
pub(super) async fn ask<T: ChiefRequest>(
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

/// The id of a second act a request asks, told apart by `mark`, the
/// same for each repeat of the request.
pub(super) fn derived(id: &RequestId, mark: [u8; 2]) -> RequestId {
    let uuid = uuid::Uuid::parse_str(id.as_str())
        .map(|uuid| {
            uuid.as_u128() ^ u128::from(u16::from_be_bytes(mark))
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
            Ok(words::began(began.number))
        }
        Act::Sessions => {
            let history =
                calls.call(&Agent(speaker), &ReadHistory).await?;
            let mut numbers = history
                .sessions
                .iter()
                .map(|session| session.number)
                .collect::<Vec<_>>();
            numbers.sort_unstable();
            Ok(words::sessions(numbers.into_iter()))
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
                Some(target) => number(view, &target, Reach::Living),
            };
            match agent {
                Ok(agent) => {
                    Ok(words::stopped(calls.stop(&[agent]).await?))
                }
                Err(refusal) => Err(refusal),
            }
        }
        Act::StopAll => {
            Ok(words::stopped(stop_all(calls, view).await?))
        }
        Act::To { to: target, text } => {
            match number(view, &target, Reach::Living) {
                Ok(agent) => {
                    let line = protocol::Say {
                        files: Vec::new(),
                        id: derived(id, *b"to"),
                        images: Vec::new(),
                        text,
                        to: view::wire(agent),
                    };
                    match calls.say(line).await? {
                        Said::Accepted => {
                            Ok(words::told(&view.name(agent)))
                        }
                        Said::Left(words) | Said::Refused(words) => {
                            Err(Refusal::Usage { words })
                        }
                    }
                }
                Err(refusal) => Err(refusal),
            }
        }
        Act::Reflect => {
            let asked = calls.call(&Agent(speaker), &Reflect).await?;
            Ok(words::reflecting(
                &view.name(speaker),
                asked == Reflecting::Asked,
            ))
        }
        Act::Worker(start) => match ask(calls, id, start).await? {
            Started::Started { worker } => {
                Ok(words::worker_started(worker.agent.0))
            }
            Started::Refused { why } => Err(refused(why)),
        },
        Act::Queue => {
            let waits = view
                .world
                .waits
                .iter()
                .filter_map(|seen| {
                    Some((
                        view.wait(seen)?.clone(),
                        view.name(seen.agent),
                    ))
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
            match number(view, &agent, Reach::Kept) {
                Ok(agent) => {
                    let name = view.name(agent);
                    let word = oversight::word(&permission);
                    match permission {
                        protocol::Permission::Network => {
                            let act = Revoke {
                                agent,
                                permission: Permission::Network,
                            };
                            match ask(calls, id, act).await? {
                                Revoked::Refused { why } => {
                                    Err(refused(why))
                                }
                                notice => Ok(notice.to_string()),
                            }
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
                        &view::read(calls, &view.own)
                            .await?
                            .name(mayor),
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
            HireAnswer::Hired { agent } => Ok(words::hired(
                &view::read(calls, &view.own)
                    .await?
                    .name(agent.agent),
            )),
            HireAnswer::Refused { why } => Err(refused(why)),
        },
        Act::End { hire, .. }
            if let Some(refusal) =
                not_hires(view, Reach::Living, &[&hire]) =>
        {
            Err(refusal)
        }
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
        Act::Road { a, b, .. }
            if let Some(refusal) =
                not_hires(view, Reach::Living, &[&a, &b]) =>
        {
            Err(refusal)
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
        Act::Merge {
            absorbed,
            into: Err(refusal),
            ..
        } => Err(number(view, &absorbed, Reach::Living)
            .err()
            .unwrap_or(refusal)),
        Act::Merge {
            absorbed,
            into: Ok(into),
            ..
        } if let Some(refusal) =
            not_hires(view, Reach::Living, &[&absorbed, &into]) =>
        {
            Err(refusal)
        }
        Act::Merge {
            absorbed,
            into: Ok(into),
            why,
        } => {
            let said = words::merged(&absorbed.text, &into.text);
            let act = Merge {
                absorbed: absorbed.named,
                into: into.named,
                why: Some(why),
            };
            done(ask(calls, id, act).await?, || said)
        }
        Act::Restore { hire, why } => match hire_of(view, &hire) {
            Ok(agent) => {
                let act = Restore {
                    hire: Named::Agent { agent },
                    why,
                };
                restored(ask(calls, id, act).await?, &hire.text)
            }
            Err(refusal) => Err(refusal),
        },
        Act::Erase { hire }
            if let Some(refusal) =
                not_hires(view, Reach::Kept, &[&hire]) =>
        {
            Err(refusal)
        }
        Act::Erase { hire } => {
            let said = words::erased(&hire.text);
            let act = EraseHire { hire: hire.named };
            done(ask(calls, id, act).await?, || said)
        }
        Act::RemoveHamlet(act) => {
            let said = words::hamlet_removed(&act.hamlet.0);
            done(ask(calls, id, act).await?, || said)
        }
        Act::RestoreHamlet(act) => {
            let said = words::hamlet_restored(&act.hamlet.0);
            done(ask(calls, id, act).await?, || said)
        }
        Act::EraseHamlet(act) => {
            let said = words::hamlet_erased(&act.hamlet.0);
            done(ask(calls, id, act).await?, || said)
        }
        Act::Fork(act) => {
            let (at, name) = (act.at.clone(), act.name.clone());
            forked(ask(calls, id, act).await?, at.as_ref(), &name)
        }
        Act::ArchiveProfile(act) => {
            lived(ask(calls, id, act).await?, words::profile_archived)
        }
        Act::RestoreProfile(act) => {
            lived(ask(calls, id, act).await?, words::profile_restored)
        }
        Act::EraseProfile(act) => {
            lived(ask(calls, id, act).await?, words::erased)
        }
        Act::Welcome(act) => {
            lived(ask(calls, id, act).await?, words::welcomed)
        }
        Act::TurnAway(act) => {
            lived(ask(calls, id, act).await?, words::turned_away)
        }
        Act::MoveHouse {
            profile,
            spot: Ok(spot),
        } => {
            let act = MoveHouse {
                profile,
                site: Site::of(spot),
            };
            lived(ask(calls, id, act).await?, |label| {
                words::house_moved(label, spot.x, spot.z)
            })
        }
        Act::MoveHouse {
            profile,
            spot: Err(usage),
        } => {
            let act = MoveHouse {
                profile,
                site: Site::Unread,
            };
            match ask(calls, id, act).await? {
                Lifecycle::Refused {
                    why: LifecycleRefusal::Profile(why),
                } => Err(refused(why)),
                _ => Err(usage),
            }
        }
        Act::History(hamlet) => {
            let act = ReadRecord {
                hamlet: Some(hamlet),
            };
            match ask(calls, id, act).await? {
                Record::Read { lines } => Ok(lines
                    .into_iter()
                    .map(|line| line.text)
                    .collect::<Vec<_>>()
                    .join("\n")),
                Record::Refused { why } => Err(refused(why)),
            }
        }
        Act::Scholars => {
            let state = calls.call(&Realm, &ReadState).await?;
            let hands = state
                .agents
                .iter()
                .filter(|agent| agent.kind == AgentKind::Hand)
                .filter_map(|agent| agent.scholar)
                .collect::<Vec<_>>();
            Ok(words::scholars(
                state
                    .scholars
                    .into_iter()
                    .filter(|shown| {
                        shown.arrival != protocol::Arrival::Apart
                            && !hands.contains(&shown.id)
                    })
                    .map(|shown| {
                        (
                            format!(
                                "scholar#{} ({})",
                                shown.id.0, shown.name
                            ),
                            shown.left.is_none(),
                        )
                    }),
            ))
        }
        Act::CreateProfile(act) => match ask(calls, id, act).await? {
            Made::Made { label, .. } => {
                Ok(words::profile_made(&label))
            }
            Made::Refused { why } => Err(refused(why)),
        },
        Act::ToolsOff(hire) => number(view, &hire, Reach::Living)
            .map(|agent| {
                let off = view
                    .agent(agent)
                    .map(|seen| {
                        seen.tools_off
                            .iter()
                            .map(|tool| tool.name.clone())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                words::tools_off(&hire.text, &off)
            }),
        Act::Versions(profile) => {
            versions::list(calls, profile).await?
        }
        Act::Tag(tag) => versions::tag(calls, id, tag).await?,
        Act::Revert {
            profile,
            which,
            reason,
        } => {
            versions::revert(calls, id, profile, &which, reason)
                .await?
        }
        Act::Purge(act) => versions::purge(calls, id, act).await?,
        Act::Dream(dreamt) => {
            let of = match dreamt {
                Dreamt::Profile(profile) => {
                    Ok(Dreamer::Profile { profile })
                }
                Dreamt::Hire(hire) => hire_of(view, &hire)
                    .map(|hire| Dreamer::Hire { hire }),
            };
            match of {
                Ok(of) => match ask(calls, id, Dream { of }).await? {
                    Dreaming::Began { .. } => {
                        Ok(words::DREAM_BEGAN.into())
                    }
                    Dreaming::Nothing => {
                        Ok(words::NOTHING_TO_DREAM.into())
                    }
                    Dreaming::Refused { why } => Err(refused(why)),
                },
                Err(refusal) => Err(refusal),
            }
        }
        Act::Pin { hire, to, reason } => {
            match number(view, &hire, Reach::Living) {
                Ok(agent) => {
                    versions::pin(calls, view, id, agent, to, reason)
                        .await?
                }
                Err(refusal) => Err(refusal),
            }
        }
        Act::Unshelve(act) => {
            let said = words::unshelved(&act.skill);
            done(ask(calls, id, act).await?, || said)
        }
        Act::NameProfile(act) => match ask(calls, id, act).await? {
            Made::Made { label, .. } => {
                Ok(words::profile_named(&label))
            }
            Made::Refused { why } => Err(refused(why)),
        },
    };
    Ok(Outcome::said(reply))
}

/// Stops the running turns of the agent a client names by `agent`,
/// in the words of a client's stop.
pub(crate) async fn stop_one(
    calls: &Calls,
    view: &View,
    agent: protocol::AgentId,
) -> Result<Result<String, Refusal>> {
    let shown = super::route::number(agent)
        .filter(|number| view.is_living(*number));
    let Some(number) = shown else {
        return Ok(Err(Refusal::NoOne {
            address: agent.0.to_string(),
        }));
    };
    let turns = calls.stop(&[number]).await?;
    Ok(Ok(words::stopped_or_nothing(turns)))
}

/// Of the open waits, each a number and the agent that asked it, the
/// oldest `speaker` asked, else the oldest of all.
fn oldest_wait(
    open: impl IntoIterator<Item = (u64, AgentNumber)> + Clone,
    speaker: AgentNumber,
) -> Option<u64> {
    let oldest = |of: Option<AgentNumber>| {
        open.clone()
            .into_iter()
            .filter(|(_, agent)| of.is_none_or(|of| *agent == of))
            .map(|(wait, _)| wait)
            .min()
    };
    oldest(Some(speaker)).or_else(|| oldest(None))
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
    let open = view
        .world
        .waits
        .iter()
        .map(|seen| (seen.wait.0, seen.agent));
    let picked = match wait {
        Some(named) => named,
        None => match oldest_wait(open, speaker) {
            Some(oldest) => oldest.to_string(),
            None => return Ok(Ok(words::NOTHING_WAITS.into())),
        },
    };
    let answer = protocol::Answer {
        choice,
        id: id.clone(),
        picks: None,
        text,
        wait: picked,
    };
    self::answer(calls, view, answer).await
}

/// Gives the realm a chief's answer to a wait, worded with the wait
/// as `view` shows it.
pub(crate) async fn answer(
    calls: &Calls,
    view: &View,
    answer: protocol::Answer,
) -> Result<Result<String, Refusal>> {
    let number = answer.wait.trim_start_matches('#');
    let line = view
        .world
        .waits
        .iter()
        .find(|seen| seen.wait.0.to_string() == number)
        .and_then(|seen| {
            Some(words::wait_line(
                view.wait(seen)?,
                &view.name(seen.agent),
            ))
        });
    let choice = answer.choice.clone();
    Ok(done(calls.call(&Realm, &Answer(answer)).await?, || {
        words::answered(&choice, &line.unwrap_or_default())
    }))
}

/// Stops each agent `view` shows that has not ended, a hire still
/// being hired too, in one act, and gives the turns stopped.
pub(crate) async fn stop_all(
    calls: &Calls,
    view: &View,
) -> Result<u32> {
    let living = view
        .world
        .agents
        .iter()
        .filter(|seen| !matches!(seen.status, Status::Ended { .. }))
        .map(|seen| seen.agent)
        .collect::<Vec<_>>();
    calls.stop(&living).await
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
    let seen = match number(view, &hire, Reach::Living) {
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
mod tests;
