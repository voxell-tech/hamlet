use hamlet_messages::agent::{Author, Input};
use hamlet_messages::realm::choices::{ChoiceRefusal, SetNickname};
use hamlet_messages::realm::workers::{
    Ended, StartWorker, Started, WorkerDone,
};
use hamlet_messages::realm::{
    AgentRefusal, Done, DoneTo, End, EndRefusal, LeadRefusal,
    LetterRefusal, Named, Sent,
};
use hamlet_messages::workspace::AgentNumber;

use super::chiefs::chief;
use super::{
    FOUR, MAYOR, THREE, hamlet_with, named_as, seen_submits, sent,
    tell, to, tool,
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
