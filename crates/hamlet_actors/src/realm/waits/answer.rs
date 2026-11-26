//! A chief's answer to a wait, and what the wait's kind adds to it.

use anyhow::Result;
use hamlet_messages::realm::Done;
use hamlet_messages::realm::client::{Answer, AnswerRefusal};
use hamlet_messages::realm::oversight::Permission;
use hamlet_messages::realm::world::WaitId;
use hamlet_messages::sender::Client;
use hamlet_messages::workspace::reach::Need;
use hamlet_messages::workspace::{
    AgentNumber, CallId, ConversationId,
};
use hamlet_protocol::client as protocol;

use super::super::act::{Act, Decided, Decision};
use super::super::calls::{Calls, Made};
use super::super::change::{self, Rows, number};
use super::super::oversight::{asking, reserved};
use super::super::profiles::lifecycle;
use super::super::tasks::TaskId;
use super::super::{oversight, parcels, record, versions};
use super::{
    Change, Choice, Kind, Ran, Refused, Scope, Told, most_refusals,
    named, needs_of, past_the_count, refusals,
};

/// What the unit of a wait's kind adds to a chief's answer: the
/// changes and decisions committed with the close, the words the
/// chief's picks stand for, and the words the wait's agent is told.
#[derive(Default)]
pub(in crate::realm) struct Ruling {
    pub(in crate::realm) changes: Vec<change::Change>,
    pub(in crate::realm) entries: Vec<record::Entry>,
    pub(in crate::realm) picked: Option<String>,
    pub(in crate::realm) told: Option<String>,
    /// The wait's agent is told nothing of the ruling.
    pub(in crate::realm) unheard: bool,
    /// The allowed act runs after the close is committed, and its
    /// notice tells the agent what it did.
    pub(in crate::realm) deferred: bool,
}

/// The command a chief answers with `choice`.
fn command(choice: &protocol::Choice) -> &'static str {
    use protocol::Choice as Chief;
    match choice {
        Chief::Allow => "allow",
        Chief::Always => "always",
        Chief::Deny => "deny",
        Chief::Go => "go",
        Chief::Withdraw => "withdraw",
        Chief::Reply => "reply",
        Chief::Dismiss => "dismiss",
        Chief::Welcome => "welcome",
    }
}

/// The open wait a chief answers.
pub(in crate::realm) struct Waiting {
    wait: WaitId,
    agent: AgentNumber,
    kind: Kind,
    task: Option<TaskId>,
    conversation: Option<ConversationId>,
    call: Option<CallId>,
    subject: String,
    needs: Vec<Need>,
    /// The agent's other open tool waits.
    others: Vec<Other>,
    /// For a refusal that reaches its task's count: the words the
    /// task's other waits close with.
    past_the_count: Option<String>,
    ruling: Result<Ruling, AnswerRefusal>,
}

/// Another open tool wait of the agent's, which a ruling may settle
/// with it.
struct Other {
    wait: WaitId,
    task: Option<TaskId>,
    conversation: Option<ConversationId>,
    subject: String,
    needs: Vec<Need>,
}

impl Waiting {
    fn takes(
        &self,
        choice: &protocol::Choice,
        text: Option<&str>,
    ) -> Result<Choice, AnswerRefusal> {
        use protocol::Choice as Chief;
        let answers = self.kind.answers(!self.needs.is_empty());
        let not_that_way = || AnswerRefusal::NotThatWay {
            answers: answers
                .iter()
                .map(|answer| format!("/{}", command(answer)))
                .collect::<Vec<_>>()
                .join(" "),
        };
        if !answers.contains(choice) {
            return Err(not_that_way());
        }
        Ok(match choice {
            Chief::Allow => Choice::Allow(Scope::Once),
            Chief::Always => Choice::Allow(Scope::Always),
            Chief::Deny => Choice::Refuse,
            Chief::Reply => match text {
                Some(_) => Choice::Reply,
                None => return Err(AnswerRefusal::BlankReply),
            },
            Chief::Dismiss => Choice::Dismiss,
            Chief::Go => Choice::Go,
            Chief::Withdraw => Choice::Withdraw,
            Chief::Welcome => Choice::Welcome,
        })
    }

    /// What an always on this tool wait keeps: a grant of the
    /// network, else a trust for its conversation.
    fn lasting(&self) -> Option<oversight::Change> {
        if self.needs.contains(&Need::Network) {
            return Some(oversight::Change::Grant {
                agent: self.agent,
                permission: Permission::Network,
            });
        }
        Some(oversight::Change::Trust {
            agent: self.agent,
            conversation: self.conversation.clone()?,
            edits: self.needs.contains(&Need::Edit),
            commands: self.needs.contains(&Need::Command),
        })
    }

    /// Whether the lasting change of an always on this wait allows
    /// `other` too: a grant every call for the network alone, a
    /// trust the conversation's calls for what it trusts.
    fn allows(&self, other: &Other) -> bool {
        if self.needs.contains(&Need::Network) {
            other.needs.iter().all(|need| *need == Need::Network)
        } else {
            self.conversation.is_some()
                && other.conversation == self.conversation
                && other
                    .needs
                    .iter()
                    .all(|need| self.needs.contains(need))
        }
    }

    /// The other waits this ruling settles, each with its close: on
    /// a refusal past the count every one on the task, on a refusal
    /// each on the task it covers, on an always each it allows.
    fn settled(
        &self,
        choice: &Choice,
        text: Option<&str>,
    ) -> Vec<(WaitId, Choice)> {
        let on_task = |other: &Other| {
            self.task.is_some() && other.task == self.task
        };
        let closing = |keep: &dyn Fn(&Other) -> bool,
                       close: Choice| {
            self.others
                .iter()
                .filter(|other| keep(other))
                .map(|other| (other.wait, close.clone()))
                .collect()
        };
        if let Some(words) = &self.past_the_count {
            return closing(&on_task, Choice::Unasked(words.clone()));
        }
        match choice {
            Choice::Refuse if self.kind == Kind::Tool => {
                let refused = Refused {
                    subject: self.subject.clone(),
                    needs: self.needs.clone(),
                    ruling: text.map(str::to_owned),
                };
                closing(
                    &|other| {
                        on_task(other)
                            && refused
                                .covers(&other.needs, &other.subject)
                    },
                    Choice::Unasked(refused.again().to_string()),
                )
            }
            Choice::Allow(Scope::Always)
                if self.kind == Kind::Tool =>
            {
                closing(
                    &|other| self.allows(other),
                    Choice::Allow(Scope::Once),
                )
            }
            _ => Vec::new(),
        }
    }

    fn told(&self, choice: &Choice) -> Told {
        let notices = vec![self.wait];
        match (self.kind, choice) {
            (Kind::Tool, _) => Told::workspace(self.wait),
            (Kind::Objection, Choice::Go) => Told {
                notices,
                held: self.task,
                ..Told::default()
            },
            (Kind::Question | Kind::Objection, _)
            | (Kind::Reserved | Kind::Send, Choice::Refuse) => Told {
                notices,
                ..Told::default()
            },
            _ => Told::default(),
        }
    }
}

impl Act<Client> for Answer {
    type Output = Done<AnswerRefusal>;
    type Loaded = Option<Waiting>;
    type Done = Told;

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Option<Waiting>> {
        let Some(wait) = named(&self.0.wait) else {
            return Ok(None);
        };
        let Some((
            agent,
            kind,
            task,
            conversation,
            call,
            subject,
            needs,
        )) = rows
            .row::<(
                u32,
                String,
                Option<TaskId>,
                Option<String>,
                Option<String>,
                String,
                Option<String>,
            )>(
                "SELECT agent, kind, task, conversation, call, \
                 subject, needs FROM wait WHERE id = ?1 AND \
                 closed_at IS NULL",
                vec![change::integer(wait.0)?],
            )
            .await?
        else {
            return Ok(None);
        };
        let agent = AgentNumber(agent);
        let kind = change::from_case_name::<Kind>(kind)?;
        let ruling = match kind {
            Kind::Question => asking::reply(rows, wait, self).await?,
            Kind::Objection => {
                Ok(asking::ruling(rows, wait, self).await?)
            }
            Kind::Reserved => {
                Ok(reserved::ruling(rows, wait, self).await?)
            }
            Kind::Send => {
                Ok(parcels::ruling(rows, wait, self).await?)
            }
            Kind::Newcomer => {
                Ok(lifecycle::newcomer_ruling(rows, wait, self)
                    .await?)
            }
            Kind::Tool | Kind::Fault => Ok(Ruling::default()),
        };
        let past_the_count = match (kind, &self.0.choice, task) {
            (
                Kind::Tool | Kind::Reserved,
                protocol::Choice::Deny,
                Some(task),
            ) => {
                let refused =
                    refusals(rows, agent, task).await?.len();
                let most = most_refusals(rows, agent).await?;
                past_the_count(refused + 1, most)
                    .map(|why| why.to_string())
            }
            _ => None,
        };
        let others = rows
            .rows::<(
                u64,
                Option<TaskId>,
                Option<String>,
                String,
                Option<String>,
            )>(
                "SELECT id, task, conversation, subject, needs FROM \
                 wait WHERE agent = ?1 AND kind = 'tool' AND \
                 closed_at IS NULL AND id != ?2 ORDER BY id",
                vec![number(agent), change::integer(wait.0)?],
            )
            .await?
            .into_iter()
            .map(|(other, task, conversation, subject, needs)| {
                Ok(Other {
                    wait: WaitId(other),
                    task,
                    conversation: conversation.map(ConversationId),
                    subject,
                    needs: needs_of(needs)?,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Some(Waiting {
            wait,
            agent,
            kind,
            task,
            others,
            past_the_count,
            ruling,
            conversation: conversation.map(ConversationId),
            call: call.map(CallId),
            subject,
            needs: needs_of(needs)?,
        }))
    }

    fn decide(
        &self,
        _by: &Client,
        open: Option<Waiting>,
    ) -> Decision<Told, Self::Output> {
        let refuse = |why| Decision::Answer(Done::Refused { why });
        let Some(mut open) = open else {
            return refuse(AnswerRefusal::NothingWaits {
                wait: self.0.wait.clone(),
            });
        };
        let ruling = match std::mem::replace(
            &mut open.ruling,
            Ok(Ruling::default()),
        ) {
            Ok(ruling) => ruling,
            Err(why) => return refuse(why),
        };
        let text = self
            .0
            .text
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty());
        let choice = match (&self.0.picks, &ruling.picked) {
            (_, Some(_)) => Ok(Choice::Picked),
            (Some(_), None) => open
                .takes(&self.0.choice, text)
                .and(Err(AnswerRefusal::PicksWithText)),
            (None, None) => open.takes(&self.0.choice, text),
        };
        let text = ruling.picked.as_deref().or(text);
        let choice = match choice {
            Ok(choice) => choice,
            Err(why) => return refuse(why),
        };
        let mut decided = Decided::new(open.told(&choice));
        if ruling.unheard {
            decided.done.notices.clear();
        }
        if ruling.deferred {
            decided.done.notices = vec![open.wait];
        }
        decided.done.ran = ruling.told.map(|text| Ran {
            wait: open.wait,
            agent: open.agent,
            task: open.task,
            conversation: open.conversation.clone(),
            call: open.call.clone(),
            text,
        });
        for change in ruling.changes {
            decided = decided.change(change);
        }
        for entry in ruling.entries {
            decided = decided.record(entry);
        }
        if choice == Choice::Allow(Scope::Always)
            && let Some(lasting) = open.lasting()
        {
            decided = decided
                .change(lasting)
                .change(versions::Change::WantReach(open.agent));
        }
        for (other, close) in open.settled(&choice, text) {
            decided.done.workspace.push(other);
            decided = decided.change(Change::Close {
                wait: other,
                choice: close,
                text: None,
            });
        }
        Decision::Change(decided.change(Change::Close {
            wait: open.wait,
            choice,
            text: text.map(str::to_owned),
        }))
    }

    fn deliveries(&self, _by: &Client, told: &Told) -> Calls {
        told.calls()
    }

    fn answer(&self, _told: Told, _made: &Made) -> Self::Output {
        Done::Done
    }
}
