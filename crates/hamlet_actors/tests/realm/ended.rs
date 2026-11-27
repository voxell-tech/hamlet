//! An ended hire restored, retired and erased.

use std::time::Duration;

use hamlet_actor_core::ShutdownKind;
use hamlet_messages::address::{self, Party as _};
use hamlet_messages::realm::hires::{EraseHire, Restore, Restored};
use hamlet_messages::realm::{Done, Named, ProfileId};

use super::merges::{
    agent, answering_within, chief, decided, done_again, end, erase,
    fills_of, hire_done, kept, merge, restore, retires,
};
use super::stand_ins::Callee;
use super::{
    FOUR, MAYOR, Realm, THREE, closed_until, decisions, hire, hires,
    made, tool, until, words,
};

#[tokio::test(flavor = "multi_thread")]
async fn an_ended_hire_comes_back_with_its_files() {
    let (realm, _) = super::hamlet_with(1).await;
    end(&realm, THREE).await;

    let back = chief(&realm, restore(THREE)).await;

    assert_eq!(back, Restored::Back);
    assert_eq!(
        agent(&realm, THREE).await,
        ("live".to_owned(), None, Some(MAYOR.0))
    );
    assert_eq!(
        realm
            .seen
            .made
            .lock()
            .unwrap()
            .iter()
            .filter(|(made, _)| { *made == THREE })
            .count(),
        1,
        "its agent is not made again"
    );
    assert_eq!(fills_of(&realm, THREE), 1, "nor filled again");
    assert_eq!(super::closed(&realm, "hire", 3).await, Some(true));
    assert!(realm.seen.erases.lock().unwrap().is_empty());
    assert!(made(&realm, THREE).is_some());
    assert!(
        decisions(&realm)
            .await
            .ends_with(&["ended".to_owned(), "restored".to_owned()])
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_ended_while_it_was_made_is_made_once_restored() {
    for held in [Callee::Branch, Callee::Create] {
        let (realm, _) = super::hamlet_with(0).await;
        realm.seen.hold(held);
        realm.tool(MAYOR, &tool("h0", hire(None))).await.unwrap();
        until("the hire's step waits", || {
            realm.seen.is_waiting(held, THREE)
        })
        .await;
        end(&realm, THREE).await;

        let restored = chief(&realm, restore(THREE)).await;
        realm.seen.release(held);
        closed_until(&realm, "hire", 3).await;
        let live = hire_done(&realm, THREE).await;

        assert_eq!(restored, Restored::Back, "{held:?}");
        assert_eq!(live.as_deref(), Some("live"), "{held:?}");
        assert_eq!(agent(&realm, THREE).await.0, "live", "{held:?}");
        assert_eq!(fills_of(&realm, THREE), 1, "{held:?}");
        realm.host.shutdown().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_restore_refuses_in_order() {
    let (realm, hamlet) = super::hamlet_with(2).await;
    end(&realm, THREE).await;
    let refused = async |act| words(chief(&realm, act).await);

    assert_eq!(
        refused(restore(FOUR)).await,
        "`hire#4 (Hire 2)` has not left"
    );
    assert_eq!(
        refused(Restore {
            hire: Named::Hand,
            why: None,
        })
        .await,
        "`hand` is not a hire"
    );
    realm
        .write(&format!(
            "UPDATE hamlet SET status = 'removed' WHERE id = '{}'; \
             UPDATE profile SET status = 'archived' WHERE id = 3;",
            hamlet.0
        ))
        .await;
    assert_eq!(
        refused(restore(THREE)).await,
        format!(
            "the hamlet `{}` was removed; restore it first",
            hamlet.0
        )
    );
    realm.write("UPDATE hamlet SET status = 'living';").await;
    assert_eq!(
        refused(restore(THREE)).await,
        "the profile `profile#3 (Hire 1)` no longer lives in the \
         city"
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_erased_while_its_merge_runs_keeps_its_branch_until_the_merge_ends()
 {
    // The absorbed hire, then the kept one, is erased while the merge
    // waits in the profile; no assertion depends on how long the
    // erase's answer waits.
    for erased in [THREE, FOUR] {
        let realm =
            answering_within(Duration::from_millis(200)).await;
        realm.seen.hold(Callee::Merge);
        chief(&realm, merge(THREE, FOUR)).await;
        until("the merge waits in the profile", || {
            realm.seen.is_waiting(Callee::Merge, THREE)
        })
        .await;
        if erased == FOUR {
            end(&realm, FOUR).await;
        }

        chief(&realm, erase(erased)).await;
        let dropped_early =
            realm.seen.branch_drops.lock().unwrap().len();
        realm.seen.release(Callee::Merge);
        closed_until(&realm, "erase", i64::from(erased.0)).await;

        assert_eq!(dropped_early, 0, "hire {}", erased.0);
        assert_eq!(realm.seen.merges.lock().unwrap().len(), 1);
        let dropped = realm
            .seen
            .branch_drops
            .lock()
            .unwrap()
            .iter()
            .map(|(_, drop)| drop.name.clone())
            .collect::<Vec<_>>();
        assert_eq!(dropped, [format!("hires/{}", erased.0)]);
        realm.host.shutdown().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_erased_while_it_retires_keeps_its_branch_until_the_retire_ends()
 {
    let realm = answering_within(Duration::from_millis(200)).await;
    realm.seen.hold(Callee::Retire);
    end(&realm, THREE).await;
    until("the retire waits in the agent", || {
        realm.seen.is_waiting(Callee::Retire, THREE)
    })
    .await;

    chief(&realm, erase(THREE)).await;
    let dropped_early = realm.seen.branch_drops.lock().unwrap().len();
    realm.seen.release(Callee::Retire);
    closed_until(&realm, "erase", 3).await;

    assert_eq!(dropped_early, 0);
    assert_eq!(retires(&realm), [THREE]);
    assert_eq!(realm.seen.branch_drops.lock().unwrap().len(), 1);
    realm.host.shutdown().await.unwrap();
}

/// Waits until hire 3's retire row has closed, and its drop-parts row
/// too if one began; then gives whether any of its parts were
/// dropped.
async fn parts_dropped_after_retire(realm: &Realm) -> bool {
    closed_until(realm, "retire", 3).await;
    if super::closed(realm, "drop_parts", 3).await.is_some() {
        closed_until(realm, "drop_parts", 3).await;
    }
    !realm.seen.offer_drops.lock().unwrap().is_empty()
        || !realm.seen.save_drops.lock().unwrap().is_empty()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_retire_tried_again_after_a_restore_leaves_the_living_hire()
{
    let (realm, _) = super::hamlet_with(1).await;
    realm.seen.fail_once(Callee::Retire, THREE);
    end(&realm, THREE).await;
    until("the retire fails once", || {
        realm.seen.has_failed(Callee::Retire, THREE)
    })
    .await;

    let restored = chief(&realm, restore(THREE)).await;
    let dropped = parts_dropped_after_retire(&realm).await;

    assert_eq!(restored, Restored::Back);
    assert_eq!(retires(&realm), []);
    assert!(!dropped);
    assert_eq!(agent(&realm, THREE).await.0, "live");
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_restored_while_it_retires_keeps_its_parts() {
    // The retire already sent reaches the hire; nothing after it
    // does.
    let (realm, _) = super::hamlet_with(1).await;
    realm.seen.hold(Callee::Retire);
    end(&realm, THREE).await;
    until("the retire waits in the agent", || {
        realm.seen.is_waiting(Callee::Retire, THREE)
    })
    .await;

    let restored = chief(&realm, restore(THREE)).await;
    realm.seen.release(Callee::Retire);
    let dropped = parts_dropped_after_retire(&realm).await;

    assert_eq!(restored, Restored::Back);
    assert_eq!(retires(&realm), [THREE]);
    assert!(!dropped);
    assert_eq!(agent(&realm, THREE).await.0, "live");
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_erased_hire_cannot_be_restored() {
    let (realm, _) = super::hamlet_with(1).await;
    end(&realm, THREE).await;

    let erased = chief(&realm, erase(THREE)).await;
    let restored = chief(&realm, restore(THREE)).await;
    let again = chief(&realm, erase(THREE)).await;

    assert_eq!(erased, Done::Done);
    assert_eq!(words(restored), "no agent has the address `hire#3`");
    assert_eq!(words(again), "no agent has the address `hire#3`");
    assert_eq!(*realm.seen.erases.lock().unwrap(), [THREE]);
    let drops = realm.seen.branch_drops.lock().unwrap().clone();
    assert_eq!(drops.len(), 1);
    assert_eq!(
        (drops[0].0, drops[0].1.name.as_str()),
        (ProfileId(3), "hires/3")
    );
    assert_eq!(decisions(&realm).await.last().unwrap(), "erased");
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_erase_refuses_a_living_hire_and_the_hand() {
    let (realm, _) = super::hamlet_with(1).await;

    let living = chief(&realm, erase(THREE)).await;
    let hand = chief(&realm, EraseHire { hire: Named::Hand }).await;

    assert_eq!(words(living), "`hire#3 (Hire 1)` has not left");
    assert_eq!(words(hand), "`hand` is not a hire");
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_erased_hire_erased_again_closes_its_row() {
    let (realm, _) = super::hamlet_with(1).await;
    end(&realm, THREE).await;
    chief(&realm, erase(THREE)).await;
    realm
        .host
        .stop(
            &address::Agent(THREE).address().unwrap(),
            ShutdownKind::Destroy,
        )
        .await
        .unwrap();

    realm
        .write(
            "UPDATE step SET done = 'branch', closed = 0 WHERE kind \
             = 'erase' AND n = 3;",
        )
        .await;
    closed_until(&realm, "erase", 3).await;

    assert_eq!(*realm.seen.erases.lock().unwrap(), [THREE]);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_ended_again_after_a_restore_retires_again() {
    let (realm, _) = super::hamlet_with(1).await;
    end(&realm, THREE).await;
    until("the first retire", || retires(&realm).len() == 1).await;
    closed_until(&realm, "retire", 3).await;

    chief(&realm, restore(THREE)).await;
    end(&realm, THREE).await;

    until("the second retire", || retires(&realm).len() == 2).await;
    assert_eq!(retires(&realm), [THREE, THREE]);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_ended_hires_parts_are_dropped_once_it_has_retired() {
    let (realm, _) = super::hamlet_with(1).await;
    realm.seen.hold(Callee::Retire);

    end(&realm, THREE).await;
    until("the retire waits in the agent", || {
        realm.seen.is_waiting(Callee::Retire, THREE)
    })
    .await;
    let dropped_early = realm.seen.offer_drops.lock().unwrap().len();
    realm.seen.release(Callee::Retire);
    until("the library drops its offers", || {
        !realm.seen.offer_drops.lock().unwrap().is_empty()
    })
    .await;

    assert_eq!(dropped_early, 0);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_ended_while_hiring_retires_with_no_call() {
    let (realm, _) = super::hamlet_with(1).await;
    realm
        .write(
            "UPDATE step SET done = 'branch' WHERE kind = 'hire' \
             AND n = 3;",
        )
        .await;

    end(&realm, THREE).await;
    closed_until(&realm, "retire", 3).await;
    chief(&realm, erase(THREE)).await;
    closed_until(&realm, "erase", 3).await;

    assert!(retires(&realm).is_empty());
    assert!(realm.seen.erases.lock().unwrap().is_empty());
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_ended_before_its_fill_retires_with_no_call_and_is_erased()
 {
    let (realm, _) = super::hamlet_with(1).await;
    realm
        .write(
            "UPDATE step SET done = 'create' WHERE kind = 'hire' \
             AND n = 3;",
        )
        .await;

    end(&realm, THREE).await;
    closed_until(&realm, "retire", 3).await;
    chief(&realm, erase(THREE)).await;
    closed_until(&realm, "erase", 3).await;

    assert!(retires(&realm).is_empty());
    assert_eq!(*realm.seen.erases.lock().unwrap(), [THREE]);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn every_retire_step_twice_retires_once() {
    let (realm, _) = super::hamlet_with(1).await;
    realm.seen.fail_once(Callee::Retire, THREE);

    end(&realm, THREE).await;
    until("the stand-in fails its call", || {
        realm.seen.has_failed(Callee::Retire, THREE)
    })
    .await;
    let realm = realm.restart().await;
    closed_until(&realm, "retire", 3).await;
    assert_eq!(retires(&realm), [THREE]);

    done_again(&realm, "retire", 3, &[]).await;
    assert_eq!(kept(&realm), [0, 0, 1, 0, 0]);
    assert_eq!(decided(&realm, "ended").await, 1);
    assert_eq!(agent(&realm, THREE).await.0, "ended");
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn every_erase_step_twice_erases_once() {
    for callee in [Callee::DropBranch, Callee::Erase] {
        let (realm, _) = super::hamlet_with(1).await;
        end(&realm, THREE).await;
        realm.seen.fail_once(callee, THREE);

        chief(&realm, erase(THREE)).await;
        until("the stand-in fails its call", || {
            realm.seen.has_failed(callee, THREE)
        })
        .await;
        let realm = realm.restart().await;
        closed_until(&realm, "erase", 3).await;

        assert_eq!(
            (
                realm.seen.branch_drops.lock().unwrap().len(),
                realm.seen.erases.lock().unwrap().len()
            ),
            (1, 1),
            "{callee:?} failed once"
        );

        done_again(&realm, "erase", 3, &["branch"]).await;
        assert_eq!(kept(&realm), [0, 0, 1, 1, 1], "{callee:?}");
        assert_eq!(decided(&realm, "erased").await, 1);
        realm.host.shutdown().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_whose_lead_has_ended_comes_back_under_the_mayor() {
    let (realm, _) = super::hamlet_with(1).await;
    hires(&realm, THREE, "h3").await;
    realm.live(FOUR).await;
    end(&realm, FOUR).await;
    end(&realm, THREE).await;

    chief(&realm, restore(FOUR)).await;

    assert_eq!(agent(&realm, FOUR).await.2, Some(MAYOR.0));
    until("4 is told its place under the mayor", || {
        let places = realm.seen.places.lock().unwrap();
        places
            .iter()
            .rev()
            .find(|(to, _)| *to == FOUR)
            .is_some_and(|(_, place)| place.lead == Some(MAYOR))
    })
    .await;
    realm.host.shutdown().await.unwrap();
}
