//! Votes (H13): the agents of a hamlet deciding to replace their
//! mayor (H18) or to found a new hamlet (H6), kept in the hamlet's
//! record as a call, a ballot for each voter and a close.

mod asking;

use anyhow::{Result, bail};
use hamlet_model::log;
use serde_json::{Value, json};

use crate::hamlet::Hamlet;
use crate::realm::{Origin, Realm, Rules};
use crate::record::text;
pub use crate::vote::asking::{
    BALLOTS, PAYER, answer_text, ballot_definition, ballot_text,
    call_definition, grounded, history, motion_of,
};
use crate::words::clip;

/// The matter a vote decides.
#[derive(Clone, Debug, PartialEq)]
pub enum Motion {
    /// `candidate`, an agent of the hamlet, takes the office.
    ReplaceMayor { candidate: String },
    /// A new hamlet under a new agent as its mayor.
    FoundHamlet {
        hamlet: String,
        mayor: String,
        purpose: String,
    },
}

impl Motion {
    fn kind(&self) -> &'static str {
        match self {
            Motion::ReplaceMayor { .. } => "replace_mayor",
            Motion::FoundHamlet { .. } => "found_hamlet",
        }
    }

    fn to_json(&self) -> Value {
        match self {
            Motion::ReplaceMayor { candidate } => {
                json!({"kind": self.kind(), "candidate": candidate})
            }
            Motion::FoundHamlet {
                hamlet,
                mayor,
                purpose,
            } => json!({
                "kind": self.kind(), "hamlet": hamlet,
                "mayor": mayor, "purpose": purpose,
            }),
        }
    }

    fn from_json(e: &Value) -> Option<Self> {
        let text = |key: &str| Some(e[key].as_str()?.to_string());
        match e["kind"].as_str()? {
            "replace_mayor" => Some(Motion::ReplaceMayor {
                candidate: text("candidate")?,
            }),
            "found_hamlet" => Some(Motion::FoundHamlet {
                hamlet: text("hamlet")?,
                mayor: text("mayor")?,
                purpose: text("purpose")?,
            }),
            _ => None,
        }
    }

    /// The motion in a sentence, for a ballot and for the chief.
    pub fn describe(&self, mayor: &str) -> String {
        match self {
            Motion::ReplaceMayor { candidate } => format!(
                "replace the mayor: `{candidate}` would become \
                 mayor, and `{mayor}`, the mayor now, would stay as \
                 an ordinary agent"
            ),
            Motion::FoundHamlet {
                hamlet,
                mayor: new,
                purpose,
            } => format!(
                "found a new hamlet `{hamlet}` under a new mayor \
                 `{new}`, whose purpose would be: {purpose} This \
                 hamlet and its mayor `{mayor}` stay as they are"
            ),
        }
    }
}

/// A voter's answer to a motion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Yes,
    No,
    Abstain,
}

impl Choice {
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_lowercase().as_str() {
            "yes" => Some(Choice::Yes),
            "no" => Some(Choice::No),
            "abstain" => Some(Choice::Abstain),
            _ => None,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Choice::Yes => "yes",
            Choice::No => "no",
            Choice::Abstain => "abstain",
        }
    }
}

/// One voter's answer.
#[derive(Clone, Debug, PartialEq)]
pub struct Ballot {
    pub voter: String,
    /// `None` when the voter failed or never answered.
    pub choice: Option<Choice>,
    pub reason: String,
    /// The part of its own house the voter says its choice rests on.
    pub evidence: String,
    /// True when `evidence` was found in the voter's notes or
    /// history.
    pub grounded: bool,
}

/// The state of a vote.
#[derive(Clone, Debug, PartialEq)]
pub enum Standing {
    Open,
    Passed,
    /// Reason it did not pass.
    Failed(String),
    /// Passed, then reversed by a chief (rule one).
    Overturned,
}

/// A vote as the record tells it.
#[derive(Clone, Debug, PartialEq)]
pub struct Vote {
    pub id: u64,
    pub motion: Motion,
    /// The agent that called it.
    pub by: String,
    pub reason: String,
    /// The mayor at the time it was called.
    pub mayor: String,
    /// The agents asked to vote, fixed when it was called.
    pub voters: Vec<String>,
    /// The mayor's words on the motion, said once, before the
    /// ballots.
    pub answer: Option<String>,
    pub ballots: Vec<Ballot>,
    pub standing: Standing,
    called: u64,
}

/// The count of a vote's ballots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tally {
    pub yes: usize,
    pub no: usize,
    pub abstain: usize,
    /// Voters with no ballot, or one that failed.
    pub silent: usize,
    /// Yes ballots whose evidence was found in the voter's house.
    pub grounded_yes: usize,
    /// Yes ballots it takes to pass.
    pub needed: usize,
}

impl Vote {
    pub fn tally(&self, rules: &Rules) -> Tally {
        let count = |c: Choice| {
            self.ballots
                .iter()
                .filter(|b| b.choice == Some(c))
                .count()
        };
        let (yes, no, abstain) = (
            count(Choice::Yes),
            count(Choice::No),
            count(Choice::Abstain),
        );
        let (of, from) = rules.pass;
        Tally {
            yes,
            no,
            abstain,
            silent: self.voters.len() - yes - no - abstain,
            grounded_yes: self
                .ballots
                .iter()
                .filter(|b| {
                    b.choice == Some(Choice::Yes) && b.grounded
                })
                .count(),
            needed: (self.voters.len() * of as usize)
                .div_ceil(from as usize)
                .max(1),
        }
    }

    pub fn passes(&self, rules: &Rules) -> bool {
        let tally = self.tally(rules);
        let yes = if rules.grounded_only {
            tally.grounded_yes
        } else {
            tally.yes
        };
        yes >= tally.needed
    }

    /// Voters who have no ballot yet.
    pub fn waiting(&self) -> Vec<String> {
        self.voters
            .iter()
            .filter(|v| !self.ballots.iter().any(|b| &b.voter == *v))
            .cloned()
            .collect()
    }

    /// The whole vote with every ballot and its reason, for a chief.
    pub fn report(&self, rules: &Rules) -> String {
        let t = self.tally(rules);
        let standing = match &self.standing {
            Standing::Open => "Open".to_string(),
            Standing::Passed => "Passed".to_string(),
            Standing::Failed(why) => format!("Failed ({why})"),
            Standing::Overturned => {
                "Passed, then overturned by a chief".to_string()
            }
        };
        let mut out = format!(
            "Vote {}: {}.\nCalled by `{}`: {}\n{standing}: {} yes \
             of {} agents ({} needed), {} no, {} abstained, {} did \
             not answer.\n",
            self.id,
            self.motion.describe(&self.mayor),
            self.by,
            match &self.answer {
                Some(answer) => format!(
                    "{}\nThe mayor answered: {answer}",
                    self.reason
                ),
                None => self.reason.clone(),
            },
            t.yes,
            self.voters.len(),
            t.needed,
            t.no,
            t.abstain,
            t.silent,
        );
        for b in &self.ballots {
            let choice = b.choice.map_or("no answer", Choice::word);
            out.push_str(&format!(
                "- {}: {choice}. {}",
                b.voter, b.reason
            ));
            if !b.evidence.is_empty() {
                out.push_str(&format!(
                    " [{} its house: \"{}\"]",
                    if b.grounded {
                        "found in"
                    } else {
                        "not found in"
                    },
                    b.evidence
                ));
            }
            out.push('\n');
        }
        out
    }
}

/// Every vote the hamlet has held, oldest first.
pub fn votes(hamlet: &Hamlet) -> Vec<Vote> {
    let mut votes = Vec::<Vote>::new();
    for e in &hamlet.events() {
        let id = e["id"].as_u64().unwrap_or_default();
        let found = votes.iter_mut().find(|v| v.id == id);
        match (e["type"].as_str(), found) {
            (Some("vote_called"), None) => {
                let Some(motion) = Motion::from_json(&e["motion"])
                else {
                    continue;
                };
                votes.push(Vote {
                    id,
                    motion,
                    by: text(e, "by"),
                    reason: text(e, "reason"),
                    mayor: text(e, "mayor"),
                    voters: e["voters"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect(),
                    answer: None,
                    ballots: Vec::new(),
                    standing: Standing::Open,
                    called: e["ts"].as_u64().unwrap_or_default(),
                });
            }
            (Some("ballot"), Some(vote)) => {
                vote.ballots.push(Ballot {
                    voter: text(e, "voter"),
                    choice: e["choice"]
                        .as_str()
                        .and_then(Choice::parse),
                    reason: text(e, "reason"),
                    evidence: text(e, "evidence"),
                    grounded: e["grounded"]
                        .as_bool()
                        .unwrap_or(false),
                })
            }
            (Some("vote_answered"), Some(vote)) => {
                vote.answer = Some(text(e, "answer"))
            }
            (Some("vote_closed"), Some(vote)) => {
                vote.standing = if e["passed"] == true {
                    Standing::Passed
                } else {
                    Standing::Failed(text(e, "why"))
                }
            }
            (Some("vote_overturned"), Some(vote)) => {
                vote.standing = Standing::Overturned
            }
            _ => {}
        }
    }
    votes
}

/// The vote `id` of the hamlet.
pub fn find(hamlet: &Hamlet, id: u64) -> Result<Vote> {
    votes(hamlet)
        .into_iter()
        .find(|v| v.id == id)
        .ok_or_else(|| anyhow::anyhow!("no vote {id} in this hamlet"))
}

fn close(hamlet: &Hamlet, id: u64, why: Option<&str>) -> Result<()> {
    hamlet.append(json!({
        "v": 1,
        "type": "vote_closed",
        "ts": log::now_millis(),
        "id": id,
        "passed": why.is_none(),
        "why": why.unwrap_or_default(),
    }))
}

/// The most the mayor says to a motion, in characters.
const ANSWER: usize = 600;

/// Records what the mayor said to the motion of the open vote `id`.
/// It is shown to every voter beside the caller's reason.
pub fn answer(hamlet: &Hamlet, id: u64, said: &str) -> Result<()> {
    let vote = find(hamlet, id)?;
    if vote.standing != Standing::Open || vote.answer.is_some() {
        bail!("vote {id} takes no answer now");
    }
    let said = clip(said, ANSWER);
    if said.is_empty() {
        bail!("the mayor said nothing");
    }
    hamlet.append(json!({
        "v": 1,
        "type": "vote_answered",
        "ts": log::now_millis(),
        "id": id,
        "answer": said,
    }))
}

/// The name of a chief as the caller of a vote.
pub fn chief(user: &str) -> String {
    format!("chief:{user}")
}

/// Calls a vote. `by` is an agent of the hamlet, or a chief as
/// [`chief`] names one. The voters are the hamlet's agents besides
/// the mayor, as they stand now.
pub fn call(
    hamlet: &Hamlet,
    rules: &Rules,
    by: &str,
    motion: Motion,
    reason: &str,
) -> Result<Vote> {
    if !by.starts_with("chief:") && hamlet.member(by).is_none() {
        bail!("`{by}` does not live in this hamlet");
    }
    if reason.trim().is_empty() {
        bail!("say why in `reason`; every voter is shown it");
    }
    let now = log::now_millis();
    let held = votes(hamlet);
    if let Some(open) =
        held.iter().find(|v| v.standing == Standing::Open)
    {
        if now.saturating_sub(open.called) < rules.open_ms {
            bail!(
                "vote {} is still open ({}); only one vote is held \
                 at a time",
                open.id,
                open.motion.describe(&open.mayor)
            );
        }
        // Left open by a run that never finished.
        close(hamlet, open.id, Some("lapsed"))?;
    }
    let again = held.iter().rev().find(|v| {
        v.by == by
            && v.motion.kind() == motion.kind()
            && matches!(v.standing, Standing::Failed(_))
            && now.saturating_sub(v.called) < rules.retry_ms
    });
    if let Some(lost) = again {
        bail!(
            "your vote {} of this kind failed a short while ago; \
             you cannot call another yet",
            lost.id
        );
    }
    let mayor = hamlet.mayor().name;
    let voters = hamlet
        .residents()
        .into_iter()
        .map(|r| r.name)
        .collect::<Vec<_>>();
    if voters.len() < rules.least_voters {
        bail!(
            "a vote needs at least {} agents besides the mayor, and \
             this hamlet has {}; ask the chief instead",
            rules.least_voters,
            voters.len()
        );
    }
    let realm = Realm::with(hamlet.home().to_path_buf(), *rules);
    match &motion {
        Motion::ReplaceMayor { candidate } => {
            if candidate == &mayor {
                bail!("`{candidate}` is the mayor already");
            }
            if hamlet.member(candidate).is_none() {
                bail!(
                    "the candidate must be an agent of this hamlet, \
                     and `{candidate}` is not"
                );
            }
        }
        Motion::FoundHamlet {
            hamlet: name,
            mayor: new,
            purpose,
        } => {
            realm.may_found(name, new, purpose)?;
        }
    }
    let id = held.last().map_or(1, |v| v.id + 1);
    hamlet.append(json!({
        "v": 1,
        "type": "vote_called",
        "ts": now,
        "id": id,
        "by": by,
        "reason": reason.trim(),
        "mayor": mayor,
        "motion": motion.to_json(),
        "voters": voters,
    }))?;
    find(hamlet, id)
}

/// Records the ballot of `voter` in the open vote `id`.
pub fn cast(
    hamlet: &Hamlet,
    id: u64,
    voter: &str,
    choice: &str,
    reason: &str,
    evidence: &str,
    grounded: bool,
) -> Result<Choice> {
    let Some(choice) = Choice::parse(choice) else {
        bail!("`choice` is one of yes, no and abstain");
    };
    if choice != Choice::Abstain && reason.trim().is_empty() {
        bail!("a yes or a no needs your `reason`");
    }
    record(
        hamlet,
        id,
        voter,
        Some(choice),
        reason,
        evidence,
        grounded,
    )?;
    Ok(choice)
}

/// Records that `voter` failed or never answered, and why.
pub fn missed(
    hamlet: &Hamlet,
    id: u64,
    voter: &str,
    why: &str,
) -> Result<()> {
    record(hamlet, id, voter, None, why, "", false)
}

fn record(
    hamlet: &Hamlet,
    id: u64,
    voter: &str,
    choice: Option<Choice>,
    reason: &str,
    evidence: &str,
    grounded: bool,
) -> Result<()> {
    let vote = find(hamlet, id)?;
    if vote.standing != Standing::Open {
        bail!("vote {id} is closed");
    }
    if !vote.voters.iter().any(|v| v == voter) {
        bail!("`{voter}` is not a voter in vote {id}");
    }
    if vote.ballots.iter().any(|b| b.voter == voter) {
        bail!("`{voter}` has already voted in vote {id}");
    }
    hamlet.append(json!({
        "v": 1,
        "type": "ballot",
        "ts": log::now_millis(),
        "id": id,
        "voter": voter,
        "choice": choice.map(Choice::word),
        "reason": reason.trim(),
        "evidence": evidence.trim(),
        "grounded": grounded,
    }))
}

/// Says why nothing else is decided in the hamlet just now, if a vote
/// is open in it and has not lapsed.
pub fn wait(hamlet: &Hamlet, rules: &Rules) -> Result<()> {
    let now = log::now_millis();
    let open = votes(hamlet).into_iter().find(|vote| {
        vote.standing == Standing::Open
            && now.saturating_sub(vote.called) < rules.open_ms
    });
    match open {
        Some(vote) => bail!(
            "vote {} is open ({}); nothing else is decided in this \
             hamlet until it closes. Try again then",
            vote.id,
            vote.motion.describe(&vote.mayor)
        ),
        None => Ok(()),
    }
}

/// Counts an open vote, carries it out if it passed, and closes it.
/// Voters without a ballot count as not having said yes.
pub fn settle(
    hamlet: &Hamlet,
    rules: &Rules,
    id: u64,
) -> Result<Vote> {
    let vote = find(hamlet, id)?;
    if vote.standing != Standing::Open {
        bail!("vote {id} is closed");
    }
    let why = if !vote.passes(rules) {
        Some("too few said yes".to_string())
    } else {
        enact(hamlet, rules, &vote)
            .err()
            .map(|e| format!("it passed but could not be done: {e}"))
    };
    close(hamlet, id, why.as_deref())?;
    find(hamlet, id)
}

fn enact(hamlet: &Hamlet, rules: &Rules, vote: &Vote) -> Result<()> {
    let by = format!("vote:{}", vote.id);
    match &vote.motion {
        Motion::ReplaceMayor { candidate } => {
            if hamlet.mayor().name != vote.mayor {
                bail!("the mayor changed while the vote was open");
            }
            hamlet.set_mayor(candidate, &by, &vote.reason)?;
        }
        Motion::FoundHamlet {
            hamlet: name,
            mayor,
            purpose,
        } => {
            Realm::with(hamlet.home().to_path_buf(), *rules).found(
                name,
                mayor,
                purpose,
                Origin::Vote {
                    hamlet: hamlet.name().to_string(),
                    id: vote.id,
                },
            )?;
        }
    }
    Ok(())
}

/// A chief reverses a vote that passed (rule one): the mayor it
/// replaced takes the office back, or the hamlet it founded is
/// removed and archived.
pub fn overturn(
    hamlet: &Hamlet,
    rules: &Rules,
    id: u64,
    chief: &str,
    reason: &str,
) -> Result<String> {
    let vote = find(hamlet, id)?;
    let realm = Realm::with(hamlet.home().to_path_buf(), *rules);
    if !realm.is_chief(hamlet.name(), chief) {
        bail!("only a chief of this hamlet can overturn its votes");
    }
    let done = match (&vote.standing, &vote.motion) {
        (Standing::Passed, Motion::ReplaceMayor { candidate }) => {
            if &hamlet.mayor().name != candidate {
                bail!(
                    "the mayor has changed again since vote {id}; \
                     name the mayor you want with /mayor"
                );
            }
            hamlet.set_mayor(
                &vote.mayor,
                &format!("chief:{chief}"),
                reason,
            )?;
            format!("`{}` is the mayor again.", vote.mayor)
        }
        (
            Standing::Passed,
            Motion::FoundHamlet { hamlet: name, .. },
        ) => {
            realm.remove(name, chief, reason)?;
            format!("The hamlet `{name}` is removed and archived.")
        }
        (Standing::Open, _) => {
            close(hamlet, id, Some("stopped by a chief"))?;
            return Ok(format!("Vote {id} is stopped."));
        }
        (Standing::Overturned, _) => {
            bail!("vote {id} is already overturned")
        }
        (Standing::Failed(_), _) => bail!(
            "vote {id} did not pass, so nothing was done. To do \
             what it asked, use /mayor or /found"
        ),
    };
    hamlet.append(json!({
        "v": 1,
        "type": "vote_overturned",
        "ts": log::now_millis(),
        "id": id,
        "by": chief,
        "reason": reason.trim(),
    }))?;
    Ok(format!("Vote {id} is overturned. {done}"))
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;
    use crate::hamlet::{FOUNDER, HOME};

    fn hamlet(tag: &str, agents: &[&str]) -> (Scratch, Hamlet) {
        let dir = Scratch::new(&format!("vote-{tag}"));
        let hamlet = Hamlet::new(dir.to_path_buf());
        for name in agents {
            hamlet.create(name, "Works.", "test").unwrap();
        }
        (dir, hamlet)
    }

    fn replace(candidate: &str) -> Motion {
        Motion::ReplaceMayor {
            candidate: candidate.into(),
        }
    }

    fn yes(hamlet: &Hamlet, id: u64, voter: &str) {
        cast(hamlet, id, voter, "yes", "it is time", "", false)
            .unwrap();
    }

    #[test]
    fn two_thirds_of_all_the_agents_must_say_yes() {
        let rules = Rules::default();
        let (_dir, hamlet) = hamlet("thirds", &["a", "b", "c", "d"]);
        let vote =
            call(&hamlet, &rules, "a", replace("b"), "x").unwrap();
        assert_eq!(vote.voters, ["a", "b", "c", "d"]);
        assert_eq!(vote.tally(&rules).needed, 3);
        yes(&hamlet, 1, "a");
        yes(&hamlet, 1, "b");
        cast(&hamlet, 1, "c", "abstain", "", "", false).unwrap();
        // The fourth never answers: two of four is a tie, not a
        // pass.
        let vote = settle(&hamlet, &rules, 1).unwrap();
        assert!(matches!(vote.standing, Standing::Failed(_)));
        assert_eq!(vote.tally(&rules).silent, 1);
        assert_eq!(hamlet.mayor().name, FOUNDER);
    }

    #[test]
    fn a_passed_vote_replaces_the_mayor_and_a_chief_can_overturn_it()
    {
        let rules = Rules::default();
        let (_dir, hamlet) = hamlet("pass", &["a", "b", "c"]);
        call(&hamlet, &rules, "a", replace("b"), "x").unwrap();
        yes(&hamlet, 1, "a");
        yes(&hamlet, 1, "b");
        missed(&hamlet, 1, "c", "timed out").unwrap();
        let vote = settle(&hamlet, &rules, 1).unwrap();
        assert_eq!(vote.standing, Standing::Passed);
        assert_eq!(hamlet.mayor().name, "b");
        assert!(hamlet.residents().iter().any(|r| r.name == FOUNDER));
        assert!(vote.report(&rules).contains("- c: no answer"));

        let done =
            overturn(&hamlet, &rules, 1, "alice", "no").unwrap();
        assert!(done.contains("mayor again"));
        assert_eq!(hamlet.mayor().name, FOUNDER);
        assert_eq!(
            find(&hamlet, 1).unwrap().standing,
            Standing::Overturned
        );
        assert!(overturn(&hamlet, &rules, 1, "alice", "no").is_err());
    }

    #[test]
    fn bad_calls_are_refused_before_anyone_votes() {
        let rules = Rules::default();
        let (_dir, small) = hamlet("small", &["a", "b"]);
        // Too few agents for a vote to mean anything.
        assert!(
            call(&small, &rules, "a", replace("b"), "x").is_err()
        );

        let (_dir, hamlet) = hamlet("calls", &["a", "b", "c"]);
        let mayor = replace(FOUNDER);
        assert!(call(&hamlet, &rules, "a", mayor, "x").is_err());
        assert!(
            call(&hamlet, &rules, "a", replace("zed"), "x").is_err()
        );
        assert!(
            call(&hamlet, &rules, "a", replace("b"), "").is_err()
        );
        assert!(
            call(&hamlet, &rules, "zed", replace("b"), "x").is_err()
        );
        call(&hamlet, &rules, "a", replace("b"), "x").unwrap();
        // One vote at a time.
        let second = call(&hamlet, &rules, "c", replace("a"), "x");
        assert!(
            second.unwrap_err().to_string().contains("still open")
        );
        // One ballot each, from voters only, with a reason.
        yes(&hamlet, 1, "a");
        assert!(
            cast(&hamlet, 1, "a", "no", "again", "", false).is_err()
        );
        assert!(
            cast(&hamlet, 1, FOUNDER, "no", "mine", "", false)
                .is_err()
        );
        assert!(
            cast(&hamlet, 1, "b", "yes", " ", "", false).is_err()
        );
        assert!(
            cast(&hamlet, 1, "b", "maybe", "x", "", false).is_err()
        );
        settle(&hamlet, &rules, 1).unwrap();
        assert!(
            cast(&hamlet, 1, "b", "yes", "x", "", false).is_err()
        );
        // The caller who lost cannot call the same kind again at
        // once; another agent can.
        assert!(
            call(&hamlet, &rules, "a", replace("c"), "x").is_err()
        );
        call(&hamlet, &rules, "c", replace("a"), "x").unwrap();
    }

    #[test]
    fn a_chief_calls_a_vote_and_the_mayor_answers_it_once() {
        let rules = Rules::default();
        let (_dir, hamlet) = hamlet("answer", &["a", "b", "c"]);
        // Only an agent of the hamlet or a chief calls one.
        assert!(
            call(&hamlet, &rules, "stranger", replace("b"), "x")
                .is_err()
        );
        let vote =
            call(&hamlet, &rules, &chief("alice"), replace("b"), "x")
                .unwrap();
        assert_eq!(vote.by, "chief:alice");
        assert!(answer(&hamlet, 1, "  ").is_err());
        answer(&hamlet, 1, "The budget was\n over pace.").unwrap();
        // It has one say, and every voter is shown it as a claim.
        assert!(answer(&hamlet, 1, "And another thing.").is_err());
        let vote = find(&hamlet, 1).unwrap();
        let asked = ballot_text(&vote, &[], &[], &rules);
        assert!(asked.contains(&format!(
            "The mayor `{FOUNDER}` answers: The budget was over \
             pace."
        )));
        assert!(vote.report(&rules).contains("The mayor answered"));
        settle(&hamlet, &rules, 1).unwrap();
        assert!(answer(&hamlet, 1, "Too late.").is_err());
    }

    #[test]
    fn nothing_else_is_decided_while_a_vote_is_open() {
        let rules = Rules::default();
        let (_dir, hamlet) = hamlet("wait", &["a", "b", "c"]);
        wait(&hamlet, &rules).unwrap();
        call(&hamlet, &rules, "a", replace("b"), "x").unwrap();
        let held = wait(&hamlet, &rules).unwrap_err().to_string();
        assert!(held.contains("vote 1 is open"), "{held}");
        settle(&hamlet, &rules, 1).unwrap();
        wait(&hamlet, &rules).unwrap();
        // One that lapsed open holds nothing up.
        call(&hamlet, &rules, "c", replace("a"), "x").unwrap();
        let lapsed = Rules {
            open_ms: 0,
            ..Rules::default()
        };
        wait(&hamlet, &lapsed).unwrap();
    }

    #[test]
    fn a_vote_left_open_lapses() {
        let rules = Rules {
            open_ms: 0,
            ..Rules::default()
        };
        let (_dir, hamlet) = hamlet("lapse", &["a", "b", "c"]);
        call(&hamlet, &rules, "a", replace("b"), "x").unwrap();
        call(&hamlet, &rules, "c", replace("a"), "x").unwrap();
        assert_eq!(
            find(&hamlet, 1).unwrap().standing,
            Standing::Failed("lapsed".into())
        );
        assert_eq!(
            find(&hamlet, 2).unwrap().standing,
            Standing::Open
        );
    }

    #[test]
    fn a_mayor_changed_during_the_vote_is_not_replaced_by_it() {
        let rules = Rules::default();
        let (_dir, hamlet) = hamlet("during", &["a", "b", "c"]);
        call(&hamlet, &rules, "a", replace("b"), "x").unwrap();
        for voter in ["a", "b", "c"] {
            yes(&hamlet, 1, voter);
        }
        hamlet.set_mayor("c", "chief:alice", "mine").unwrap();
        let vote = settle(&hamlet, &rules, 1).unwrap();
        assert!(matches!(vote.standing, Standing::Failed(_)));
        assert_eq!(hamlet.mayor().name, "c");
    }

    #[test]
    fn a_vote_founds_a_hamlet_within_the_limits() {
        let rules = Rules {
            most_hamlets: 2,
            ..Rules::default()
        };
        let (_dir, hamlet) = hamlet("found", &["a", "b", "c"]);
        let found = |name: &str, mayor: &str| Motion::FoundHamlet {
            hamlet: name.into(),
            mayor: mayor.into(),
            purpose: "Trade.".into(),
        };
        // The new mayor is a new agent.
        assert!(
            call(&hamlet, &rules, "a", found("mill", "b"), "x")
                .is_err()
        );
        call(&hamlet, &rules, "a", found("mill", "miller"), "x")
            .unwrap();
        for voter in ["a", "b", "c"] {
            yes(&hamlet, 1, voter);
        }
        let vote = settle(&hamlet, &rules, 1).unwrap();
        assert_eq!(vote.standing, Standing::Passed);
        let realm = Realm::with(hamlet.home().to_path_buf(), rules);
        let mill = realm.hamlet("mill").unwrap();
        assert_eq!(mill.mayor().name, "miller");
        // The mayor of the hamlet the vote was held in never left.
        assert_eq!(hamlet.mayor().name, FOUNDER);
        assert_eq!(
            realm.hamlet_of("miller").as_deref(),
            Some("mill")
        );
        // It belongs to the chiefs of the hamlet it came from (C7).
        assert_eq!(realm.chiefs("mill"), realm.chiefs(HOME));

        // The limits allow no more.
        let full =
            call(&hamlet, &rules, "b", found("dock", "d"), "x");
        assert!(full.unwrap_err().to_string().contains("no more"));

        // Overturning a founding removes and archives the hamlet.
        overturn(&hamlet, &rules, 1, "alice", "no").unwrap();
        assert!(realm.hamlet("mill").is_err());
        assert!(!hamlet.root().join("hamlets/mill").exists());
        assert_eq!(realm.archived().len(), 1);
    }

    #[test]
    fn evidence_is_checked_against_the_house() {
        let rules = Rules::default();
        let corpus =
            "# Notes\nThe mayor ignored my road request three times.";
        assert!(grounded(
            "\"the mayor ignored my road request\"",
            corpus,
            &rules
        ));
        assert!(!grounded(
            "the mayor is slow and unfair",
            corpus,
            &rules
        ));
        assert!(!grounded("mayor", corpus, &rules));
    }

    #[test]
    fn a_ballot_is_asked_from_the_house_and_sealed() {
        let rules = Rules::default();
        let (_dir, hamlet) = hamlet("ballot", &["a", "b", "c"]);
        let house = hamlet.house("a").unwrap();
        std::fs::write(
            house.sessions().join("mayor.jsonl"),
            "{\"type\":\"user\",\"ts\":1,\"user\":\"mayor\",\"\
             content\":\"Sort the \
             list.\"}\n{\"type\":\"assistant\",\"ts\":2,\"message\":\
             {\"content\":\"Sorted.\"}}\n",
        )
        .unwrap();
        std::fs::write(
            house.sessions().join("vote_1.jsonl"),
            "{\"type\":\"user\",\"ts\":3,\"user\":\"hamlet\",\"\
             content\":\"A vote\"}\n",
        )
        .unwrap();
        let said = history(&house, &rules);
        assert_eq!(
            said,
            [
                "- mayor said to you: Sort the list.",
                "- you answered: Sorted."
            ]
        );
        let vote =
            call(&hamlet, &rules, "a", replace("b"), "slow").unwrap();
        let text = ballot_text(&vote, &said, &[], &rules);
        assert!(text.contains("Sort the list."));
        assert!(text.contains("`b` would become mayor"));
        assert!(!text.contains("ballots cast so far"));
    }
}
