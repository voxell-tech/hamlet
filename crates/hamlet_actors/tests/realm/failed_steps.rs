//! A row of a merge, a split, a retire or an erase whose callee
//! fails for good.

use hamlet_messages::workspace::AgentNumber;

use super::merges::{chief, end, erase, merge, restore};
use super::stand_ins::Callee;
use super::{FOUR, THREE, closed_until, until};

#[tokio::test(flavor = "multi_thread")]
async fn a_row_whose_callee_fails_for_good_closes() {
    for (callee, at, kind, n) in [
        (Callee::Retire, THREE, "retire", 3),
        (Callee::Merge, THREE, "merge", 1),
        (Callee::Unmerge, AgentNumber(4), "split", 1),
        (Callee::Erase, THREE, "erase", 3),
    ] {
        let (realm, _) = super::hamlet_with(2).await;
        realm.seen.fail_for_good(callee, at);

        match kind {
            "retire" => end(&realm, THREE).await,
            "erase" => {
                end(&realm, THREE).await;
                chief(&realm, erase(THREE)).await;
            }
            _ => {
                chief(&realm, merge(THREE, FOUR)).await;
                if kind == "split" {
                    chief(&realm, restore(THREE)).await;
                }
            }
        }
        closed_until(&realm, kind, n).await;

        if kind != "split" {
            until("the hire's parts are dropped", || {
                !realm.seen.offer_drops.lock().unwrap().is_empty()
            })
            .await;
        }
        realm.host.shutdown().await.unwrap();
    }
}
