//! A lead's end or merge of a hire on a chief's task, refused (H7).

use hamlet_messages::agent::OnTask;
use hamlet_messages::realm::hires::{Merge, MergeRefusal, MergedTo};
use hamlet_messages::realm::{
    Done, DoneTo, End, EndRefusal, HamletId, LeadRefusal, Limit,
    SetLimits,
};
use hamlet_messages::workspace::{AgentNumber, RealmTask};

use super::asking::chief_task;
use super::merges::{agent, chief};
use super::{
    FOUR, MAYOR, Realm, THREE, hamlet_with, paid, shown, tell, to,
    tool, words,
};

fn end(hire: AgentNumber) -> End {
    End {
        hire: to(hire),
        why: None,
    }
}

fn merge(absorbed: AgentNumber, into: AgentNumber) -> Merge {
    Merge {
        absorbed: to(absorbed),
        into: to(into),
        why: None,
    }
}

fn asked(realm: &Realm) -> Vec<(AgentNumber, OnTask)> {
    realm.seen.asked_on_task.lock().unwrap().clone()
}

/// A hamlet whose mayor leads hires 3 and 4: 3 was given the chief's
/// line `.2`, and 4 a letter the mayor sent on the chief's line `.3`.
async fn on_chiefs_tasks() -> (Realm, HamletId, RealmTask, RealmTask)
{
    let (realm, hamlet) = hamlet_with(2).await;
    let three = chief_task(&realm, THREE).await;
    let mayors = chief_task(&realm, MAYOR).await;
    realm
        .tool(MAYOR, &paid("t1", &mayors, tell(to(FOUR), "Dig.")))
        .await
        .unwrap();
    (realm, hamlet, three, mayors)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_leads_end_or_merge_of_a_hire_on_a_chiefs_task_is_refused()
{
    let (realm, hamlet, three, mayors) = on_chiefs_tasks().await;
    chief(
        &realm,
        SetLimits {
            hamlet: Some(hamlet),
            budget: Vec::new(),
            limits: vec![Limit::ProtectedMinutes { minutes: 5 }],
        },
    )
    .await;
    realm.seen.on_task.lock().unwrap().extend([THREE, FOUR]);

    let ended =
        realm.tool(MAYOR, &tool("e1", end(THREE))).await.unwrap();
    realm.seen.on_task.lock().unwrap().remove(&THREE);
    let merged = realm
        .tool(MAYOR, &tool("m1", merge(FOUR, THREE)))
        .await
        .unwrap();

    assert_eq!(
        ended,
        DoneTo::Refused {
            why: LeadRefusal::Act(EndRefusal::OnChiefsTask {
                name: shown(THREE)
            })
        }
    );
    assert_eq!(
        words(&ended),
        "`hire#3 (Hire 1)` is on a chief's task; a lead may end or \
         merge it once it has answered and its protected minutes \
         have passed"
    );
    assert_eq!(
        merged,
        MergedTo::Refused {
            why: LeadRefusal::Act(MergeRefusal::OnChiefsTask {
                name: shown(FOUR)
            })
        }
    );
    let within = |tasks: &RealmTask| OnTask {
        tasks: vec![tasks.clone()],
        within_ms: 300_000,
    };
    assert_eq!(
        asked(&realm),
        [
            (THREE, within(&three)),
            (THREE, within(&three)),
            (FOUR, within(&mayors)),
        ]
    );
    for hire in [THREE, FOUR] {
        assert_eq!(agent(&realm, hire).await.0, "live");
    }
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chiefs_end_or_merge_asks_nothing() {
    let (realm, _, _, _) = on_chiefs_tasks().await;
    realm.seen.on_task.lock().unwrap().extend([THREE, FOUR]);

    let merged = chief(&realm, merge(FOUR, THREE)).await;
    let ended = chief(&realm, end(THREE)).await;

    assert_eq!((merged, ended), (Done::Done, Done::Done));
    assert_eq!(asked(&realm), []);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_leads_end_goes_through_once_nothing_is_open_and_its_repeat_asks_nothing()
 {
    let (realm, _, three, _) = on_chiefs_tasks().await;

    let ended =
        realm.tool(MAYOR, &tool("e1", end(THREE))).await.unwrap();
    let again =
        realm.tool(MAYOR, &tool("e1", end(THREE))).await.unwrap();

    assert_eq!(ended, again);
    assert!(matches!(ended, DoneTo::Done { .. }), "{ended:?}");
    assert_eq!(
        asked(&realm),
        [(
            THREE,
            OnTask {
                tasks: vec![three],
                within_ms: 0,
            }
        )]
    );
    assert_eq!(agent(&realm, THREE).await.0, "ended");
    realm.host.shutdown().await.unwrap();
}
