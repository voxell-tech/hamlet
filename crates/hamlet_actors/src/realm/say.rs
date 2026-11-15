//! A chief's line to an agent.

use anyhow::Result;
use hamlet_messages::agent::{Input, InputId, Submit};
use hamlet_messages::realm::Refusable;
use hamlet_messages::realm::client::{Say, SayAnswer, SayRefusal};
use hamlet_messages::sender::Client;
use hamlet_messages::workspace::AgentNumber;

use super::act::{Act, Decided, Decision};
use super::calls::{Calls, Delivery, Made};
use super::change::Rows;
use super::tasks::{self, TaskId};
use super::tree::{self, Lineage, Status};
use super::{chiefs, hire};

pub(super) struct Target {
    lineage: Option<Lineage>,
    task: TaskId,
}

impl Act<Client> for Say {
    type Output = SayAnswer;
    type Loaded = Target;
    type Done = TaskId;

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Target> {
        let lineage = match to(self) {
            Some(to) => tree::lineage(rows, to).await?,
            None => None,
        };
        Ok(Target {
            lineage,
            task: tasks::fresh(rows).await?,
        })
    }

    fn decide(
        &self,
        _by: &Client,
        target: Target,
    ) -> Decision<TaskId, Self::Output> {
        let Some(lineage) = target.lineage else {
            return Decision::Answer(SayAnswer::refused(
                SayRefusal::NoSuchAgent,
            ));
        };
        let agent = &lineage.agent;
        match &agent.status {
            Status::Ended(left) => Decision::Answer(
                SayAnswer::refused(SayRefusal::Left {
                    agent: agent.agent,
                    left: left.clone(),
                }),
            ),
            Status::Hiring => Decision::WaitFor {
                row: hire::row(agent.agent),
                refusal: SayAnswer::refused(
                    SayRefusal::StillHiring { agent: agent.agent },
                ),
            },
            Status::Live => {
                let mut decided = Decided::new(target.task).change(
                    tasks::Change::Open {
                        task: target.task,
                        origin: tasks::Origin::Chief,
                        agent: agent.agent,
                        payer: chiefs::payer_of(&lineage),
                    },
                );
                if agent.can_take_purpose()
                    && let Some(purpose) =
                        tree::purpose_from(agent.agent, &self.0.text)
                {
                    decided = decided.change(purpose);
                }
                Decision::Change(decided)
            }
        }
    }

    fn deliveries(&self, _by: &Client, task: &TaskId) -> Calls {
        to(self).map_or_else(Calls::none, |to| {
            Calls::before_answer(Delivery::Line {
                to,
                submit: Submit {
                    id: InputId(self.0.id.as_str().to_owned()),
                    task: Some(task.on_the_wire()),
                    input: Input::Line {
                        text: self.0.text.clone(),
                        images: self.0.images.clone(),
                        files: self.0.files.clone(),
                    },
                },
            })
        })
    }

    fn answer(&self, task: TaskId, made: &Made) -> Self::Output {
        let task = task.on_the_wire();
        if made.all_taken() {
            SayAnswer::Taken { task }
        } else {
            SayAnswer::NotTaken { task }
        }
    }
}

fn to(say: &Say) -> Option<AgentNumber> {
    u32::try_from(say.0.to.0).ok().map(AgentNumber)
}

#[cfg(test)]
mod tests {
    use hamlet_actor::Action as _;
    use hamlet_messages::agent::AgentKind;
    use hamlet_messages::realm::budget::Payer;
    use hamlet_messages::realm::{HamletId, HowEnded, Left, Tier};
    use hamlet_messages::sender::Hand;
    use hamlet_messages::workspace::RealmTask;
    use hamlet_messages::workspace::reach::Mode;
    use hamlet_protocol::client::{
        self as protocol, AgentId, RequestId,
    };

    use super::*;
    use crate::realm::chiefs::limits;
    use crate::realm::testing::Started;
    use crate::realm::tree::Member;
    use crate::realm::{act, marker};

    fn task_7() -> TaskId {
        TaskId::of(&RealmTask("7".to_owned())).unwrap()
    }

    fn say(id: &RequestId, to: u64, text: &str) -> Say {
        Say(protocol::Say {
            files: Vec::new(),
            id: id.clone(),
            images: Vec::new(),
            text: text.to_owned(),
            to: AgentId(to),
        })
    }

    fn hamlet() -> HamletId {
        HamletId("h".to_owned())
    }

    fn lineage(agent: Member) -> Lineage {
        let hamlet =
            (agent.kind == AgentKind::Hire).then(|| chiefs::Hamlet {
                id: hamlet(),
                mayor: AgentNumber(2),
                tier: Tier::Fifty,
                limits: limits::Limits::default(),
                folder: None,
                mode: Mode::Manual,
            });
        Lineage {
            agent,
            leads: Vec::new(),
            hires: Vec::new(),
            hamlet,
        }
    }

    fn mayor(status: Status, has_purpose: bool) -> Lineage {
        lineage(Member {
            agent: AgentNumber(2),
            kind: AgentKind::Hire,
            status,
            lead: None,
            has_purpose,
        })
    }

    fn hand() -> Lineage {
        lineage(Member {
            agent: Hand::NUMBER,
            kind: AgentKind::Hand,
            status: Status::Live,
            lead: None,
            has_purpose: false,
        })
    }

    fn decide(
        lineage: Option<Lineage>,
        text: &str,
    ) -> Decision<TaskId, SayAnswer> {
        let to = lineage
            .as_ref()
            .map_or(9, |lineage| u64::from(lineage.agent.agent.0));
        let target = Target {
            lineage,
            task: task_7(),
        };
        say(&RequestId::new(), to, text).decide(&Client, target)
    }

    fn opened(agent: AgentNumber, payer: Payer) -> tasks::Change {
        tasks::Change::Open {
            task: task_7(),
            origin: tasks::Origin::Chief,
            agent,
            payer,
        }
    }

    #[test]
    fn a_hire_with_no_purpose_takes_the_lines_first_line() {
        let by_hamlet = Payer::Hamlet { hamlet: hamlet() };
        let text = "\n   build the mill  \nthen the barn";

        assert_eq!(
            decide(Some(mayor(Status::Live, false)), text),
            Decision::Change(
                Decided::new(task_7())
                    .change(opened(AgentNumber(2), by_hamlet.clone()))
                    .change(tree::Change::Purpose {
                        hire: AgentNumber(2),
                        text: "build the mill".to_owned(),
                    })
            )
        );
        let only_the_task = |agent, payer| {
            Decision::Change(
                Decided::new(task_7()).change(opened(agent, payer)),
            )
        };
        assert_eq!(
            decide(Some(mayor(Status::Live, true)), text),
            only_the_task(AgentNumber(2), by_hamlet.clone())
        );
        assert_eq!(
            decide(Some(mayor(Status::Live, false)), " \n\t\n"),
            only_the_task(AgentNumber(2), by_hamlet)
        );
        assert_eq!(
            decide(Some(hand()), text),
            only_the_task(Hand::NUMBER, Payer::Realm)
        );
    }

    #[test]
    fn a_line_to_a_hire_that_left_is_refused_with_how_and_why() {
        let left = Left {
            how: HowEnded::ByTheChief,
            why: Some("done".to_owned()),
        };

        assert_eq!(
            decide(
                Some(mayor(Status::Ended(left.clone()), false)),
                "hi"
            ),
            Decision::Answer(SayAnswer::refused(SayRefusal::Left {
                agent: AgentNumber(2),
                left,
            }))
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_line_the_agent_does_not_take_keeps_its_one_task() {
        // the started realm runs no agent, so every Submit fails: the
        // line is NotTaken, and a repeat answers the same task.
        let started = Started::new().await;
        started.found(&hamlet(), AgentNumber(2)).await;
        started
            .commit(tree::Change::Live {
                agent: AgentNumber(2),
            })
            .await
            .unwrap();
        let run = async |say: Say| {
            let id = marker::Id::Request(say.0.id.clone());
            act::run(
                &started.realm,
                &started.ctx,
                Client,
                Some(id),
                Say::NAME,
                say,
            )
            .await
            .unwrap()
        };
        let id = RequestId::new();

        let first = run(say(&id, 2, "build the mill")).await;
        let again = run(say(&id, 2, "build the mill")).await;
        let beyond =
            run(say(&RequestId::new(), u64::MAX, "hi")).await;
        let tasks = started
            .rows()
            .value::<u32>(
                "SELECT COUNT(*) FROM task WHERE origin = 'chief'",
                Vec::new(),
            )
            .await
            .unwrap();

        assert!(
            matches!(first, SayAnswer::NotTaken { .. }),
            "no agent takes the line: {first:?}"
        );
        assert_eq!(again, first);
        assert_eq!(
            beyond,
            SayAnswer::refused(SayRefusal::NoSuchAgent)
        );
        assert_eq!(tasks, Some(1));
        started.stop().await;
    }
}
