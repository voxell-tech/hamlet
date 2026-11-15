use std::time::{Duration, Instant};

use hamlet_messages::agent::{
    AgentInput, AgentKind, Brief, Input, InputId, Submit,
};
use hamlet_messages::realm::workers::{
    Ended, StartWorker, Started, WorkerDone, WorkerRefusal,
};
use hamlet_messages::realm::world::Status;
use hamlet_messages::realm::{
    AgentRefusal, Asked, Done, End, HamletId, HowEnded, Left,
    SetTools, ToolName,
};
use hamlet_messages::workspace::AgentNumber;
use hamlet_messages::workspace::reach::Reach;
use hamlet_protocol::client::RequestId;

use super::{
    Callee, DEADLINE, FOUR, MAYOR, Realm, THREE, hamlet_with,
    seen_submits, standing, to, tool, until, world,
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

fn made(realm: &Realm, agent: AgentNumber) -> Vec<AgentInput> {
    realm
        .seen
        .made
        .lock()
        .unwrap()
        .iter()
        .filter(|(made, _)| *made == agent)
        .map(|(_, input)| input.clone())
        .collect()
}

async fn made_until(realm: &Realm, agent: AgentNumber) -> AgentInput {
    until(&format!("agent {} is made", agent.0), || {
        !made(realm, agent).is_empty()
    })
    .await;
    made(realm, agent).remove(0)
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

async fn started_by_three(realm: &Realm, call: &str) -> AgentNumber {
    let answer = realm
        .tool(THREE, &tool(call, start("count the files")))
        .await
        .unwrap();
    let Started::Started { worker } = answer else {
        panic!("hire#3 starts a worker: {answer:?}");
    };
    worker
}

#[tokio::test(flavor = "multi_thread")]
async fn a_worker_answers_its_starter_once_and_ends() {
    let (realm, _) = hamlet_with(1).await;

    let worker = started_by_three(&realm, "w").await;

    assert_eq!(worker, FOUR, "a worker takes the next agent number");
    let task = standing(&realm, THREE);
    let input = made_until(&realm, FOUR).await;
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
            }),
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

    let done = answered("Counted: 3 files.");
    realm.tool(FOUR, &done).await.unwrap();
    seen_submits(&realm, THREE, 1).await;
    realm.tool(FOUR, &done).await.unwrap();
    settled().await;

    assert_eq!(
        worker_answers(&realm),
        [(
            THREE,
            Submit {
                id: InputId("worker/4".to_owned()),
                task: Some(task),
                input: Input::WorkerAnswer {
                    worker: FOUR,
                    text: "Counted: 3 files.".to_owned(),
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
    assert!(made(&realm, AgentNumber(5)).is_empty());
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

    assert_eq!(ended, Done::Done);
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
    assert!(
        made(&realm, FOUR).is_empty(),
        "create follows the reach"
    );

    let realm = realm.restart().await;
    made_until(&realm, FOUR).await;
    reach_until(&realm, FOUR, "is a worker's", |reach| {
        matches!(reach, Reach::Worker { .. })
    })
    .await;

    // A kill after the create and before its step was kept: the
    // create is made again and finds the agent made.
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
    assert_eq!(made(&realm, FOUR).len(), 1);
}

async fn step_closed(realm: &Realm) -> bool {
    realm
        .rows(|db| {
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

    assert_eq!(answer, Started::Started { worker: THREE });
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
    assert_eq!(off, Done::Done);

    for (n, name) in
        ["telescope", "tell", "bash"].into_iter().enumerate()
    {
        let answer = realm
            .tool(THREE, &tool(&format!("w{n}"), with_tools(&[name])))
            .await
            .unwrap();
        assert_eq!(
            answer,
            Started::Refused {
                why: AgentRefusal::Act(WorkerRefusal::NoSuchTool {
                    tool: name.to_owned(),
                }),
            },
        );
    }
    let answer = realm
        .tool(THREE, &tool("read", with_tools(&["read", "edit"])))
        .await
        .unwrap();
    assert_eq!(answer, Started::Started { worker: FOUR });
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
            why: WorkerRefusal::NoSuchHamlet,
        }
    );
}
