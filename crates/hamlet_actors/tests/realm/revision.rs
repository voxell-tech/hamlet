//! The world's revision, which moves only with what the world shows.

use std::time::Duration;

use hamlet_messages::agent::Input;
use hamlet_messages::realm::budget::BudgetLimit;
use hamlet_messages::realm::choices::{
    ModelOf, SetModel, SetNickname,
};
use hamlet_messages::realm::client::{ReadWorld, SayAnswer};
use hamlet_messages::realm::oversight::{
    AskChief, Lapse, LapseReason, Object, Permission, Question,
    Revoke, Revoked,
};
use hamlet_messages::realm::workers::{
    Ended, StartWorker, WorkerDone,
};
use hamlet_messages::realm::world::{
    ReachSeen, Revision, Seen, Status, WaitKind,
};
use hamlet_messages::realm::{
    Done, End, Limit, Named, ReservedAct, SetLimits,
};
use hamlet_messages::workspace::reach::Need;
use hamlet_messages::workspace::{
    AgentNumber, ConversationId, ReachVersion, RealmTask,
};
use hamlet_protocol::Effort;
use hamlet_protocol::client::{Choice, RequestId};

use super::asking::chief_task;
use super::world::{
    agent, allowance, asked, chief, failed, granted, seen_until,
    start, started_by_three,
};
use super::{
    MAYOR, Realm, THREE, answer, hamlet_with, hire, say, sent,
    standing, submits_to, tell, tool, until,
};

#[tokio::test(flavor = "multi_thread")]
async fn an_unrelated_commit_leaves_the_world_as_it_was() {
    let (realm, _) = hamlet_with(1).await;
    granted(&realm).await;
    let said = realm
        .ask(&say(&RequestId::new(), THREE, "Make the file."))
        .await
        .unwrap();
    let SayAnswer::Taken { task } = said else {
        panic!("hire 3 takes the chief's line: {said:?}");
    };
    let asked = tool(
        "q1",
        AskChief {
            task,
            conversation: ConversationId("c".to_owned()),
            message: None,
            questions: vec![Question {
                question: "Which seed?".to_owned(),
                description: None,
                options: Vec::new(),
                text_input: true,
            }],
        },
    );
    realm.tool(THREE, &asked).await.unwrap();
    let four = started_by_three(&realm, "w1").await;
    let done = WorkerDone {
        ended: Ended::Answered {
            text: "Counted.".to_owned(),
        },
    };
    realm.tool(four, &done).await.unwrap();
    let before =
        seen_until(&realm, "worker 4's end applied", |seen| {
            agent(seen, four).is_some_and(|worker| {
                worker.status != Status::Live
                    && worker.reach
                        == ReachSeen::Applied {
                            version: ReachVersion(2),
                        }
            })
        })
        .await;

    let told = realm
        .tool(
            MAYOR,
            &tool(
                "t1",
                tell(Named::Agent { agent: THREE }, "Hello."),
            ),
        )
        .await
        .unwrap();
    until("the letter reaches 3", || {
        submits_to(&realm, THREE).iter().any(|submit| {
            matches!(submit.input, Input::Letter { .. })
        })
    })
    .await;
    let asked_again = realm.tool(THREE, &asked).await.unwrap();
    realm.tool(four, &done).await.unwrap();
    let revoked = chief(
        &realm,
        Revoke {
            agent: MAYOR,
            permission: Permission::Network,
        },
    )
    .await;
    let after = realm.ask(&ReadWorld).await.unwrap();

    assert_eq!((told, asked_again), (sent(THREE), Done::Done));
    assert!(
        matches!(revoked, Revoked::NoGrant { .. }),
        "{revoked:?}"
    );
    assert_eq!((before.grants.len(), before.waits.len()), (1, 1));
    assert_eq!(after, before);
    realm.host.shutdown().await.unwrap();
}

/// The world once it holds still: the same read twice, 400 ms apart.
async fn settled(realm: &Realm) -> Seen {
    let mut last = realm.ask(&ReadWorld).await.unwrap();
    loop {
        tokio::time::sleep(Duration::from_millis(400)).await;
        let now = realm.ask(&ReadWorld).await.unwrap();
        if now == last {
            return now;
        }
        last = now;
    }
}

/// What `seen` shows, its revision aside.
fn shown(seen: &Seen) -> Seen {
    Seen {
        revision: Revision(0),
        ..seen.clone()
    }
}

/// The world after a step, checked: the copy is a restart's full
/// read, and the revision moved only if what the world shows changed.
async fn stepped(
    realm: Realm,
    step: &str,
    before: &Seen,
    faults: &mut Vec<String>,
) -> (Realm, Seen) {
    let after = settled(&realm).await;
    let realm = realm.restart().await;
    let restarted = settled(&realm).await;
    if after != restarted {
        faults.push(format!(
            "{step}: the copy {after:?} is not a restart's read \
             {restarted:?}"
        ));
    }
    let changed = shown(&after) != shown(before);
    let moved = after.revision != before.revision;
    if changed != moved {
        faults.push(format!(
            "{step}: shown changed {changed}, revision moved {moved}"
        ));
    }
    (realm, restarted)
}

fn wait_of(seen: &Seen, kind: fn(&WaitKind) -> bool) -> String {
    seen.waits
        .iter()
        .find(|wait| kind(&wait.kind))
        .unwrap_or_else(|| panic!("no such wait: {:?}", seen.waits))
        .wait
        .0
        .to_string()
}

fn lapse(why: LapseReason) -> Lapse {
    Lapse {
        conversation: Some(ConversationId("c".to_owned())),
        why,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn each_writer_moves_the_revision_only_with_the_world() {
    let (mut realm, hamlet) = hamlet_with(1).await;
    let mut faults = Vec::new();
    let mut before = settled(&realm).await;
    macro_rules! step {
        ($step:expr, $body:expr) => {{
            $body;
            (realm, before) =
                stepped(realm, $step, &before, &mut faults).await;
        }};
    }
    let is_tool =
        |kind: &WaitKind| matches!(kind, WaitKind::Tool { .. });
    let question = || Question {
        question: "Which?".to_owned(),
        description: None,
        options: Vec::new(),
        text_input: true,
    };

    step!("a network wait opens", {
        realm
            .call_as_workspace(&asked(
                THREE,
                "n1",
                Need::Network,
                None,
            ))
            .await
            .unwrap();
    });
    step!("always grants", {
        let wait = wait_of(&before, is_tool);
        realm
            .ask(&answer(&wait, Choice::Always, None))
            .await
            .unwrap();
    });
    step!("a command wait opens", {
        realm
            .call_as_workspace(&asked(
                THREE,
                "t1",
                Need::Command,
                None,
            ))
            .await
            .unwrap();
    });
    step!("always trusts", {
        let wait = wait_of(&before, is_tool);
        realm
            .ask(&answer(&wait, Choice::Always, None))
            .await
            .unwrap();
    });
    step!("a resume drops the trust", {
        realm
            .tool(THREE, &lapse(LapseReason::Resumed))
            .await
            .unwrap();
    });
    step!("a resume with nothing open", {
        realm
            .tool(THREE, &lapse(LapseReason::Resumed))
            .await
            .unwrap();
    });
    let revoke = Revoke {
        agent: THREE,
        permission: Permission::Network,
    };
    step!("revoke", chief(&realm, revoke.clone()).await);
    step!("revoke again", chief(&realm, revoke.clone()).await);
    let four = std::cell::Cell::new(AgentNumber(0));
    step!("3 starts a worker", {
        four.set(started_by_three(&realm, "w1").await);
    });
    step!("a chief starts a worker", {
        chief(
            &realm,
            StartWorker {
                hamlet: Some(hamlet.clone()),
                ..start("tidy")
            },
        )
        .await;
    });
    let nickname = SetNickname {
        hire: Named::Agent { agent: THREE },
        name: Some("Spade".to_owned()),
        why: None,
    };
    step!("nickname", {
        realm
            .tool(MAYOR, &tool("s1", nickname.clone()))
            .await
            .unwrap();
    });
    step!("nickname again", {
        realm
            .tool(MAYOR, &tool("s2", nickname.clone()))
            .await
            .unwrap();
    });
    let model = SetModel {
        of: ModelOf::Hire {
            hire: Named::Agent { agent: THREE },
        },
        model: Some("faux::faux-2".to_owned()),
        effort: Some(Effort::High),
        fast: Some(true),
        why: None,
    };
    step!("model", chief(&realm, model.clone()).await);
    step!("model again", chief(&realm, model.clone()).await);
    step!("a hamlet's model", {
        chief(
            &realm,
            SetModel {
                of: ModelOf::Hamlet {
                    hamlet: hamlet.clone(),
                },
                ..model.clone()
            },
        )
        .await;
    });
    step!("allowance", chief(&realm, allowance(Some(500_000))).await);
    step!("allowance again", {
        chief(&realm, allowance(Some(500_000))).await;
    });
    step!("allowance cleared", chief(&realm, allowance(None)).await);
    step!("allowance cleared again", {
        chief(&realm, allowance(None)).await;
    });
    let mut task: RealmTask;
    step!("a chief's line", task = chief_task(&realm, THREE).await);
    step!("a question opens", {
        let ask = AskChief {
            task: task.clone(),
            conversation: ConversationId("c".to_owned()),
            message: None,
            questions: vec![question()],
        };
        realm.tool(THREE, &tool("q1", ask)).await.unwrap();
    });
    step!("the question is answered", {
        let wait = wait_of(&before, |kind| {
            matches!(kind, WaitKind::Question { .. })
        });
        realm
            .ask(&answer(&wait, Choice::Reply, Some("oats")))
            .await
            .unwrap();
    });
    step!("an objection opens", {
        let object = Object {
            task: task.clone(),
            conversation: ConversationId("c".to_owned()),
            reason: "leave it".to_owned(),
        };
        realm.tool(THREE, &tool("o1", object)).await.unwrap();
    });
    step!("go", {
        let wait = wait_of(&before, |kind| {
            matches!(kind, WaitKind::Objection { .. })
        });
        realm.ask(&answer(&wait, Choice::Go, None)).await.unwrap();
    });
    let limits = |limits| SetLimits {
        hamlet: Some(hamlet.clone()),
        budget: Vec::new(),
        limits,
    };
    step!("the limits reserve hire", {
        chief(
            &realm,
            limits(vec![Limit::Reserved {
                act: ReservedAct::Hire,
                reserved: true,
            }]),
        )
        .await;
    });
    step!("a reserved hire opens", {
        realm.tool(MAYOR, &tool("h9", hire(None))).await.unwrap();
    });
    step!("the reserved hire is denied", {
        let wait = wait_of(&before, |kind| {
            matches!(kind, WaitKind::Reserved)
        });
        realm.ask(&answer(&wait, Choice::Deny, None)).await.unwrap();
    });
    step!("a fault opens", {
        realm
            .tool(MAYOR, &failed(standing(&realm, MAYOR)))
            .await
            .unwrap();
    });
    step!("the fault is dismissed", {
        let wait =
            wait_of(&before, |kind| matches!(kind, WaitKind::Fault));
        realm
            .ask(&answer(&wait, Choice::Dismiss, None))
            .await
            .unwrap();
    });
    step!("one refusal at most", {
        chief(&realm, limits(vec![Limit::Refusals { most: 1 }]))
            .await;
    });
    step!("a second line", task = chief_task(&realm, THREE).await);
    step!("a command wait on a task opens", {
        realm
            .call_as_workspace(&asked(
                THREE,
                "u1",
                Need::Command,
                Some(&task),
            ))
            .await
            .unwrap();
    });
    step!("refused", {
        let wait = wait_of(&before, is_tool);
        realm
            .ask(&answer(&wait, Choice::Deny, Some("no")))
            .await
            .unwrap();
    });
    step!("asked past the count", {
        realm
            .call_as_workspace(&asked(
                THREE,
                "u2",
                Need::Command,
                Some(&task),
            ))
            .await
            .unwrap();
    });
    let budget = SetLimits {
        hamlet: Some(hamlet.clone()),
        budget: vec![BudgetLimit::Cost { micros: 1_000_000 }],
        limits: Vec::new(),
    };
    step!("a budget", chief(&realm, budget.clone()).await);
    step!(
        "the same budget again",
        chief(&realm, budget.clone()).await
    );
    step!("worker 4 is done", {
        let done = WorkerDone {
            ended: Ended::Answered {
                text: "Counted.".to_owned(),
            },
        };
        realm.tool(four.get(), &done).await.unwrap();
    });
    step!("a tool wait opens in c", {
        realm
            .call_as_workspace(&asked(
                THREE,
                "l1",
                Need::Command,
                None,
            ))
            .await
            .unwrap();
    });
    step!("a fault opens in c", {
        realm
            .tool(THREE, &failed(standing(&realm, THREE)))
            .await
            .unwrap();
    });
    step!("a stop lapses both", {
        realm
            .tool(THREE, &lapse(LapseReason::Stopped))
            .await
            .unwrap();
    });
    step!("a tool wait opens before the end", {
        realm
            .call_as_workspace(&asked(
                THREE,
                "x1",
                Need::Command,
                None,
            ))
            .await
            .unwrap();
    });
    step!("3 ends", {
        chief(
            &realm,
            End {
                hire: Named::Agent { agent: THREE },
                why: None,
            },
        )
        .await;
    });

    assert!(faults.is_empty(), "{}", faults.join("\n"));
    assert!(agent(&before, THREE).is_some_and(|hire| matches!(
        hire.status,
        Status::Ended { .. }
    )));
    realm.host.shutdown().await.unwrap();
}
