//! Durable hearings, fixed voters, ballots and their office
//! decisions.

use anyhow::{Context, Result};
use hamlet_protocol::{self as protocol, VoteView, Voter, VoterCast};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Caller, Deed, errands, outcome};
use crate::Outcome;
use crate::harness::Harnesses;
use crate::limits::Limits;
use crate::protocol::Refusal;
use crate::record::{
    self, Actor, AgentRef, Change, NameFallback, RecordOperation,
};
use crate::service::Service;

#[derive(Clone, Serialize, Deserialize)]
struct Ballot {
    voter: u64,
    /// `None` for a voter that answered without casting a ballot.
    choice: VoterCast,
    reason: String,
    evidence: String,
    grounded: bool,
}

/// What a vote decides.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum VoteKind {
    ReplaceMayor,
    FoundHamlet,
}

/// Where a vote stands.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Standing {
    Open,
    Passed,
    Overturned,
    Lapsed,
    /// Closed without passing, for the reason given.
    Closed(String),
}

#[derive(Clone, Serialize, Deserialize)]
struct Vote {
    id: u64,
    kind: VoteKind,
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
    standing: Standing,
    heard: bool,
    answer: Option<String>,
    ballots: Vec<Ballot>,
    announced: bool,
    reported: bool,
}

/// The society document kind of a hamlet's votes still open or not
/// yet reported, the only ones the sweep reads.
const DUE: &str = "votes";
/// The society document kind of a hamlet's finished votes.
const DONE: &str = "votes-done";

impl Vote {
    fn due(&self) -> bool {
        self.standing == Standing::Open || !self.reported
    }
}

/// Every vote of `hamlet`, in the order called.
fn read(db: &Connection, hamlet: &str) -> Result<Vec<Vote>> {
    let mut votes = super::document::<Vec<Vote>>(db, DONE, hamlet)?;
    votes.extend(super::document::<Vec<Vote>>(db, DUE, hamlet)?);
    votes.sort_by_key(|v| v.id);
    Ok(votes)
}

/// Keeps `votes` of `hamlet`, which hold all its due votes, and the
/// open ones as its record's guards.
fn save(db: &Connection, hamlet: &str, votes: &[Vote]) -> Result<()> {
    let (due, done) =
        votes.iter().cloned().partition::<Vec<_>, _>(Vote::due);
    if due.is_empty() {
        db.execute(
            "DELETE FROM society_document WHERE kind = ?1 AND key = \
             ?2",
            [DUE, hamlet],
        )?;
    } else {
        super::keep(db, DUE, hamlet, &due)?;
    }
    if !done.is_empty() {
        let mut kept =
            super::document::<Vec<Vote>>(db, DONE, hamlet)?;
        kept.retain(|v| done.iter().all(|d| d.id != v.id));
        kept.extend(done);
        kept.sort_by_key(|v| v.id);
        super::keep(db, DONE, hamlet, &kept)?;
    }
    if let Some(r) = record::read(db, hamlet)? {
        let open = due
            .iter()
            .filter(|v| v.standing == Standing::Open)
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

/// The yes votes `v` needs to pass under `limits`.
fn needed(v: &Vote, limits: &Limits) -> u64 {
    (v.voters.len() as u64 * limits.votes.pass_yes as u64)
        .div_ceil((limits.votes.pass_of as u64).max(1))
        .max(1)
}

fn describe(v: &Vote, r: &record::Hamlet) -> String {
    let name = |n| r.name_of(n, NameFallback::Recorded);
    if v.kind == VoteKind::ReplaceMayor {
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

fn report(v: &Vote, r: &record::Hamlet, limits: &Limits) -> String {
    let cast = |choice: VoterCast| {
        v.ballots.iter().filter(|b| b.choice == choice).count()
    };
    let (yes, no, abstain) = (
        cast(VoterCast::Yes),
        cast(VoterCast::No),
        cast(VoterCast::Abstain),
    );
    let silent = v.voters.len() - yes - no - abstain;
    let needed = needed(v, limits);
    let standing = match &v.standing {
        Standing::Open => "Open".into(),
        Standing::Passed => "Passed".into(),
        Standing::Overturned => {
            "Passed, then overturned by a chief".into()
        }
        Standing::Lapsed => "Failed (lapsed)".into(),
        Standing::Closed(reason) => format!("Failed ({reason})"),
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
                match b.choice {
                    VoterCast::Yes => "yes",
                    VoterCast::No => "no",
                    VoterCast::Abstain => "abstain",
                    VoterCast::None => "no answer",
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
) -> Result<Vec<VoteView>> {
    let limits = crate::limits::of(db, &r.id)?;
    read(db, &r.id)?
        .iter()
        .map(|v| {
            let voters = v.voters.iter().map(|&number| Voter {
                agent: protocol::AgentRef {
                    hamlet: r.id.clone(),
                    number,
                },
                cast: v
                    .ballots
                    .iter()
                    .find(|b| b.voter == number)
                    .map(|b| b.choice.clone()),
            });
            Ok(VoteView {
                by: v.by.clone(),
                id: i64::try_from(v.id).with_context(|| {
                    format!("vote {} of {}", v.id, r.id)
                })?,
                motion: describe(v, r),
                open: v.standing == Standing::Open,
                overturnable: matches!(
                    v.standing,
                    Standing::Passed | Standing::Open
                ),
                report: report(v, r, &limits),
                voters: voters.collect(),
            })
        })
        .collect()
}

/// Why a vote could not be called.
enum Uncalled {
    StillOpen { vote: u64, motion: String },
    FewVoters { least: usize, voters: usize },
    Kind,
    NoCandidate,
    AlreadyMayor { name: String },
    NoReason,
    TooSoon { vote: u64 },
}

impl Uncalled {
    fn text(&self) -> String {
        match self {
            Self::StillOpen { vote, motion } => {
                format!("Vote {vote} is still open: {motion}")
            }
            Self::FewVoters { least, voters } => format!(
                "A vote needs at least {least} agents besides the \
                 mayor; there are {voters}."
            ),
            Self::Kind => "Use replace_mayor or found_hamlet.".into(),
            Self::NoCandidate => "The candidate must be an agent of \
                                  this hamlet."
                .into(),
            Self::AlreadyMayor { name } => {
                format!("`{name}` is the mayor already.")
            }
            Self::NoReason => "Give a reason for the vote.".into(),
            Self::TooSoon { vote } => format!(
                "Vote {vote} was called too recently. Wait before \
                 calling it again."
            ),
        }
    }
}

/// The arguments of `call_vote`: those a kind of vote does not use
/// are empty.
#[derive(Default, Deserialize)]
pub(crate) struct CallVote {
    kind: String,
    reason: String,
    #[serde(default)]
    candidate: String,
    #[serde(default)]
    hamlet: String,
    #[serde(default)]
    mayor: String,
    #[serde(default)]
    purpose: String,
}

/// The arguments of `cast_ballot`.
#[derive(Deserialize)]
pub(crate) struct Cast {
    choice: String,
    reason: String,
    evidence: String,
}

/// Calls a vote in `hamlet` by `by`, the agent `caller` or a chief,
/// kept for the call `owner` when an agent's call made it.
fn call(
    service: &mut Service,
    hamlet: &str,
    by: String,
    caller: Option<u64>,
    args: &CallVote,
    owner: Option<&str>,
) -> Result<std::result::Result<u64, Uncalled>> {
    let r = service
        .home
        .store()
        .record(hamlet)?
        .context("the voting hamlet has no record")?;
    let limits = service.limits_of(hamlet)?;
    let mut votes = read(service.home.store().connection(), hamlet)?;
    let now = crate::clock::now();
    let open_ms = limits.vote_open_ms();
    for v in &mut votes {
        if v.standing == Standing::Open
            && now.saturating_sub(v.called) >= open_ms
        {
            v.standing = Standing::Lapsed;
        }
    }
    if let Some(v) =
        votes.iter().find(|v| v.standing == Standing::Open)
    {
        return Ok(Err(Uncalled::StillOpen {
            vote: v.id,
            motion: describe(v, &r),
        }));
    }
    let voters = r
        .agents
        .iter()
        .filter(|a| a.number != r.mayor)
        .map(|a| a.number)
        .collect::<Vec<_>>();
    let least = limits.votes.least_voters as usize;
    if voters.len() < least {
        return Ok(Err(Uncalled::FewVoters {
            least,
            voters: voters.len(),
        }));
    }
    let kind = match args.kind.as_str() {
        "replace_mayor" => VoteKind::ReplaceMayor,
        "found_hamlet" => VoteKind::FoundHamlet,
        _ => return Ok(Err(Uncalled::Kind)),
    };
    let mut candidate = 0;
    if kind == VoteKind::ReplaceMayor {
        let name = args.candidate.as_str();
        let Some(agent) = r.named(name) else {
            return Ok(Err(Uncalled::NoCandidate));
        };
        if agent.number == r.mayor {
            return Ok(Err(Uncalled::AlreadyMayor {
                name: name.into(),
            }));
        }
        candidate = agent.number;
    }
    let reason = args.reason.trim();
    if reason.is_empty() {
        return Ok(Err(Uncalled::NoReason));
    }
    let retry_ms = crate::clock::minutes(limits.votes.retry_minutes);
    if let Some(v) = votes.iter().rev().find(|v| {
        v.by == by
            && v.kind == kind
            && !matches!(
                v.standing,
                Standing::Passed | Standing::Overturned
            )
            && now.saturating_sub(v.called) < retry_ms
    }) {
        return Ok(Err(Uncalled::TooSoon { vote: v.id }));
    }
    let id = votes.last().map_or(1, |v| v.id + 1);
    votes.push(Vote {
        id,
        kind,
        candidate,
        destination: args.hamlet.clone(),
        founder: args.mayor.clone(),
        purpose: args.purpose.clone(),
        by,
        caller,
        reason: reason.into(),
        mayor: r.mayor,
        voters,
        called: now,
        task: format!("vote:{hamlet}:{id}"),
        standing: Standing::Open,
        heard: kind != VoteKind::ReplaceMayor
            || caller == Some(r.mayor),
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
    args: &CallVote,
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
            .context("the voter's hamlet has no record")?;
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
            Err(uncalled) => {
                return Ok(Some(outcome(&uncalled.text(), true)));
            }
        }
    };
    sweep(service, harnesses).await?;
    if service.house_work.founding(hamlet, id) {
        return Ok(None);
    }
    let v = read(service.home.store().connection(), hamlet)?
        .into_iter()
        .find(|v| v.id == id)
        .context("the called vote has no record")?;
    if v.standing == Standing::Open {
        return Ok(None);
    }
    let r = service
        .home
        .store()
        .record(hamlet)?
        .context("the voting hamlet has no record")?;
    Ok(Some(outcome(
        &report(&v, &r, &service.limits_of(hamlet)?),
        false,
    )))
}

pub(super) fn ballot(
    service: &mut Service,
    caller: &Caller<'_>,
    args: Cast,
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
        .find(|v| v.id == id && v.standing == Standing::Open)
    else {
        return Ok(outcome("The vote is closed.", true));
    };
    if v.ballots.iter().any(|b| b.voter == voter) {
        return Ok(outcome("Your ballot is already cast.", false));
    }
    let choice = match args.choice.as_str() {
        "yes" => VoterCast::Yes,
        "no" => VoterCast::No,
        "abstain" => VoterCast::Abstain,
        _ => return Ok(outcome("Choose yes, no or abstain.", true)),
    };
    let evidence = args.evidence.as_str();
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
        choice,
        reason: args.reason.clone(),
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
    let r = service
        .home
        .store()
        .record(hamlet)?
        .context("the voting hamlet has no record")?;
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

/// Cancels the hearing and ballot errands of vote `vote` of `hamlet`
/// that have not ended.
async fn cancel_errands(
    service: &mut Service,
    harnesses: &mut Harnesses,
    hamlet: &str,
    vote: u64,
) -> Result<()> {
    let bindings = service
        .home
        .store()
        .connection()
        .prepare(&format!(
            "SELECT s.binding FROM society_binding s JOIN \
             harness_input i ON i.binding=s.binding WHERE s.errand \
             IN ('hearing','ballot') AND \
             json_extract(s.owner,'$.hamlet')=?1 AND \
             json_extract(s.owner,'$.vote')=?2 AND \
             i.state!='dropped' AND NOT {}",
            crate::harness::ended("i.status")
        ))?
        .query_map(
            rusqlite::params![hamlet, i64::try_from(vote)?],
            |row| Ok(crate::harness::BindingId(row.get(0)?)),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for binding in bindings {
        harnesses.cancel(service.home.store(), binding).await?;
    }
    Ok(())
}

/// Moves on every vote still open or not yet reported.
pub(crate) async fn sweep(
    service: &mut Service,
    harnesses: &mut Harnesses,
) -> Result<()> {
    let hamlets = service
        .home
        .store()
        .connection()
        .prepare("SELECT key FROM society_document WHERE kind = ?1")?
        .query_map([DUE], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if hamlets.is_empty() {
        return Ok(());
    }
    let realm =
        crate::realm::read(service.home.store().connection())?;
    for hamlet in hamlets {
        if realm.lives(&hamlet) {
            sweep_hamlet(service, harnesses, &hamlet).await?;
        } else {
            close_hamlet(service, harnesses, &hamlet).await?;
        }
    }
    Ok(())
}

/// Closes the due votes of `hamlet`, which no longer lives.
async fn close_hamlet(
    service: &mut Service,
    harnesses: &mut Harnesses,
    hamlet: &str,
) -> Result<()> {
    let mut votes = super::document::<Vec<Vote>>(
        service.home.store().connection(),
        DUE,
        hamlet,
    )?;
    for vote in &mut votes {
        if vote.standing == Standing::Open {
            vote.standing =
                Standing::Closed("the hamlet closed".into());
        }
        cancel_errands(service, harnesses, hamlet, vote.id).await?;
        vote.reported = true;
    }
    save(service.home.store().connection(), hamlet, &votes)
}

/// Moves on the due votes of the living `hamlet`: each is announced,
/// heard, balloted and counted, decided once it closes, then
/// reported.
async fn sweep_hamlet(
    service: &mut Service,
    harnesses: &mut Harnesses,
    hamlet: &str,
) -> Result<()> {
    let mut votes = super::document::<Vec<Vote>>(
        service.home.store().connection(),
        DUE,
        hamlet,
    )?;
    let limits = service.limits_of(hamlet)?;
    let r = service
        .home
        .store()
        .record(hamlet)?
        .context("the voting hamlet has no record")?;
    for index in 0..votes.len() {
        let mut v = votes[index].clone();
        if service.house_work.founding(hamlet, v.id) {
            continue;
        }
        if !v.announced {
            let text = format!(
                "Vote {} called by {}: {}. {}",
                v.id,
                v.by,
                describe(&v, &r),
                v.reason
            );
            announce(service, harnesses, hamlet, &v, "called", &text)
                .await?;
            v.announced = true;
        }
        if v.standing == Standing::Open {
            poll(service, harnesses, hamlet, &r, &limits, &mut v)
                .await?;
            if v.standing != Standing::Open {
                votes[index] = v;
                if decide(
                    service, harnesses, hamlet, &r, &limits,
                    &mut votes, index,
                )
                .await?
                {
                    continue;
                }
                v = votes[index].clone();
            }
        }
        if v.standing != Standing::Open && !v.reported {
            let text = report(&v, &r, &limits);
            announce(service, harnesses, hamlet, &v, "result", &text)
                .await?;
            cancel_errands(service, harnesses, hamlet, v.id).await?;
            v.reported = true;
        }
        votes[index] = v;
    }
    save(service.home.store().connection(), hamlet, &votes)
}

/// Moves on the open vote `v`: it lapses past its time, else is
/// heard, then balloted and counted.
async fn poll(
    service: &mut Service,
    harnesses: &mut Harnesses,
    hamlet: &str,
    r: &record::Hamlet,
    limits: &Limits,
    v: &mut Vote,
) -> Result<()> {
    if crate::clock::now().saturating_sub(v.called)
        >= limits.vote_open_ms()
    {
        v.standing = Standing::Lapsed;
        return Ok(());
    }
    if !v.heard {
        hear(service, harnesses, hamlet, r, v).await?;
    }
    if v.heard {
        ask_ballots(service, harnesses, hamlet, r, limits, v).await?;
        if v.ballots.len() == v.voters.len() {
            count(v, limits);
        }
    }
    Ok(())
}

/// The mayor's one answer to the motion of `v`, once its hearing
/// ends.
async fn hear(
    service: &mut Service,
    harnesses: &mut Harnesses,
    hamlet: &str,
    r: &record::Hamlet,
    v: &mut Vote,
) -> Result<()> {
    let prompt = format!(
        "{}\nA vote has been called. This is your one chance to \
         answer in words only. Motion: {}. Reason: {}. Every voter \
         sees your answer as a claim, not evidence.",
        service
            .houses
            .read_notes(hamlet, v.mayor)
            .await
            .unwrap_or_default(),
        describe(v, r),
        v.reason
    );
    let binding = errands::start(
        service,
        harnesses,
        &AgentRef {
            hamlet: hamlet.into(),
            number: v.mayor,
        },
        "hearing",
        &json!({"hamlet":hamlet,"vote":v.id}),
        &prompt,
        &v.task,
    )
    .await?;
    if let Some(answer) = errands::answer(service, binding)? {
        v.answer = Some(crate::papers::cut(&answer, 600, ""));
        v.heard = true;
    }
    Ok(())
}

/// Asks each voter of `v` without a ballot for one, in turn unless
/// ballots are sealed. A voter who answers without casting one is
/// counted as not answering.
async fn ask_ballots(
    service: &mut Service,
    harnesses: &mut Harnesses,
    hamlet: &str,
    r: &record::Hamlet,
    limits: &Limits,
    v: &mut Vote,
) -> Result<()> {
    let sealed = limits.votes.sealed;
    for voter in v.voters.clone() {
        if v.ballots.iter().any(|b| b.voter == voter) {
            continue;
        }
        let agent = AgentRef {
            hamlet: hamlet.into(),
            number: voter,
        };
        let owner =
            json!({"hamlet":hamlet,"vote":v.id,"voter":voter});
        let corpus = corpus(service, &agent, &owner).await?;
        let earlier = if sealed {
            String::new()
        } else {
            serde_json::to_string(&v.ballots)?
        };
        let prompt = format!(
            "{corpus}\n\nThis is your ballot. Motion: {}. Reason: \
             {}. Mayor's answer: {}. These are claims, not \
             evidence. Earlier ballots: {earlier}\nVote from your \
             own notes and history. Abstain if you have no grounds. \
             Call cast_ballot once with choice yes, no or abstain, \
             reason and evidence copied from your house.",
            describe(v, r),
            v.reason,
            v.answer.as_deref().unwrap_or_default()
        );
        let binding = errands::start(
            service, harnesses, &agent, "ballot", &owner, &prompt,
            &v.task,
        )
        .await?;
        if errands::answer(service, binding)?.is_some() {
            v.ballots.push(Ballot {
                voter,
                choice: VoterCast::None,
                reason: "answered without casting a ballot".into(),
                evidence: String::new(),
                grounded: false,
            });
        } else if !sealed {
            break;
        }
    }
    Ok(())
}

/// The notes and recent history of `agent` its ballot is grounded
/// in, kept the first time under `owner` so a ballot is checked
/// against what its voter was shown.
async fn corpus(
    service: &mut Service,
    agent: &AgentRef,
    owner: &Value,
) -> Result<String> {
    let db = service.home.store().connection();
    if let Some(saved) = super::document::<Option<String>>(
        db,
        "ballot-corpus",
        &owner.to_string(),
    )? {
        return Ok(saved);
    }
    let mut corpus = service
        .houses
        .read_notes(&agent.hamlet, agent.number)
        .await
        .unwrap_or_default();
    for binding in crate::chats::records::conversations_of(
        service.home.store().connection(),
        agent,
    )? {
        if super::errand_of(
            service.home.store().connection(),
            binding,
        )?
        .is_some()
        {
            continue;
        }
        for item in
            service.home.store().items(binding)?.iter().rev().take(20)
        {
            corpus.push('\n');
            corpus.push_str(&item.text);
        }
    }
    super::keep(
        service.home.store().connection(),
        "ballot-corpus",
        &owner.to_string(),
        &corpus,
    )?;
    Ok(corpus)
}

/// The standing of `v` once every voter has answered.
fn count(v: &mut Vote, limits: &Limits) {
    let grounded_only = limits.votes.grounded_only;
    let yes = v
        .ballots
        .iter()
        .filter(|b| {
            b.choice == VoterCast::Yes
                && (!grounded_only || b.grounded)
        })
        .count() as u64;
    v.standing = if yes >= needed(v, limits) {
        Standing::Passed
    } else {
        Standing::Closed("too few said yes".into())
    };
}

/// Carries out the vote at `index` of the due `votes` of `hamlet`,
/// which just closed, and keeps them. True while a founded hamlet's
/// remote house is being prepared, and the vote waits for it.
async fn decide(
    service: &mut Service,
    harnesses: &mut Harnesses,
    hamlet: &str,
    r: &record::Hamlet,
    limits: &Limits,
    votes: &mut [Vote],
    index: usize,
) -> Result<bool> {
    let tx = service.home.store().transaction()?;
    let v = &mut votes[index];
    if v.standing == Standing::Passed {
        let refused = match v.kind {
            VoteKind::ReplaceMayor => seat(&tx, hamlet, v)?,
            VoteKind::FoundHamlet => {
                found(&tx, hamlet, r, limits, v)?
            }
        };
        if let Some(reason) = refused {
            v.standing = Standing::Closed(reason);
        }
    }
    let founded = (v.standing == Standing::Passed
        && v.kind == VoteKind::FoundHamlet)
        .then(|| (v.destination.clone(), v.id));
    save(&tx, hamlet, votes)?;
    tx.commit()?;
    if let Some((destination, vote)) = founded {
        let initialize = service.houses.initialize(&destination, 0);
        service.house_work.found(hamlet.into(), vote, initialize);
        return Ok(true);
    }
    changed(service, harnesses).await?;
    Ok(false)
}

/// Seats the candidate of the passed vote `v`; the words of why not.
fn seat(
    tx: &Connection,
    hamlet: &str,
    v: &Vote,
) -> Result<Option<String>> {
    let current = record::read(tx, hamlet)?
        .context("the voting hamlet has no record")?;
    if current.mayor != v.mayor {
        return Ok(Some(
            "the mayor changed while the vote was open".into(),
        ));
    }
    let op = RecordOperation {
        hamlet: hamlet.into(),
        actor: Actor::Chief(
            current.chiefs.first().cloned().unwrap_or_default(),
        ),
        at: crate::clock::now(),
        reason: v.reason.clone(),
        task: Some(v.task.clone()),
        change: Change::Seat {
            number: v.candidate,
        },
    };
    Ok(match crate::operation::change(tx, &op)? {
        Outcome::Refused(refusal) => {
            Some(refusal.text(record::Audience::Chief))
        }
        Outcome::Done(_) => None,
    })
}

/// Founds the hamlet of the passed vote `v` called in `r`; the words
/// of why not.
fn found(
    tx: &Connection,
    hamlet: &str,
    r: &record::Hamlet,
    limits: &Limits,
    v: &Vote,
) -> Result<Option<String>> {
    let realm = crate::realm::read(tx)?;
    if realm.hamlets.iter().any(|h| h.id == v.destination)
        || realm.live().count() >= limits.hamlets.most as usize
    {
        return Ok(Some(
            "the hamlet cannot be founded under current limits"
                .into(),
        ));
    }
    let op = record::FoundRecord {
        hamlet: v.destination.clone(),
        name: v.founder.clone(),
        purpose: v.purpose.clone(),
        chief: r.chiefs.first().cloned().unwrap_or_default(),
        at: crate::clock::now(),
    };
    if let Outcome::Refused(refusal) =
        crate::operation::change(tx, &op)?
    {
        return Ok(Some(refusal.text(record::Audience::Chief)));
    }
    let mut realm = crate::realm::read(tx)?;
    if let Some(place) = realm
        .hamlets
        .iter_mut()
        .find(|place| place.id == v.destination)
    {
        place.from = Some(crate::realm::From {
            hamlet: hamlet.into(),
            vote: v.id,
        });
    }
    crate::realm::write(tx, &realm)?;
    Ok(None)
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
            &CallVote {
                kind: "replace_mayor".into(),
                candidate: word.into(),
                reason: if reason.is_empty() {
                    "a chief asked".into()
                } else {
                    reason
                },
                ..CallVote::default()
            },
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
                .map_err(|uncalled| {
                    Refusal::Invalid(uncalled.text())
                }),
        ));
    }
    let mut votes = read(service.home.store().connection(), hamlet)?;
    let r = service
        .home
        .store()
        .record(hamlet)?
        .context("the voting hamlet has no record")?;
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
    let id = v.id;
    if v.standing == Standing::Overturned {
        return Ok(Some(Err(Refusal::Invalid(format!(
            "Vote {} is already overturned.",
            v.id
        )))));
    }
    if !matches!(v.standing, Standing::Passed | Standing::Open) {
        return Ok(Some(Err(Refusal::Invalid(
            "Only a passed or open vote can be overturned.".into(),
        ))));
    }
    let tx = service.home.store().transaction()?;
    let mut closed = None;
    let text = if v.standing == Standing::Open {
        v.standing = Standing::Closed("stopped by a chief".into());
        format!("Vote {} is stopped.", v.id)
    } else {
        let done = if v.kind == VoteKind::ReplaceMayor {
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
                        at: crate::clock::now(),
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
                at: crate::clock::now(),
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
        v.standing = Standing::Overturned;
        format!("Vote {} is overturned. {done}", v.id)
    };
    save(&tx, hamlet, &votes)?;
    tx.commit()?;
    crate::fault::point("after_vote_overturn");
    cancel_errands(service, harnesses, hamlet, id).await?;
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

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;
    use crate::Store;

    fn vote(id: u64, standing: Standing, reported: bool) -> Vote {
        Vote {
            id,
            kind: VoteKind::ReplaceMayor,
            candidate: 1,
            destination: String::new(),
            founder: String::new(),
            purpose: String::new(),
            by: "scout".into(),
            caller: Some(1),
            reason: "Needed".into(),
            mayor: 0,
            voters: Vec::new(),
            called: 0,
            task: format!("vote:home:{id}"),
            standing,
            heard: true,
            answer: None,
            ballots: Vec::new(),
            announced: true,
            reported,
        }
    }

    fn ids(votes: &[Vote]) -> Vec<u64> {
        votes.iter().map(|v| v.id).collect()
    }

    #[test]
    fn finished_votes_leave_the_sweep_and_stay_readable() {
        let scratch = Scratch::new("votes-due");
        let store =
            Store::open(&scratch.join("realm.sqlite")).unwrap();
        let db = store.connection();
        let due =
            || super::super::document::<Vec<Vote>>(db, DUE, "home");
        // Votes that finished since the sweep last saved them.
        super::super::keep(
            db,
            DUE,
            "home",
            &vec![
                vote(1, Standing::Passed, true),
                vote(2, Standing::Open, false),
                vote(3, Standing::Lapsed, false),
            ],
        )
        .unwrap();
        save(db, "home", &read(db, "home").unwrap()).unwrap();
        assert_eq!(ids(&due().unwrap()), [2, 3]);
        let mut swept = due().unwrap();
        swept[1].reported = true;
        save(db, "home", &swept).unwrap();
        assert_eq!(ids(&due().unwrap()), [2]);
        assert_eq!(ids(&read(db, "home").unwrap()), [1, 2, 3]);
        let mut swept = due().unwrap();
        swept[0].standing = Standing::Lapsed;
        swept[0].reported = true;
        save(db, "home", &swept).unwrap();
        assert!(due().unwrap().is_empty());
        assert_eq!(ids(&read(db, "home").unwrap()), [1, 2, 3]);
    }
}
