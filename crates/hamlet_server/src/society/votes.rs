//! Durable hearings, fixed voters, ballots and their office
//! decisions.

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Caller, Deed, errands, outcome};
use crate::Outcome;
use crate::harness::Harnesses;
use crate::protocol::Refusal;
use crate::record::{
    self, Actor, AgentRef, Change, NameFallback, RecordOperation,
};
use crate::service::Service;

#[derive(Clone, Serialize, Deserialize)]
struct Ballot {
    voter: u64,
    choice: String,
    reason: String,
    evidence: String,
    grounded: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct Vote {
    id: u64,
    kind: String,
    candidate: u64,
    destination: String,
    founder: String,
    purpose: String,
    by: String,
    caller: Option<u64>,
    reason: String,
    mayor: u64,
    voters: Vec<u64>,
    called: i64,
    task: String,
    standing: String,
    heard: bool,
    answer: Option<String>,
    ballots: Vec<Ballot>,
    announced: bool,
    reported: bool,
}

fn read(db: &Connection, hamlet: &str) -> Result<Vec<Vote>> {
    super::document(db, "votes", hamlet)
}

fn save(db: &Connection, hamlet: &str, votes: &[Vote]) -> Result<()> {
    super::keep(db, "votes", hamlet, &votes)?;
    if let Some(r) = record::read(db, hamlet)? {
        let open = votes
            .iter()
            .filter(|v| v.standing == "open")
            .map(|v| record::OpenVote {
                id: v.id,
                called: v.called,
                motion: describe(v, &r),
            })
            .collect();
        record::guard(db, hamlet, open, r.protected)?;
    }
    Ok(())
}

fn describe(v: &Vote, r: &record::Hamlet) -> String {
    let name = |n| r.name_of(n, NameFallback::Recorded);
    if v.kind == "replace_mayor" {
        format!(
            "replace the mayor: `{}` would become mayor, and `{}`, \
             the mayor now, would stay as an ordinary agent",
            name(v.candidate),
            name(v.mayor)
        )
    } else {
        format!(
            "found a new hamlet `{}` under a new mayor `{}`, whose \
             purpose would be: {} This hamlet and its mayor `{}` \
             stay as they are",
            v.destination,
            v.founder,
            v.purpose,
            name(v.mayor)
        )
    }
}

fn report(v: &Vote, r: &record::Hamlet, limits: &Value) -> String {
    let yes = v.ballots.iter().filter(|b| b.choice == "yes").count();
    let no = v.ballots.iter().filter(|b| b.choice == "no").count();
    let abstain =
        v.ballots.iter().filter(|b| b.choice == "abstain").count();
    let silent = v.voters.len() - yes - no - abstain;
    let needed = ((v.voters.len() as u64
        * limits["votes"]["passYes"].as_u64().unwrap_or(2))
    .div_ceil(limits["votes"]["passOf"].as_u64().unwrap_or(3)))
    .max(1);
    let standing = match v.standing.as_str() {
        "open" => "Open".into(),
        "passed" => "Passed".into(),
        "overturned" => "Passed, then overturned by a chief".into(),
        _ => format!("Failed ({})", v.standing),
    };
    format!(
        "Vote {}: {}.\nCalled by `{}`: {}{}\n{standing}: {yes} yes \
         of {} agents ({needed} needed), {no} no, {abstain} \
         abstained, {silent} did not answer.\n{}",
        v.id,
        describe(v, r),
        v.by,
        v.reason,
        v.answer
            .as_ref()
            .map(|answer| format!("\nThe mayor answered: {answer}"))
            .unwrap_or_default(),
        v.voters.len(),
        v.ballots
            .iter()
            .map(|b| format!(
                "- {}: {}. {}{}",
                r.name_of(b.voter, NameFallback::Recorded),
                if b.choice.is_empty() {
                    "no answer"
                } else {
                    &b.choice
                },
                b.reason,
                if b.evidence.is_empty() {
                    String::new()
                } else {
                    format!(
                        " [{} its house: \"{}\"]",
                        if b.grounded {
                            "found in"
                        } else {
                            "not found in"
                        },
                        b.evidence
                    )
                }
            ))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

pub(crate) fn views(
    db: &Connection,
    r: &record::Hamlet,
) -> Result<Vec<Value>> {
    let limits = crate::spend::limits(db, &r.id)?;
    Ok(read(db, &r.id)?.iter().map(|v| json!({"id":v.id,"by":v.by,"motion":describe(v,r),"open":v.standing=="open","overturnable":v.standing=="passed" || v.standing=="open","report":report(v,r,&limits),"voters":v.voters.iter().map(|n| json!({"agent":{"hamlet":r.id,"number":n},"cast":v.ballots.iter().find(|b|b.voter==*n).map(|b|if b.choice.is_empty(){"none"}else{&b.choice})})).collect::<Vec<_>>()})).collect())
}

fn call(
    service: &mut Service,
    hamlet: &str,
    by: String,
    caller: Option<u64>,
    args: &Value,
    owner: Option<&str>,
) -> Result<std::result::Result<u64, String>> {
    let r =
        service.home.store().record(hamlet)?.expect("vote's record");
    let limits = service.limits_of(hamlet)?;
    let mut votes = read(service.home.store().connection(), hamlet)?;
    let now = super::now();
    let open_ms = limits["votes"]["openMinutes"]
        .as_i64()
        .unwrap_or(60)
        .saturating_mul(60_000);
    for v in &mut votes {
        if v.standing == "open"
            && now.saturating_sub(v.called) >= open_ms
        {
            v.standing = "lapsed".into();
        }
    }
    if let Some(v) = votes.iter().find(|v| v.standing == "open") {
        return Ok(Err(format!(
            "Vote {} is still open: {}",
            v.id,
            describe(v, &r)
        )));
    }
    let voters = r
        .agents
        .iter()
        .filter(|a| a.number != r.mayor)
        .map(|a| a.number)
        .collect::<Vec<_>>();
    let least =
        limits["votes"]["leastVoters"].as_u64().unwrap_or(3) as usize;
    if voters.len() < least {
        return Ok(Err(format!(
            "A vote needs at least {least} agents besides the \
             mayor; there are {}.",
            voters.len()
        )));
    }
    let kind = args["kind"].as_str().unwrap_or("replace_mayor");
    if !["replace_mayor", "found_hamlet"].contains(&kind) {
        return Ok(Err("Use replace_mayor or found_hamlet.".into()));
    }
    let mut candidate = 0;
    if kind == "replace_mayor" {
        let name = args["candidate"].as_str().unwrap_or_default();
        let Some(agent) = r.named(name) else {
            return Ok(Err("The candidate must be an agent of \
                           this hamlet."
                .into()));
        };
        if agent.number == r.mayor {
            return Ok(Err(format!(
                "`{name}` is the mayor already."
            )));
        }
        candidate = agent.number;
    }
    let reason = args["reason"].as_str().unwrap_or_default().trim();
    if reason.is_empty() {
        return Ok(Err("Give a reason for the vote.".into()));
    }
    let retry_ms = limits["votes"]["retryMinutes"]
        .as_i64()
        .unwrap_or(60)
        .saturating_mul(60_000);
    if let Some(v) = votes.iter().rev().find(|v| {
        v.by == by
            && v.kind == kind
            && v.standing != "passed"
            && v.standing != "overturned"
            && now.saturating_sub(v.called) < retry_ms
    }) {
        return Ok(Err(format!(
            "Vote {} was called too recently. Wait before calling \
             it again.",
            v.id
        )));
    }
    let id = votes.last().map_or(1, |v| v.id + 1);
    votes.push(Vote {
        id,
        kind: kind.into(),
        candidate,
        destination: args["hamlet"]
            .as_str()
            .unwrap_or_default()
            .into(),
        founder: args["mayor"].as_str().unwrap_or_default().into(),
        purpose: args["purpose"].as_str().unwrap_or_default().into(),
        by,
        caller,
        reason: reason.into(),
        mayor: r.mayor,
        voters,
        called: now,
        task: format!("vote:{hamlet}:{id}"),
        standing: "open".into(),
        heard: kind != "replace_mayor" || caller == Some(r.mayor),
        answer: None,
        ballots: Vec::new(),
        announced: false,
        reported: false,
    });
    let tx = service.home.store().transaction()?;
    save(&tx, hamlet, &votes)?;
    if let Some(owner) = owner {
        super::keep(&tx, "vote-call", owner, &id)?;
    }
    tx.commit()?;
    Ok(Ok(id))
}

pub(super) async fn operation(
    service: &mut Service,
    harnesses: &mut Harnesses,
    caller: &Caller<'_>,
    args: &Value,
) -> Result<Option<Value>> {
    let key = serde_json::to_string(&(
        caller.id.runtime.as_str(),
        &caller.id.key,
    ))?;
    let known = super::document::<Option<u64>>(
        service.home.store().connection(),
        "vote-call",
        &key,
    )?;
    let hamlet = &caller.place.agent.hamlet;
    let id = if let Some(id) = known {
        id
    } else {
        let r = service
            .home
            .store()
            .record(hamlet)?
            .expect("voter's record");
        match call(
            service,
            hamlet,
            r.name_of(
                caller.place.agent.number,
                NameFallback::Recorded,
            ),
            Some(caller.place.agent.number),
            args,
            Some(&key),
        )? {
            Ok(id) => id,
            Err(text) => return Ok(Some(outcome(&text, true))),
        }
    };
    sweep(service, harnesses).await?;
    if service.house_work.founding.contains(&(hamlet.clone(), id)) {
        return Ok(None);
    }
    let v = read(service.home.store().connection(), hamlet)?
        .into_iter()
        .find(|v| v.id == id)
        .expect("called vote");
    if v.standing == "open" {
        return Ok(None);
    }
    let r =
        service.home.store().record(hamlet)?.expect("vote's record");
    Ok(Some(outcome(
        &report(&v, &r, &service.limits_of(hamlet)?),
        false,
    )))
}

pub(super) fn ballot(
    service: &mut Service,
    caller: &Caller<'_>,
    args: &Value,
) -> Result<Value> {
    let db = service.home.store().connection();
    let owner = db
        .query_row(
            "SELECT owner FROM society_binding WHERE binding=?1 AND \
             errand='ballot'",
            [caller.binding.id.0],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(owner) = owner else {
        return Ok(outcome("You were not asked for a ballot.", true));
    };
    let owner = serde_json::from_str::<Value>(&owner)?;
    let hamlet = owner["hamlet"].as_str().unwrap_or_default();
    let id = owner["vote"].as_u64().unwrap_or_default();
    let voter = caller.place.agent.number;
    let mut votes = read(db, hamlet)?;
    let Some(v) = votes
        .iter_mut()
        .find(|v| v.id == id && v.standing == "open")
    else {
        return Ok(outcome("The vote is closed.", true));
    };
    if v.ballots.iter().any(|b| b.voter == voter) {
        return Ok(outcome("Your ballot is already cast.", false));
    }
    let choice = args["choice"].as_str().unwrap_or_default();
    if !["yes", "no", "abstain"].contains(&choice) {
        return Ok(outcome("Choose yes, no or abstain.", true));
    }
    let evidence = args["evidence"].as_str().unwrap_or_default();
    let corpus = super::document::<String>(
        db,
        "ballot-corpus",
        &owner.to_string(),
    )?;
    let words = |s: &str| {
        s.split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_lowercase)
            .collect::<Vec<_>>()
    };
    let quote = words(evidence);
    let corpus = words(&corpus);
    let grounded =
        quote.windows(4).any(|q| corpus.windows(4).any(|c| c == q));
    v.ballots.push(Ballot {
        voter,
        choice: choice.into(),
        reason: args["reason"].as_str().unwrap_or_default().into(),
        evidence: evidence.into(),
        grounded,
    });
    save(db, hamlet, &votes)?;
    Ok(outcome("Your ballot is cast.", false))
}

async fn announce(
    service: &mut Service,
    harnesses: &mut Harnesses,
    hamlet: &str,
    v: &Vote,
    kind: &str,
    text: &str,
) -> Result<()> {
    let r =
        service.home.store().record(hamlet)?.expect("vote's record");
    for chief in r.chiefs {
        let previous = std::mem::replace(&mut service.chief, chief);
        let binding = crate::chats::current(
            service,
            harnesses,
            &AgentRef {
                hamlet: hamlet.into(),
                number: r.mayor,
            },
        )
        .await;
        service.chief = previous;
        let binding = binding?;
        crate::chats::write(
            service,
            harnesses,
            binding,
            &format!("vote:{hamlet}:{}:{kind}", v.id),
            text,
            false,
        )
        .await?;
    }
    Ok(())
}

async fn cancel_closed(
    service: &mut Service,
    harnesses: &mut Harnesses,
    hamlet: &str,
    votes: &[Vote],
) -> Result<()> {
    for vote in votes.iter().filter(|vote| vote.standing != "open") {
        let bindings = service
            .home
            .store()
            .connection()
            .prepare(
                "SELECT s.binding FROM society_binding s JOIN \
                 harness_input i ON i.binding=s.binding WHERE \
                 s.errand IN ('hearing','ballot') AND \
                 json_extract(s.owner,'$.hamlet')=?1 AND \
                 json_extract(s.owner,'$.vote')=?2 AND \
                 i.state!='dropped' AND coalesce(i.status,'queued') \
                 NOT IN ('done','unanswered')",
            )?
            .query_map(
                rusqlite::params![hamlet, i64::try_from(vote.id)?],
                |row| Ok(crate::harness::BindingId(row.get(0)?)),
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for binding in bindings {
            harnesses.cancel(service.home.store(), binding).await?;
        }
    }
    Ok(())
}

pub(crate) async fn sweep(
    service: &mut Service,
    harnesses: &mut Harnesses,
) -> Result<()> {
    let hamlets = service
        .home
        .store()
        .connection()
        .prepare(
            "SELECT key FROM society_document WHERE kind='votes'",
        )?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for hamlet in hamlets {
        let mut votes =
            read(service.home.store().connection(), &hamlet)?;
        if votes
            .iter()
            .all(|vote| vote.standing != "open" && vote.reported)
        {
            cancel_closed(service, harnesses, &hamlet, &votes)
                .await?;
            continue;
        }
        if !crate::realm::read(service.home.store().connection())?
            .lives(&hamlet)
        {
            for vote in &mut votes {
                if vote.standing == "open" {
                    vote.standing = "the hamlet closed".into();
                }
                vote.reported = true;
            }
            save(service.home.store().connection(), &hamlet, &votes)?;
            cancel_closed(service, harnesses, &hamlet, &votes)
                .await?;
            continue;
        }
        let limits = service.limits_of(&hamlet)?;
        let r = service
            .home
            .store()
            .record(&hamlet)?
            .expect("vote's record");
        for index in 0..votes.len() {
            let mut v = votes[index].clone();
            if service
                .house_work
                .founding
                .contains(&(hamlet.clone(), v.id))
            {
                continue;
            }
            if !v.announced {
                announce(
                    service,
                    harnesses,
                    &hamlet,
                    &v,
                    "called",
                    &format!(
                        "Vote {} called by {}: {}. {}",
                        v.id,
                        v.by,
                        describe(&v, &r),
                        v.reason
                    ),
                )
                .await?;
                v.announced = true;
            }
            if v.standing == "open" {
                let expired = super::now().saturating_sub(v.called)
                    >= limits["votes"]["openMinutes"]
                        .as_i64()
                        .unwrap_or(60)
                        .saturating_mul(60_000);
                if expired {
                    v.standing = "lapsed".into();
                } else if !v.heard {
                    let prompt = format!(
                        "{}\nA vote has been called. This is your \
                         one chance to answer in words only. \
                         Motion: {}. Reason: {}. Every voter sees \
                         your answer as a claim, not evidence.",
                        service
                            .houses
                            .read(&hamlet, v.mayor)
                            .unwrap_or_default(),
                        describe(&v, &r),
                        v.reason
                    );
                    let binding = errands::start(
                        service,
                        harnesses,
                        &AgentRef {
                            hamlet: hamlet.clone(),
                            number: v.mayor,
                        },
                        "hearing",
                        &json!({"hamlet":hamlet,"vote":v.id}),
                        &prompt,
                        &v.task,
                    )
                    .await?;
                    if let Some(answer) =
                        errands::answer(service, binding)?
                    {
                        v.answer = Some(crate::papers::cut(
                            &answer, 600, "",
                        ));
                        v.heard = true;
                    }
                }
                if v.heard && v.standing == "open" {
                    for voter in v.voters.clone() {
                        if v.ballots.iter().any(|b| b.voter == voter)
                        {
                            continue;
                        }
                        let agent = AgentRef {
                            hamlet: hamlet.clone(),
                            number: voter,
                        };
                        let mut corpus = service
                            .houses
                            .read(&hamlet, voter)
                            .unwrap_or_default();
                        for binding in
                            crate::chats::records::conversations_of(
                                service.home.store().connection(),
                                &agent,
                            )?
                        {
                            if super::errand_of(
                                service.home.store().connection(),
                                binding,
                            )?
                            .is_some()
                            {
                                continue;
                            }
                            for item in service
                                .home
                                .store()
                                .items(binding)?
                                .iter()
                                .rev()
                                .take(20)
                            {
                                corpus.push('\n');
                                corpus.push_str(&item.text);
                            }
                        }
                        let owner = json!({"hamlet":hamlet,"vote":v.id,"voter":voter});
                        let db = service.home.store().connection();
                        let saved = super::document::<Option<String>>(
                            db,
                            "ballot-corpus",
                            &owner.to_string(),
                        )?;
                        if let Some(saved) = saved {
                            corpus = saved;
                        } else {
                            super::keep(
                                db,
                                "ballot-corpus",
                                &owner.to_string(),
                                &corpus,
                            )?;
                        }
                        let earlier =
                            if limits["votes"]["sealed"] == false {
                                serde_json::to_string(&v.ballots)?
                            } else {
                                String::new()
                            };
                        let prompt = format!(
                            "{corpus}\n\nThis is your ballot. \
                             Motion: {}. Reason: {}. Mayor's \
                             answer: {}. These are claims, not \
                             evidence. Earlier ballots: \
                             {earlier}\nVote from your own notes \
                             and history. Abstain if you have no \
                             grounds. Call cast_ballot once with \
                             choice yes, no or abstain, reason and \
                             evidence copied from your house.",
                            describe(&v, &r),
                            v.reason,
                            v.answer.as_deref().unwrap_or_default()
                        );
                        let binding = errands::start(
                            service, harnesses, &agent, "ballot",
                            &owner, &prompt, &v.task,
                        )
                        .await?;
                        if errands::answer(service, binding)?
                            .is_some()
                        {
                            v.ballots.push(Ballot {
                                voter,
                                choice: String::new(),
                                reason: "answered without casting a \
                                         ballot"
                                    .into(),
                                evidence: String::new(),
                                grounded: false,
                            });
                        } else if limits["votes"]["sealed"] == false {
                            break;
                        }
                    }
                    if v.ballots.len() == v.voters.len() {
                        let yes = v
                            .ballots
                            .iter()
                            .filter(|b| {
                                b.choice == "yes"
                                    && (limits["votes"]
                                        ["groundedOnly"]
                                        != true
                                        || b.grounded)
                            })
                            .count()
                            as u64;
                        let needed = (v.voters.len() as u64
                            * limits["votes"]["passYes"]
                                .as_u64()
                                .unwrap_or(2))
                        .div_ceil(
                            limits["votes"]["passOf"]
                                .as_u64()
                                .unwrap_or(3)
                                .max(1),
                        )
                        .max(1);
                        v.standing = if yes >= needed {
                            "passed"
                        } else {
                            "too few said yes"
                        }
                        .into();
                    }
                }
            }
            let newly_closed = v.standing != "open"
                && votes[index].standing == "open";
            if newly_closed {
                let tx = service.home.store().transaction()?;
                if v.standing == "passed" {
                    if v.kind == "replace_mayor" {
                        let current = record::read(&tx, &hamlet)?
                            .expect("vote's record");
                        if current.mayor != v.mayor {
                            v.standing = "the mayor changed while \
                                          the vote was open"
                                .into();
                        } else {
                            let op = RecordOperation {
                                hamlet: hamlet.clone(),
                                actor: Actor::Chief(
                                    current
                                        .chiefs
                                        .first()
                                        .cloned()
                                        .unwrap_or_default(),
                                ),
                                at: super::now(),
                                reason: v.reason.clone(),
                                task: Some(v.task.clone()),
                                change: Change::Seat {
                                    number: v.candidate,
                                },
                            };
                            if let Outcome::Refused(refusal) =
                                crate::operation::change(&tx, &op)?
                            {
                                v.standing = refusal
                                    .text(record::Audience::Chief);
                            }
                        }
                    } else {
                        let realm = crate::realm::read(&tx)?;
                        if realm
                            .hamlets
                            .iter()
                            .any(|h| h.id == v.destination)
                            || realm.live().count()
                                >= limits["hamlets"]["most"]
                                    .as_u64()
                                    .unwrap_or(6)
                                    as usize
                        {
                            v.standing = "the hamlet cannot be \
                                          founded under current \
                                          limits"
                                .into();
                        } else {
                            let op = record::FoundRecord {
                                hamlet: v.destination.clone(),
                                name: v.founder.clone(),
                                purpose: v.purpose.clone(),
                                chief: r
                                    .chiefs
                                    .first()
                                    .cloned()
                                    .unwrap_or_default(),
                                at: super::now(),
                            };
                            let result =
                                crate::operation::change(&tx, &op)?;
                            if let Outcome::Refused(refusal) = result
                            {
                                v.standing = refusal
                                    .text(record::Audience::Chief);
                            } else {
                                let mut realm =
                                    crate::realm::read(&tx)?;
                                if let Some(place) = realm
                                    .hamlets
                                    .iter_mut()
                                    .find(|place| {
                                        place.id == v.destination
                                    })
                                {
                                    place.from =
                                        Some(crate::realm::From {
                                            hamlet: hamlet.clone(),
                                            vote: v.id,
                                        });
                                }
                                crate::realm::write(&tx, &realm)?;
                            }
                        }
                    }
                }
                votes[index] = v.clone();
                save(&tx, &hamlet, &votes)?;
                tx.commit()?;
                if v.standing == "passed" && v.kind == "found_hamlet"
                {
                    if let Some(remote) = service.houses.remote(
                        &v.destination,
                        0,
                        false,
                    )? {
                        let hamlet = hamlet.clone();
                        let vote = v.id;
                        service
                            .house_work
                            .founding
                            .insert((hamlet.clone(), vote));
                        service.house_work.tasks.spawn(async move {
                            let result = remote
                                .initialize()
                                .await
                                .map_err(Into::into);
                            super::work::Completed::Founded {
                                hamlet,
                                vote,
                                result,
                            }
                        });
                        continue;
                    }
                    service
                        .houses
                        .initialize(&v.destination, 0)
                        .await?;
                }
                changed(service, harnesses).await?;
            }
            if v.standing != "open" && !v.reported {
                announce(
                    service,
                    harnesses,
                    &hamlet,
                    &v,
                    "result",
                    &report(&v, &r, &limits),
                )
                .await?;
                v.reported = true;
            }
            votes[index] = v;
        }
        save(service.home.store().connection(), &hamlet, &votes)?;
        cancel_closed(service, harnesses, &hamlet, &votes).await?;
    }
    Ok(())
}

pub(super) async fn changed(
    service: &mut Service,
    harnesses: &mut Harnesses,
) -> Result<()> {
    super::reconfigure(service, harnesses).await?;
    let owners = service
        .home
        .store()
        .connection()
        .prepare(
            "SELECT runtime,key FROM society_call WHERE \
             tool='call_vote'",
        )?
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (runtime, key) in owners {
        harnesses
            .wake(&crate::RuntimeInstance::new(runtime)?, &key)
            .await;
    }
    Ok(())
}

pub(super) async fn deed(
    service: &mut Service,
    harnesses: &mut Harnesses,
    hamlet: &str,
    deed: &str,
    rest: &str,
) -> Result<Option<Deed>> {
    if !["vote", "votes", "overturn"].contains(&deed) {
        return Ok(None);
    }
    let mut words = rest.split_whitespace();
    let word = words.next().unwrap_or_default();
    let reason = words.collect::<Vec<_>>().join(" ");
    if deed == "vote" {
        let result = call(
            service,
            hamlet,
            format!("chief:{}", service.chief),
            None,
            &json!({"candidate":word,"reason":if reason.is_empty(){"a chief asked"}else{&reason}}),
            None,
        )?;
        sweep(service, harnesses).await?;
        return Ok(Some(
            result
                .map(|id| {
                    format!(
                        "Vote {id} is called. The chief may \
                         overturn it."
                    )
                })
                .map_err(Refusal::Invalid),
        ));
    }
    let mut votes = read(service.home.store().connection(), hamlet)?;
    let r =
        service.home.store().record(hamlet)?.expect("vote's record");
    if deed == "votes" {
        let limits = service.limits_of(hamlet)?;
        return Ok(Some(Ok(votes
            .iter()
            .map(|v| report(v, &r, &limits))
            .collect::<Vec<_>>()
            .join("\n\n"))));
    }
    let Some(v) = votes
        .iter_mut()
        .find(|v| word.parse::<u64>().ok() == Some(v.id))
    else {
        return Ok(Some(Err(Refusal::Invalid(format!(
            "There is no vote `{word}`."
        )))));
    };
    if v.standing == "overturned" {
        return Ok(Some(Err(Refusal::Invalid(format!(
            "Vote {} is already overturned.",
            v.id
        )))));
    }
    if v.standing != "passed" && v.standing != "open" {
        return Ok(Some(Err(Refusal::Invalid(
            "Only a passed or open vote can be overturned.".into(),
        ))));
    }
    let tx = service.home.store().transaction()?;
    let mut closed = None;
    let text = if v.standing == "open" {
        v.standing = "stopped by a chief".into();
        format!("Vote {} is stopped.", v.id)
    } else {
        let done = if v.kind == "replace_mayor" {
            if r.mayor != v.candidate {
                return Ok(Some(Err(Refusal::Invalid(
                    "The mayor has changed since that vote.".into(),
                ))));
            }
            let name = r.name_of(v.mayor, NameFallback::Recorded);
            if let Outcome::Refused(refusal) =
                crate::operation::change(
                    &tx,
                    &RecordOperation {
                        hamlet: hamlet.into(),
                        actor: Actor::Chief(service.chief.clone()),
                        at: super::now(),
                        reason: reason.clone(),
                        task: None,
                        change: Change::Seat { number: v.mayor },
                    },
                )?
            {
                return Ok(Some(Err(Refusal::Invalid(
                    refusal.text(record::Audience::Chief),
                ))));
            }
            format!("`{name}` is the mayor again.")
        } else {
            let mut realm = crate::realm::read(&tx)?;
            let Some(place) =
                realm.hamlets.iter_mut().find(|place| {
                    place.id == v.destination
                        && !place.erased
                        && place.removed.is_none()
                })
            else {
                return Ok(Some(Err(Refusal::Invalid(
                    "The founded hamlet is no longer living.".into(),
                ))));
            };
            place.removed = Some(crate::realm::Removed {
                at: super::now(),
                by: service.chief.clone(),
                reason: reason.clone(),
            });
            crate::realm::write(&tx, &realm)?;
            record::release_names(&tx, &v.destination)?;
            closed = record::read(&tx, &v.destination)?;
            format!(
                "The hamlet `{}` is removed and archived.",
                v.destination
            )
        };
        v.standing = "overturned".into();
        format!("Vote {} is overturned. {done}", v.id)
    };
    save(&tx, hamlet, &votes)?;
    tx.commit()?;
    crate::fault::point("after_vote_overturn");
    cancel_closed(service, harnesses, hamlet, &votes).await?;
    if let Some(r) = closed {
        for agent in r.agents {
            crate::chats::stop_agent(
                service,
                harnesses,
                &AgentRef {
                    hamlet: r.id.clone(),
                    number: agent.number,
                },
            )
            .await?;
            super::leaving::lapse(
                service.home.store().connection(),
                &r.id,
                agent.number,
            )?;
        }
    }
    super::reconfigure(service, harnesses).await?;
    Ok(Some(Ok(text)))
}
