//! A vote as the agent that calls it holds it: the mayor's one
//! answer, a ballot from each voter in its own house, and the count.

use std::time::Duration;

use anyhow::{Result, bail};
use hamlet_rules::budget;
use hamlet_rules::civic::Civic;
use hamlet_rules::hamlet::Hamlet;
use hamlet_rules::vote::{self, Choice, Motion, Standing};

use crate::agent::{Agent, Event, Listener};

/// A ballot being asked of an agent.
pub(super) struct Polling {
    /// The vote it is cast in.
    pub(super) id: u64,
    /// The voter's notes and history, to check its evidence against.
    pub(super) corpus: String,
    pub(super) cast: Option<Choice>,
}

impl Agent {
    /// Holds a vote on `motion`, called by this agent or by `chief`:
    /// hears the mayor once, asks every voter in turn, counts and
    /// carries the result out. Returns the count alone.
    pub async fn hold_vote(
        &mut self,
        motion: Motion,
        reason: &str,
        chief: Option<&str>,
        on: &mut Listener<'_>,
    ) -> Result<String> {
        self.here()?;
        let (Some(hamlet), Some(me)) = (
            self.hamlet.as_ref().map(Hamlet::again),
            self.resident.as_ref().map(|r| r.name.clone()),
        ) else {
            bail!("not in a hamlet");
        };
        let by = chief.map_or(me, vote::chief);
        let rules = self.rules;
        let place = hamlet.name().to_string();
        let vote = vote::call(&hamlet, &rules, &by, motion, reason)?;
        on(Event::Civic(Civic::VoteCalled {
            hamlet: place.clone(),
            id: vote.id,
            by,
            motion: vote.motion.describe(&vote.mayor),
        }));
        // Its own task, so the ballots are not held to the caps of
        // the caller's.
        let task = budget::new_task("vote");
        // The mayor is heard once before anyone votes on it. A
        // mayor that says nothing is voted on all the same.
        let replaces =
            matches!(vote.motion, Motion::ReplaceMayor { .. });
        if replaces && vote.by != vote.mayor {
            let _ = self.hear(&hamlet, vote.id, &task, on).await;
        }
        for voter in &vote.voters {
            let asked =
                self.poll(&hamlet, vote.id, voter, &task, on).await;
            let choice = match asked {
                Ok(Some(choice)) => Some(choice),
                Ok(None) => {
                    let _ = vote::missed(
                        &hamlet,
                        vote.id,
                        voter,
                        "answered without casting a ballot",
                    );
                    None
                }
                Err(e) => {
                    let why = format!("could not vote: {e}");
                    let _ =
                        vote::missed(&hamlet, vote.id, voter, &why);
                    None
                }
            };
            on(Event::Civic(Civic::Ballot {
                hamlet: place.clone(),
                voter: voter.clone(),
                choice,
            }));
        }
        let closed = vote::settle(&hamlet, &rules, vote.id)?;
        let tally = closed.tally(&rules);
        let passed = closed.standing == Standing::Passed;
        let summary = format!(
            "{} yes of {} agents ({} needed), {} no, {} abstained, \
             {} did not answer{}",
            tally.yes,
            closed.voters.len(),
            tally.needed,
            tally.no,
            tally.abstain,
            tally.silent,
            match &closed.standing {
                Standing::Failed(why) if closed.passes(&rules) => {
                    format!("; {why}")
                }
                _ => String::new(),
            }
        );
        on(Event::Civic(Civic::VoteClosed {
            hamlet: place.clone(),
            id: closed.id,
            passed,
            summary: summary.clone(),
        }));
        if passed {
            on(Event::Civic(match &closed.motion {
                Motion::ReplaceMayor { candidate } => {
                    Civic::MayorChanged {
                        hamlet: place,
                        from: closed.mayor.clone(),
                        to: candidate.clone(),
                    }
                }
                Motion::FoundHamlet { hamlet, mayor, .. } => {
                    Civic::HamletFounded {
                        name: hamlet.clone(),
                        mayor: mayor.clone(),
                    }
                }
            }));
        }
        Ok(format!(
            "Vote {} {}: {summary}. The chief has been told and may \
             overturn it.",
            closed.id,
            if passed { "passed" } else { "failed" },
        ))
    }

    /// Asks the mayor, in its own house, for its one answer to the
    /// motion of the vote `id`, and keeps it for the voters.
    async fn hear(
        &mut self,
        hamlet: &Hamlet,
        id: u64,
        task: &str,
        on: &mut Listener<'_>,
    ) -> Result<()> {
        let vote = vote::find(hamlet, id)?;
        let Some(resident) = hamlet.member(&vote.mayor) else {
            bail!("the mayor left the hamlet");
        };
        let house = hamlet.hall()?;
        let path = house
            .sessions()
            .join(format!("{}{id}_answer.jsonl", vote::BALLOTS));
        let mut agent = self.beside(house, &path)?;
        agent.hamlet = Some(hamlet.again());
        agent.resident = Some(resident);
        agent.task = task.to_string();
        agent.answering = true;
        let mayor = vote.mayor.clone();
        let mut relay = |event: Event| {
            on(Event::Agent {
                name: mayor.clone(),
                event: Box::new(event),
            })
        };
        // Boxed because the mayor's turn is this same code.
        let said = tokio::time::timeout(
            Duration::from_secs(self.rules.ballot_secs),
            Box::pin(agent.run(
                "hamlet",
                &vote::answer_text(&vote),
                &mut relay,
            )),
        )
        .await;
        match said {
            Err(_) => bail!("it did not answer in time"),
            Ok(said) => vote::answer(hamlet, id, &said?),
        }
    }

    /// Asks `voter` for its ballot in its own house. `None` when it
    /// answered without casting one.
    async fn poll(
        &mut self,
        hamlet: &Hamlet,
        id: u64,
        voter: &str,
        task: &str,
        on: &mut Listener<'_>,
    ) -> Result<Option<Choice>> {
        let vote = vote::find(hamlet, id)?;
        let Some(resident) = hamlet.member(voter) else {
            bail!("it left the hamlet");
        };
        let house = hamlet.house(voter)?;
        let said = vote::history(&house, &self.rules);
        let notes = std::fs::read_to_string(house.notes())
            .unwrap_or_default();
        let seen = if self.rules.sealed {
            Vec::new()
        } else {
            vote.ballots.clone()
        };
        let text =
            vote::ballot_text(&vote, &said, &seen, &self.rules);
        let path = house
            .sessions()
            .join(format!("{}{id}.jsonl", vote::BALLOTS));
        let mut agent = self.beside(house, &path)?;
        agent.hamlet = Some(hamlet.again());
        agent.resident = Some(resident);
        agent.task = task.to_string();
        agent.ballot = Some(Polling {
            id,
            corpus: format!("{notes}\n{}", said.join("\n")),
            cast: None,
        });
        let mut relay = |event: Event| {
            on(Event::Agent {
                name: voter.to_string(),
                event: Box::new(event),
            })
        };
        // Boxed because the voter's turn is this same code.
        let asked = tokio::time::timeout(
            Duration::from_secs(self.rules.ballot_secs),
            Box::pin(agent.run("hamlet", &text, &mut relay)),
        )
        .await;
        match asked {
            Err(_) => bail!("it did not answer in time"),
            Ok(Err(e)) => Err(e),
            Ok(Ok(_)) => Ok(agent.ballot.and_then(|b| b.cast)),
        }
    }
}
