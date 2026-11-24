use hamlet_actor::Action;
use hamlet_messages::agent::{Author, Input};
use hamlet_messages::realm::choices::{
    ChoiceRefusal, ModelOf, SetModel, SetNickname,
};
use hamlet_messages::realm::workers::{
    Ended, SetAllowance, StartWorker, Started, WorkerDone,
};
use hamlet_messages::realm::{
    AgentRefusal, AgreeRoad, Ask, Done, DoneTo, End, EndRefusal,
    LayRoad, LeadRefusal, LetterRefusal, Named, RequestRoad, Seat,
    Sent, SetTools, TakeUpRoad, Tool, ToolName,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::{AgentNumber, ConversationId};
use serde::Serialize;

use super::chiefs::chief;
use super::{
    FOUR, MAYOR, Realm, THREE, hamlet_with, hire, named_as,
    seen_submits, sent, tell, to, tool, words,
};

fn nickname(hire: Named, name: &str) -> SetNickname {
    SetNickname {
        hire,
        name: Some(name.to_owned()),
        why: None,
    }
}

fn start() -> StartWorker {
    StartWorker {
        brief: "count the files".to_owned(),
        tools: Vec::new(),
        task: None,
        hamlet: None,
    }
}

fn a_worker() -> Named {
    Named::Worker { worker: FOUR }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_shows_its_profiles_name_then_its_nickname() {
    let (realm, _) = hamlet_with(1).await;

    let before = realm
        .tool(MAYOR, &tool("t1", tell(to(THREE), "hi")))
        .await
        .unwrap();
    let renamed = realm
        .tool(MAYOR, &tool("n1", nickname(to(THREE), "Spade")))
        .await
        .unwrap();
    let after = realm
        .tool(MAYOR, &tool("t2", tell(to(THREE), "hi")))
        .await
        .unwrap();
    let to_mayor = realm
        .tool(THREE, &tool("t3", tell(Named::Mayor, "hi")))
        .await
        .unwrap();

    assert_eq!(before, sent(THREE));
    assert_eq!(
        before,
        Sent::Sent {
            to: named_as(3, "hire#3 (Hire 1)")
        }
    );
    assert_eq!(
        renamed,
        DoneTo::Done {
            agent: named_as(3, "hire#3 (Hire 1)")
        }
    );
    assert_eq!(
        after,
        Sent::Sent {
            to: named_as(3, "hire#3 (Spade)")
        }
    );
    assert_eq!(
        to_mayor,
        Sent::Sent {
            to: named_as(2, "hire#2 (Mayor)")
        }
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_worker_shows_its_address_then_its_nickname() {
    let (realm, _) = hamlet_with(1).await;
    let started =
        realm.tool(THREE, &tool("w1", start())).await.unwrap();
    let renamed =
        chief(&realm, nickname(a_worker(), "Counter")).await;
    let by_a_lead = realm
        .tool(MAYOR, &tool("n1", nickname(a_worker(), "Tally")))
        .await
        .unwrap();
    realm
        .tool(
            FOUR,
            &WorkerDone {
                ended: Ended::Answered {
                    text: "Three.".to_owned(),
                },
            },
        )
        .await
        .unwrap();
    let told = seen_submits(&realm, THREE, 1).await;

    assert_eq!(
        started,
        Started::Started {
            worker: named_as(4, "worker#4")
        }
    );
    assert_eq!(renamed, Done::Done);
    assert_eq!(
        by_a_lead,
        DoneTo::Refused {
            why: LeadRefusal::Act(ChoiceRefusal::NotAHire {
                name: "worker#4 (Counter)".to_owned()
            })
        }
    );
    let Input::WorkerAnswer { from, .. } = &told[0].input else {
        panic!("the worker's answer: {told:?}");
    };
    assert_eq!(
        *from,
        Author {
            agent: FOUR,
            name: Some("worker#4 (Counter)".to_owned()),
        }
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_workers_address_names_only_a_worker_and_a_hires_only_a_hire()
 {
    let (realm, _) = hamlet_with(1).await;
    realm.tool(THREE, &tool("w1", start())).await.unwrap();
    let end = |hire| End { hire, why: None };

    let hire_as_worker = chief(
        &realm,
        end(Named::Worker {
            worker: AgentNumber(3),
        }),
    )
    .await;
    let worker_as_hire = realm
        .tool(MAYOR, &tool("t1", tell(to(FOUR), "hi")))
        .await
        .unwrap();

    assert_eq!(
        hire_as_worker,
        Done::Refused {
            why: EndRefusal::NoSuchAgent {
                address: "worker#3".to_owned()
            }
        }
    );
    assert_eq!(
        worker_as_hire,
        Sent::Refused {
            why: AgentRefusal::Act(LetterRefusal::NoSuchAgent {
                address: "hire#4".to_owned()
            })
        }
    );
    realm.host.shutdown().await.unwrap();
}

async fn refused<T>(
    realm: &Realm,
    by: AgentNumber,
    call: &str,
    act: T,
) -> String
where
    Tool<T>: Action<Output: Serialize>,
{
    words(realm.tool(by, &tool(call, act)).await.unwrap())
}

fn set_model(of: ModelOf) -> SetModel {
    SetModel {
        of,
        model: Some("faux::faux-1".to_owned()),
        effort: None,
        fast: None,
        why: None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agent_naming_an_ended_hire_reads_that_no_agent_has_its_address()
 {
    let (realm, _) = hamlet_with(2).await;
    let ended = End {
        hire: to(THREE),
        why: None,
    };
    realm.tool(MAYOR, &tool("e1", ended.clone())).await.unwrap();
    let road = |a, b| (to(a), to(b));
    let (a, b) = road(THREE, FOUR);

    let read = [
        refused(&realm, MAYOR, "t1", tell(to(THREE), "hi")).await,
        refused(
            &realm,
            MAYOR,
            "q1",
            Ask {
                to: to(THREE),
                text: "Which file?".to_owned(),
                conversation: ConversationId("mayor".to_owned()),
                paths: Vec::new(),
            },
        )
        .await,
        refused(&realm, MAYOR, "e2", ended).await,
        refused(
            &realm,
            MAYOR,
            "s1",
            SetTools {
                hire: to(THREE),
                off: Vec::new(),
                on: Vec::new(),
                why: None,
            },
        )
        .await,
        refused(&realm, MAYOR, "n1", nickname(to(THREE), "Gone"))
            .await,
        refused(
            &realm,
            MAYOR,
            "m1",
            set_model(ModelOf::Hire { hire: to(THREE) }),
        )
        .await,
        refused(
            &realm,
            MAYOR,
            "a1",
            SetAllowance {
                hire: to(THREE),
                micros: None,
                why: None,
            },
        )
        .await,
        refused(
            &realm,
            MAYOR,
            "l1",
            LayRoad {
                a: a.clone(),
                b: b.clone(),
                why: None,
            },
        )
        .await,
        refused(&realm, MAYOR, "u1", TakeUpRoad { a, b, why: None })
            .await,
        refused(
            &realm,
            MAYOR,
            "g1",
            AgreeRoad {
                mine: to(FOUR),
                theirs: to(THREE),
                why: None,
            },
        )
        .await,
        refused(&realm, MAYOR, "x1", hire(Some(to(THREE)))).await,
        refused(
            &realm,
            FOUR,
            "r1",
            RequestRoad {
                to: to(THREE),
                why: None,
            },
        )
        .await,
        refused(
            &realm,
            Hand::NUMBER,
            "p1",
            Seat {
                hire: to(THREE),
                why: None,
            },
        )
        .await,
    ];

    assert_eq!(
        read,
        ["no agent has the address `hire#3`"; 13].map(String::from)
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agent_naming_an_ended_worker_reads_that_no_agent_has_its_address()
 {
    let (realm, _) = hamlet_with(1).await;
    realm.tool(THREE, &tool("w1", start())).await.unwrap();
    realm
        .tool(
            FOUR,
            &WorkerDone {
                ended: Ended::Answered {
                    text: "Three.".to_owned(),
                },
            },
        )
        .await
        .unwrap();

    let read = [
        refused(&realm, THREE, "t1", tell(a_worker(), "hi")).await,
        refused(&realm, MAYOR, "n1", nickname(a_worker(), "Tally"))
            .await,
        refused(
            &realm,
            MAYOR,
            "m1",
            set_model(ModelOf::Hire { hire: a_worker() }),
        )
        .await,
        refused(
            &realm,
            THREE,
            "e1",
            End {
                hire: a_worker(),
                why: None,
            },
        )
        .await,
    ];

    assert_eq!(
        read,
        ["no agent has the address `worker#4`"; 4].map(String::from)
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chief_naming_an_ended_hire_reads_that_no_agent_has_its_address()
 {
    let (realm, _) = hamlet_with(2).await;
    let end = || End {
        hire: to(THREE),
        why: None,
    };
    assert_eq!(chief(&realm, end()).await, Done::Done);
    let road = || (to(THREE), to(FOUR));
    let (a, b) = road();
    let (c, d) = road();

    let read = [
        words(chief(&realm, nickname(to(THREE), "Gone")).await),
        words(
            chief(
                &realm,
                set_model(ModelOf::Hire { hire: to(THREE) }),
            )
            .await,
        ),
        words(chief(&realm, end()).await),
        words(
            chief(
                &realm,
                SetAllowance {
                    hire: to(THREE),
                    micros: None,
                    why: None,
                },
            )
            .await,
        ),
        words(
            chief(
                &realm,
                SetTools {
                    hire: to(THREE),
                    off: vec![ToolName {
                        name: "bash".to_owned(),
                    }],
                    on: Vec::new(),
                    why: None,
                },
            )
            .await,
        ),
        words(
            chief(
                &realm,
                Seat {
                    hire: to(THREE),
                    why: None,
                },
            )
            .await,
        ),
        words(chief(&realm, LayRoad { a, b, why: None }).await),
        words(
            chief(
                &realm,
                TakeUpRoad {
                    a: c,
                    b: d,
                    why: None,
                },
            )
            .await,
        ),
        words(chief(&realm, hire(Some(to(THREE)))).await),
    ];

    assert_eq!(
        read,
        ["no agent has the address `hire#3`"; 9].map(String::from)
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chief_naming_an_ended_worker_reads_that_no_agent_has_its_address()
 {
    let (realm, _) = hamlet_with(1).await;
    realm.tool(THREE, &tool("w1", start())).await.unwrap();
    realm
        .tool(
            FOUR,
            &WorkerDone {
                ended: Ended::Answered {
                    text: "Three.".to_owned(),
                },
            },
        )
        .await
        .unwrap();

    let read = [
        words(chief(&realm, nickname(a_worker(), "Tally")).await),
        words(
            chief(
                &realm,
                set_model(ModelOf::Hire { hire: a_worker() }),
            )
            .await,
        ),
        words(
            chief(
                &realm,
                End {
                    hire: a_worker(),
                    why: None,
                },
            )
            .await,
        ),
    ];

    assert_eq!(
        read,
        ["no agent has the address `worker#4`"; 3].map(String::from)
    );
    realm.host.shutdown().await.unwrap();
}
