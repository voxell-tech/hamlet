use std::time::{Duration, Instant};

use hamlet_messages::agent::{
    AgentInput, AgentKind, Author, Brief, Input, InputId, Submit,
};
use hamlet_messages::realm::budget::{
    BudgetRead, OpenTask, Payer, ReadBudget, TaskOpened, TaskTurn,
};
use hamlet_messages::realm::choices::{ModelOf, SetModel, SetTools};
use hamlet_messages::realm::documents::ReadCopies;
use hamlet_messages::realm::hamlets::{ReadRecord, Record};
use hamlet_messages::realm::workers::{
    Ended, StartWorker, Started, WorkerDone, WorkerRefusal,
};
use hamlet_messages::realm::world::Status;
use hamlet_messages::realm::{
    AgentRefusal, Asked, Done, End, HamletId, HowEnded, Left, Named,
    Tool, ToolName,
};
use hamlet_messages::workspace::reach::Reach;
use hamlet_messages::workspace::{
    AgentNumber, ConversationId, RealmTask,
};
use hamlet_protocol::Effort;
use hamlet_protocol::client::RequestId;

use super::chiefs::chief;
use super::stand_ins::Callee;
use super::{
    DEADLINE, FOUR, MAYOR, Realm, THREE, done_to, hamlet_with, made,
    made_until, needs, say, seen_submits, standing, to, tool, until,
    worker_shown, world,
};

fn start(brief: &str) -> StartWorker {
    StartWorker {
        brief: brief.to_owned(),
        tools: Vec::new(),
        task: None,
        hamlet: None,
    }
}

fn with_tools(tools: &[&str]) -> StartWorker {
    StartWorker {
        tools: tools
            .iter()
            .map(|&name| ToolName {
                name: name.to_owned(),
            })
            .collect(),
        ..start("count the files")
    }
}

fn answered(text: &str) -> WorkerDone {
    WorkerDone {
        ended: Ended::Answered {
            text: text.to_owned(),
        },
    }
}

/// The worker answers every agent was given.
fn worker_answers(realm: &Realm) -> Vec<(AgentNumber, Submit)> {
    realm
        .seen
        .submits
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, submit)| {
            matches!(submit.input, Input::WorkerAnswer { .. })
        })
        .cloned()
        .collect()
}

/// Gives the realm time to make a call it should not make.
async fn settled() {
    tokio::time::sleep(Duration::from_millis(500)).await;
}

fn latest_reach(realm: &Realm, agent: AgentNumber) -> Option<Reach> {
    realm
        .seen
        .reaches
        .lock()
        .unwrap()
        .iter()
        .filter(|set| set.agent == agent)
        .max_by_key(|set| set.version)
        .map(|set| set.reach.clone())
}

async fn reach_until(
    realm: &Realm,
    agent: AgentNumber,
    what: &str,
    holds: impl Fn(&Reach) -> bool,
) -> Reach {
    until(&format!("agent {}'s reach {what}", agent.0), || {
        latest_reach(realm, agent).as_ref().is_some_and(&holds)
    })
    .await;
    latest_reach(realm, agent).unwrap()
}

async fn ended_until(
    realm: &Realm,
    agent: AgentNumber,
    how: HowEnded,
) {
    let left = Status::Ended {
        left: Left { how, why: None },
    };
    world::seen_until(
        realm,
        &format!("agent {} ended {left:?}", agent.0),
        |seen| {
            seen.agents.iter().any(|shown| {
                shown.agent == agent && shown.status == left
            })
        },
    )
    .await;
}

/// The conversation hire#3 starts its workers in.
fn talk() -> ConversationId {
    ConversationId("talk".to_owned())
}

async fn started_by_three(realm: &Realm, call: &str) -> AgentNumber {
    let call = Tool {
        conversation: Some(talk()),
        ..tool(call, start("count the files"))
    };
    let answer = realm.tool(THREE, &call).await.unwrap();
    let Started::Started { worker } = answer else {
        panic!("hire#3 starts a worker: {answer:?}");
    };
    worker.agent
}

#[tokio::test(flavor = "multi_thread")]
async fn a_worker_answers_its_starter_once_in_its_conversation_and_ends()
 {
    let (realm, _) = hamlet_with(1).await;

    let worker = started_by_three(&realm, "w").await;

    assert_eq!(worker, FOUR, "a worker takes the next agent number");
    let task = standing(&realm, THREE);
    let input = made_until(&realm, FOUR).await;
    let copies =
        realm.tool(FOUR, &ReadCopies::default()).await.unwrap();
    assert_eq!(
        input,
        AgentInput {
            kind: AgentKind::Worker,
            task: task.clone(),
            place: input.place.clone(),
            brief: Some(Brief {
                instructions: None,
                text: "count the files".to_owned(),
                tools: Vec::new(),
                model: None,
                dream: false,
            }),
            copies,
        }
    );
    let reach = reach_until(&realm, FOUR, "is a worker's", |reach| {
        matches!(reach, Reach::Worker { .. })
    })
    .await;
    assert!(
        matches!(
            reach,
            Reach::Worker {
                starter: Some(THREE),
                ..
            }
        ),
        "{reach:?}"
    );

    realm
        .tool(FOUR, &answered("Counted: 3 files."))
        .await
        .unwrap();
    seen_submits(&realm, THREE, 1).await;
    realm
        .tool(FOUR, &answered("Counted: 4 files."))
        .await
        .unwrap();
    settled().await;

    assert_eq!(
        worker_answers(&realm),
        [(
            THREE,
            Submit {
                id: InputId("worker/4".to_owned()),
                task: Some(task),
                input: Input::WorkerAnswer {
                    from: Author {
                        agent: FOUR,
                        name: Some(worker_shown(FOUR).name),
                    },
                    text: "Counted: 3 files.".to_owned(),
                    conversation: Some(talk()),
                },
            }
        )]
    );
    ended_until(&realm, FOUR, HowEnded::Answered).await;
    reach_until(&realm, FOUR, "ended", |reach| {
        *reach == Reach::Ended
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_worker_runs_on_its_starters_model() {
    let (realm, _) = hamlet_with(1).await;
    let chosen = chief(
        &realm,
        SetModel {
            of: ModelOf::Hire { hire: to(THREE) },
            model: Some("faux::faux-2".to_owned()),
            effort: Some(Effort::High),
            fast: None,
            why: None,
        },
    )
    .await;

    started_by_three(&realm, "w").await;
    let input = made_until(&realm, FOUR).await;

    assert_eq!(chosen, Done::Done);
    assert_eq!(
        input.brief.and_then(|brief| brief.model).as_deref(),
        Some("faux::faux-2")
    );
    assert_eq!(
        input
            .copies
            .model
            .map(|model| (model.value.model, model.value.effort)),
        Some(("faux::faux-2".to_owned(), Some(Effort::High)))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_worker_keeps_its_own_fast_mode() {
    let (realm, _) = hamlet_with(1).await;
    let chosen = chief(
        &realm,
        SetModel {
            of: ModelOf::Hire { hire: to(THREE) },
            model: Some("faux::faux-2".to_owned()),
            effort: None,
            fast: Some(true),
            why: None,
        },
    )
    .await;

    started_by_three(&realm, "w").await;
    let input = made_until(&realm, FOUR).await;

    assert_eq!(chosen, Done::Done);
    assert_eq!(
        input
            .copies
            .model
            .map(|model| (model.value.model, model.value.fast)),
        Some(("faux::faux-2".to_owned(), false))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_worker_tells_its_starter_why() {
    let (realm, _) = hamlet_with(1).await;
    started_by_three(&realm, "w").await;
    made_until(&realm, FOUR).await;

    let failed = WorkerDone {
        ended: Ended::Failed {
            why: "the model is out of reach".to_owned(),
        },
    };
    realm.tool(FOUR, &failed).await.unwrap();
    seen_submits(&realm, THREE, 1).await;

    let answers = worker_answers(&realm);
    assert!(
        matches!(
            &answers[..],
            [(THREE, Submit { input: Input::WorkerAnswer { text, .. }, .. })]
                if text == "It could not answer: the model is out of reach"
        ),
        "{answers:?}"
    );
    ended_until(&realm, FOUR, HowEnded::Failed).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_worker_starts_no_worker() {
    let (realm, _) = hamlet_with(1).await;
    started_by_three(&realm, "w").await;
    made_until(&realm, FOUR).await;

    let answer = realm
        .tool(FOUR, &tool("again", start("count again")))
        .await
        .unwrap();

    assert_eq!(
        answer,
        Started::Refused {
            why: AgentRefusal::Act(WorkerRefusal::AWorker),
        }
    );
    settled().await;
    assert!(made(&realm, AgentNumber(5)).is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn ending_a_starter_stops_its_workers() {
    let (realm, _) = hamlet_with(1).await;
    started_by_three(&realm, "w").await;
    made_until(&realm, FOUR).await;

    let ended = realm
        .tool(
            MAYOR,
            &tool(
                "end",
                End {
                    hire: to(THREE),
                    why: None,
                },
            ),
        )
        .await
        .unwrap();

    assert_eq!(ended, done_to(THREE));
    ended_until(&realm, FOUR, HowEnded::Stopped).await;
    reach_until(&realm, FOUR, "ended", |reach| {
        *reach == Reach::Ended
    })
    .await;
    realm.tool(FOUR, &answered("late")).await.unwrap();
    settled().await;
    assert_eq!(worker_answers(&realm), []);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_worker_step_finishes_at_the_realms_start() {
    let (realm, _) = hamlet_with(1).await;
    realm.seen.fail_once(Callee::SetReach, FOUR);
    started_by_three(&realm, "w").await;
    until("the workspace failed the worker's reach once", || {
        realm.seen.has_failed(Callee::SetReach, FOUR)
    })
    .await;
    settled().await;
    assert!(made(&realm, FOUR).is_none(), "create follows the reach");

    let realm = realm.restart().await;
    made_until(&realm, FOUR).await;
    reach_until(&realm, FOUR, "is a worker's", |reach| {
        matches!(reach, Reach::Worker { .. })
    })
    .await;

    realm
        .write(
            "UPDATE step SET done = 'reach', closed = 0 WHERE kind \
             = 'worker' AND n = 4",
        )
        .await;
    let begun = Instant::now();
    while !step_closed(&realm).await {
        assert!(begun.elapsed() < DEADLINE, "never: the step closes");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let made = realm.seen.made.lock().unwrap().clone();
    assert_eq!(
        made.iter().filter(|(made, _)| *made == FOUR).count(),
        1
    );
}

async fn step_closed(realm: &Realm) -> bool {
    realm
        .stopped_rows(|db| {
            db.query_row(
                "SELECT closed FROM step WHERE kind = 'worker' AND \
                 n = 4",
                [],
                |row| row.get(0),
            )
        })
        .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chiefs_worker_answers_in_its_own_conversation() {
    let (realm, hamlet) = hamlet_with(0).await;

    let answer = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: StartWorker {
                hamlet: Some(hamlet),
                ..start("count the files")
            },
        })
        .await
        .unwrap();

    assert_eq!(
        answer,
        Started::Started {
            worker: worker_shown(THREE)
        }
    );
    let input = made_until(&realm, THREE).await;
    assert_eq!(input.kind, AgentKind::Worker);
    assert_ne!(
        input.task,
        standing(&realm, MAYOR),
        "a task of its own"
    );
    let reach =
        reach_until(&realm, THREE, "is a worker's", |reach| {
            matches!(reach, Reach::Worker { .. })
        })
        .await;
    assert!(
        matches!(reach, Reach::Worker { starter: None, .. }),
        "{reach:?}"
    );

    realm
        .tool(THREE, &answered("Counted: 3 files."))
        .await
        .unwrap();
    ended_until(&realm, THREE, HowEnded::Answered).await;
    settled().await;
    assert_eq!(worker_answers(&realm), []);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_tool_is_refused() {
    let (realm, _) = hamlet_with(1).await;
    let off = realm
        .tool(
            MAYOR,
            &tool(
                "off",
                SetTools {
                    hire: to(THREE),
                    off: vec![ToolName {
                        name: "bash".to_owned(),
                    }],
                    on: Vec::new(),
                    why: None,
                },
            ),
        )
        .await
        .unwrap();
    assert_eq!(off, done_to(THREE));

    for (n, name) in ["telescope", "bash"].into_iter().enumerate() {
        let answer = realm
            .tool(THREE, &tool(&format!("w{n}"), with_tools(&[name])))
            .await
            .unwrap();
        let Started::Refused { why } = answer else {
            panic!("{name} was given: {answer:?}");
        };
        assert_eq!(
            why.to_string(),
            format!("`hire#3 (Hire 1)` has no such tool: {name}")
        );
    }
    let answer = realm
        .tool(THREE, &tool("tell", with_tools(&["tell"])))
        .await
        .unwrap();
    let Started::Refused { why } = answer else {
        panic!("tell was given: {answer:?}");
    };
    assert_eq!(
        why.to_string(),
        "a worker's tools are a part of its starter's"
    );
    let answer = realm
        .tool(THREE, &tool("read", with_tools(&["read", "edit"])))
        .await
        .unwrap();
    assert_eq!(
        answer,
        Started::Started {
            worker: worker_shown(FOUR)
        }
    );
    let tools = made_until(&realm, FOUR).await.brief.unwrap().tools;
    let names = tools.iter().map(|tool| tool.name.as_str());
    assert_eq!(names.collect::<Vec<_>>(), ["read", "edit"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_brief_and_a_hamlet_are_checked() {
    let (realm, hamlet) = hamlet_with(1).await;
    let refused = |why| Started::Refused {
        why: AgentRefusal::Act(why),
    };

    let blank = realm
        .tool(THREE, &tool("blank", start("  ")))
        .await
        .unwrap();
    assert_eq!(blank, refused(WorkerRefusal::EmptyBrief));
    let named = StartWorker {
        hamlet: Some(hamlet),
        ..start("count the files")
    };
    let elsewhere =
        realm.tool(THREE, &tool("named", named)).await.unwrap();
    assert_eq!(elsewhere, refused(WorkerRefusal::HamletFromAnAgent));
    let missing = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: StartWorker {
                hamlet: Some(HamletId("nowhere".to_owned())),
                ..start("count the files")
            },
        })
        .await
        .unwrap();
    assert_eq!(
        missing,
        Started::Refused {
            why: WorkerRefusal::NoSuchHamlet {
                id: HamletId("nowhere".to_owned()),
            },
        }
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_worker_runs_and_answers_on_the_task_it_is_given() {
    let (realm, _) = hamlet_with(1).await;
    realm
        .ask(&say(&RequestId::new(), THREE, "count the files"))
        .await
        .unwrap();
    let line = seen_submits(&realm, THREE, 1).await.remove(0).task;
    let line = line.expect("a chief's line has a task");
    assert_ne!(line, standing(&realm, THREE));

    for (call, task) in [
        ("on the line", Some(line.clone())),
        ("unknown", Some(RealmTask("999".to_owned()))),
    ] {
        let answer = realm
            .tool(
                THREE,
                &tool(
                    call,
                    StartWorker {
                        task,
                        ..start("count the files")
                    },
                ),
            )
            .await
            .unwrap();
        assert!(
            matches!(answer, Started::Started { .. }),
            "{answer:?}"
        );
    }

    assert_eq!(made_until(&realm, FOUR).await.task, line);
    let five = AgentNumber(5);
    assert_eq!(
        made_until(&realm, five).await.task,
        standing(&realm, THREE),
        "a task the realm does not hold falls back to the standing \
         one"
    );
    realm.tool(FOUR, &answered("3 files")).await.unwrap();
    until("hire#3 hears worker#4", || {
        !worker_answers(&realm).is_empty()
    })
    .await;
    assert_eq!(worker_answers(&realm)[0].1.task, Some(line));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_blank_answer_says_none_was_given() {
    let (realm, _) = hamlet_with(1).await;
    started_by_three(&realm, "w").await;
    made_until(&realm, FOUR).await;

    realm.tool(FOUR, &answered("  ")).await.unwrap();
    until("hire#3 hears worker#4", || {
        !worker_answers(&realm).is_empty()
    })
    .await;

    let answers = worker_answers(&realm);
    assert!(
        matches!(
            &answers[0].1.input,
            Input::WorkerAnswer { text, .. } if text == "It gave no answer."
        ),
        "{answers:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_workers_end_lapses_its_waits() {
    let (realm, _) = hamlet_with(1).await;
    started_by_three(&realm, "w").await;
    made_until(&realm, FOUR).await;
    realm.call_as_workspace(&needs(FOUR, "c1")).await.unwrap();
    world::seen_until(&realm, "worker#4 waits for a chief", |seen| {
        seen.waits.iter().any(|wait| wait.agent == FOUR)
    })
    .await;

    realm.tool(FOUR, &answered("done")).await.unwrap();

    world::seen_until(&realm, "worker#4's wait lapsed", |seen| {
        seen.waits.iter().all(|wait| wait.agent != FOUR)
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chief_stops_a_worker_and_its_starter_hears_no_answer() {
    let (realm, hamlet) = hamlet_with(1).await;
    started_by_three(&realm, "w").await;
    made_until(&realm, FOUR).await;

    let ended = chief(
        &realm,
        End {
            hire: Named::Worker { worker: FOUR },
            why: None,
        },
    )
    .await;
    ended_until(&realm, FOUR, HowEnded::Stopped).await;
    seen_submits(&realm, THREE, 1).await;
    realm.tool(FOUR, &answered("late")).await.unwrap();
    settled().await;
    let record = chief(
        &realm,
        ReadRecord {
            hamlet: Some(hamlet),
        },
    )
    .await;

    assert_eq!(ended, Done::Done);
    let told = worker_answers(&realm)
        .into_iter()
        .map(|(to, submit)| (to, submit.input))
        .collect::<Vec<_>>();
    assert_eq!(
        told,
        [(
            THREE,
            Input::WorkerAnswer {
                from: Author {
                    agent: FOUR,
                    name: Some(worker_shown(FOUR).name),
                },
                text: "It gave no answer.".to_owned(),
                conversation: Some(talk()),
            }
        )]
    );
    let Record::Read { lines } = record else {
        panic!("the hamlet's record: {record:?}");
    };
    assert!(
        lines.iter().all(|line| !line.text.contains("worker#4")),
        "{lines:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chief_sets_no_tools_of_a_worker() {
    let (realm, _) = hamlet_with(1).await;
    started_by_three(&realm, "w").await;

    let set = chief(
        &realm,
        SetTools {
            hire: Named::Worker { worker: FOUR },
            off: vec![ToolName {
                name: "bash".to_owned(),
            }],
            on: Vec::new(),
            why: None,
        },
    )
    .await;

    let Done::Refused { why } = set else {
        panic!("a worker's tools were set: {set:?}");
    };
    assert_eq!(why.to_string(), "`worker#4` is not a hire");
}

/// The payer of `task`, as `agent` reads its budget in call `call`.
async fn payer_read(
    realm: &Realm,
    agent: AgentNumber,
    call: &str,
    task: Option<RealmTask>,
) -> Payer {
    let read = realm
        .tool(agent, &tool(call, ReadBudget { task }))
        .await
        .unwrap();
    let BudgetRead::Budget { spend, .. } = read else {
        panic!("agent {} reads no budget: {read:?}", agent.0);
    };
    spend.payer
}

#[tokio::test(flavor = "multi_thread")]
async fn a_worker_pays_where_its_starter_works() {
    let (realm, hamlet) = hamlet_with(1).await;
    started_by_three(&realm, "w").await;
    made_until(&realm, FOUR).await;

    let own = payer_read(&realm, FOUR, "r1", None).await;
    let opened = realm
        .tool(
            FOUR,
            &OpenTask {
                request: RequestId::new(),
                turn: TaskTurn::Reflection,
            },
        )
        .await
        .unwrap();

    let starters = Payer::Hamlet { hamlet };
    assert_eq!(own, starters);
    let TaskOpened::Opened { task } = opened else {
        panic!("the worker opens no task: {opened:?}");
    };
    assert_eq!(
        payer_read(&realm, FOUR, "r2", Some(task)).await,
        starters
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chiefs_line_to_a_worker_is_paid_by_its_hamlet() {
    let (realm, hamlet) = hamlet_with(1).await;
    started_by_three(&realm, "w").await;
    made_until(&realm, FOUR).await;

    realm
        .ask(&say(&RequestId::new(), FOUR, "hello"))
        .await
        .unwrap();
    let line = seen_submits(&realm, FOUR, 1).await.remove(0);

    assert_eq!(
        payer_read(&realm, FOUR, "r1", line.task).await,
        Payer::Hamlet { hamlet }
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_worker_ended_before_its_step_is_never_made() {
    let (realm, _) = hamlet_with(1).await;
    realm.seen.fail_once(Callee::SetReach, FOUR);
    started_by_three(&realm, "w").await;
    until("the workspace failed the worker's reach once", || {
        realm.seen.has_failed(Callee::SetReach, FOUR)
    })
    .await;
    let ended = realm
        .tool(
            MAYOR,
            &tool(
                "end",
                End {
                    hire: to(THREE),
                    why: None,
                },
            ),
        )
        .await
        .unwrap();
    assert_eq!(ended, done_to(THREE));

    let realm = realm.restart().await;

    let begun = Instant::now();
    while !step_closed(&realm).await {
        assert!(begun.elapsed() < DEADLINE, "never: the step closes");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(made(&realm, FOUR).is_none());
}
