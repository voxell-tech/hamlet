//! An act whose step fails for good: what the tree, the rows and the
//! record say afterwards, what the chief and a calling lead are told,
//! and the command that repairs it.

use std::time::Duration;

use hamlet_messages::agent::Input;
use hamlet_messages::profile::VersionName;
use hamlet_messages::realm::hires::{
    EndedRefusal, MergeRefusal, MergedTo, Restored,
};
use hamlet_messages::realm::profiles::{
    EraseProfile, ForkFailure, ForkProfile, Forked, LifeRefusal,
    Lifecycle,
};
use hamlet_messages::realm::{
    Asked, Done, End, LeadRefusal, ProfileId,
};
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::RequestId;

use super::lifecycle::{MASON, archive, masons_hamlet};
use super::merges::{
    agent, answering_within, chief, decided, end, erase, kept, merge,
    merge_rows, open_roads, restore,
};
use super::stand_ins::Callee;
use super::{
    FOUR, MAYOR, Realm, THREE, closed_until, done_to, hires, shown,
    submits_to, to, tool, until,
};

const FIVE: AgentNumber = AgentNumber(5);

/// The commit the stand-in profile answers a merge with.
const MERGED: &str = "0707070707070707070707070707070707070707";

/// The window an act whose step fails only in passing answers within.
const SHORT: Duration = Duration::from_millis(300);

/// The step row `kind` `n`'s last step done, and whether it closed.
async fn step(
    realm: &Realm,
    kind: &str,
    n: u32,
) -> (Option<String>, bool) {
    let kind = kind.to_owned();
    realm
        .stopped_rows(move |db| {
            db.query_row(
                "SELECT done, closed FROM step WHERE kind = ?1 AND \
                 n = ?2",
                rusqlite::params![kind, n],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
        })
        .await
}

async fn erased(realm: &Realm, agent: AgentNumber) -> bool {
    realm
        .stopped_rows(move |db| {
            db.query_row(
                "SELECT erased FROM agent WHERE id = ?1",
                [agent.0],
                |row| row.get(0),
            )
        })
        .await
}

async fn profile_status(realm: &Realm, profile: ProfileId) -> String {
    realm
        .stopped_rows(move |db| {
            db.query_row(
                "SELECT status FROM profile WHERE id = ?1",
                [profile.0],
                |row| row.get(0),
            )
        })
        .await
}

/// Waits until `callee` refused `agent` twice, so its row is retried
/// rather than ended.
async fn refused_twice(
    realm: &Realm,
    callee: Callee,
    agent: AgentNumber,
) {
    until(
        &format!("{callee:?} of {} is refused twice", agent.0),
        || realm.seen.refused(callee, agent) >= 2,
    )
    .await;
}

/// The notices `agent` was sent.
fn told(realm: &Realm, agent: AgentNumber) -> Vec<String> {
    submits_to(realm, agent)
        .into_iter()
        .filter_map(|submit| match submit.input {
            Input::Notice { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

async fn told_until(
    realm: &Realm,
    agent: AgentNumber,
) -> Vec<String> {
    until(&format!("{} is sent a notice", agent.0), || {
        !told(realm, agent).is_empty()
    })
    .await;
    told(realm, agent)
}

fn not_merged() -> MergeRefusal {
    MergeRefusal::NotMerged {
        absorbed: shown(THREE),
        into: shown(FOUR),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_whose_retire_fails_stays_ended() {
    // the retire of 3, a chief's end, and of 4, the mayor's, fail for
    // good; both stay ended and are told so; /restore brings 3 back.
    let (realm, _) = super::hamlet_with(2).await;
    realm.seen.fail_for_good(Callee::Retire, THREE);
    realm.seen.fail_for_good(Callee::Retire, FOUR);

    end(&realm, THREE).await;
    let by_lead = realm
        .tool(
            MAYOR,
            &tool(
                "e1",
                End {
                    hire: to(FOUR),
                    why: None,
                },
            ),
        )
        .await
        .unwrap();
    closed_until(&realm, "retire", 3).await;
    closed_until(&realm, "retire", 4).await;

    assert_eq!(by_lead, done_to(FOUR));
    for hire in [THREE, FOUR] {
        assert_eq!(agent(&realm, hire).await.0, "ended");
        assert_eq!(
            step(&realm, "retire", hire.0).await,
            (None, true)
        );
    }
    assert_eq!(decided(&realm, "ended").await, 2);

    let restored = chief(&realm, restore(THREE)).await;
    assert_eq!(restored, Restored::Back);
    realm.live(THREE).await;
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_merge_whose_retire_fails_is_taken_back() {
    // a chief merges 3 into 4 and the retire of 3 fails for good: 3
    // lives again under the mayor with its roads, the merge is split
    // and unrecorded, and the chief is told; the merge sent again
    // merges.
    let (realm, _) = super::hamlet_with(2).await;
    let roads = open_roads(&realm).await;
    realm.seen.fail_for_good(Callee::Retire, THREE);
    let asked = Asked {
        id: RequestId::new(),
        act: merge(THREE, FOUR),
    };

    let merged = realm.ask(&asked).await.unwrap();
    let resent = realm.ask(&asked).await.unwrap();

    assert_eq!(merged, Done::Refused { why: not_merged() });
    assert_eq!(resent, merged);
    assert_eq!(
        agent(&realm, THREE).await,
        ("live".into(), None, Some(2))
    );
    assert_eq!(open_roads(&realm).await, roads);
    assert_eq!(step(&realm, "merge", 1).await, (None, true));
    assert_eq!(merge_rows(&realm).await, [(None, true)]);
    assert_eq!(decided(&realm, "merged").await, 0);

    realm.seen.stop_refusing(Callee::Retire, THREE);
    let again = chief(&realm, merge(THREE, FOUR)).await;
    assert_eq!(again, Done::Done);
    assert_eq!(agent(&realm, THREE).await.0, "ended");
    assert_eq!(decided(&realm, "merged").await, 1);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_merge_whose_commit_fails_is_taken_back() {
    // the mayor merges 3, which leads 5, into 4, and the profile's
    // merge fails for good: 5 is back under 3 with its road, 3 lives
    // under the mayor, and the mayor is answered and told; the merge
    // sent again merges.
    let (realm, _) = super::hamlet_with(2).await;
    hires(&realm, THREE, "h3").await;
    realm.live(FIVE).await;
    let roads = open_roads(&realm).await;
    realm.seen.fail_for_good(Callee::Merge, THREE);

    let merged = realm
        .tool(MAYOR, &tool("m1", merge(THREE, FOUR)))
        .await
        .unwrap();

    assert_eq!(
        merged,
        MergedTo::Refused {
            why: LeadRefusal::Act(not_merged())
        }
    );
    let resent = realm
        .tool(MAYOR, &tool("m1", merge(THREE, FOUR)))
        .await
        .unwrap();
    assert_eq!(resent, merged);
    assert_eq!(
        told_until(&realm, MAYOR).await,
        [not_merged().to_string()]
    );
    assert_eq!(
        agent(&realm, THREE).await,
        ("live".into(), None, Some(2))
    );
    assert_eq!(agent(&realm, FIVE).await.2, Some(3));
    assert_eq!(open_roads(&realm).await, roads);
    assert_eq!(
        step(&realm, "merge", 1).await,
        (Some("retire".into()), true)
    );
    assert_eq!(merge_rows(&realm).await, [(None, true)]);
    assert_eq!(decided(&realm, "merged").await, 0);

    realm.seen.stop_refusing(Callee::Merge, THREE);
    let again = realm
        .tool(MAYOR, &tool("m2", merge(THREE, FOUR)))
        .await
        .unwrap();
    assert!(matches!(again, MergedTo::Done { .. }), "{again:?}");
    assert_eq!(agent(&realm, FIVE).await.2, Some(4));
    assert_eq!(decided(&realm, "merged").await, 1);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lead_answered_before_its_merge_fails_is_told() {
    // the profile's merge waits past the answer, then fails for good:
    // the mayor was answered done, and is told it was taken back.
    let realm = answering_within(SHORT).await;
    realm.seen.hold(Callee::Merge);
    realm.seen.fail_for_good(Callee::Merge, THREE);

    let merged = realm
        .tool(MAYOR, &tool("m1", merge(THREE, FOUR)))
        .await
        .unwrap();
    until("the merge waits in the profile", || {
        realm.seen.is_waiting(Callee::Merge, THREE)
    })
    .await;
    realm.seen.release(Callee::Merge);

    assert!(matches!(merged, MergedTo::Done { .. }), "{merged:?}");
    assert_eq!(
        told_until(&realm, MAYOR).await,
        [not_merged().to_string()]
    );
    closed_until(&realm, "merge", 1).await;
    assert_eq!(agent(&realm, THREE).await.0, "live");
    assert_eq!(merge_rows(&realm).await, [(None, true)]);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_merge_whose_fill_fails_is_retried_until_done() {
    // after the merge's commit the fill of 4 is refused: the row is
    // retried, not ended, and closes once the fill is taken.
    let realm = answering_within(SHORT).await;
    realm.seen.fail_for_good(Callee::Fill, FOUR);

    let merged = chief(&realm, merge(THREE, FOUR)).await;
    refused_twice(&realm, Callee::Fill, FOUR).await;

    assert_eq!(merged, Done::Done);
    assert_eq!(
        step(&realm, "merge", 1).await,
        (Some("merge".into()), false)
    );

    realm.seen.stop_refusing(Callee::Fill, FOUR);
    closed_until(&realm, "merge", 1).await;
    assert_eq!(
        merge_rows(&realm).await,
        [(Some(MERGED.into()), false)]
    );
    assert_eq!(agent(&realm, THREE).await.0, "ended");
    assert_eq!(decided(&realm, "merged").await, 1);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_split_whose_unmerge_fails_is_taken_back() {
    // 3 merged into 4; its restore's unmerge fails for good: 3 stays
    // back, the merge stands again, and the chief is told; /end then
    // /restore splits it.
    let (realm, _) = super::hamlet_with(2).await;
    assert_eq!(chief(&realm, merge(THREE, FOUR)).await, Done::Done);
    realm.seen.fail_for_good(Callee::Unmerge, FOUR);

    let restored = chief(&realm, restore(THREE)).await;

    assert_eq!(
        restored,
        Restored::Refused {
            why: EndedRefusal::NotSplit {
                hire: shown(THREE),
                into: shown(FOUR),
            }
        }
    );
    assert_eq!(agent(&realm, THREE).await.0, "live");
    assert_eq!(step(&realm, "split", 1).await, (None, true));
    assert_eq!(
        merge_rows(&realm).await,
        [(Some(MERGED.into()), false)]
    );
    assert_eq!(decided(&realm, "restored").await, 1);
    assert_eq!(decided(&realm, "split").await, 0);

    realm.seen.stop_refusing(Callee::Unmerge, FOUR);
    end(&realm, THREE).await;
    assert_eq!(chief(&realm, restore(THREE)).await, Restored::Split);
    assert_eq!(
        merge_rows(&realm).await,
        [(Some(MERGED.into()), true)]
    );
    assert_eq!(decided(&realm, "split").await, 1);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_split_whose_fill_fails_is_retried_until_done() {
    let realm = answering_within(SHORT).await;
    assert_eq!(chief(&realm, merge(THREE, FOUR)).await, Done::Done);
    closed_until(&realm, "merge", 1).await;
    realm.seen.fail_for_good(Callee::Fill, FOUR);

    let restored = chief(&realm, restore(THREE)).await;
    refused_twice(&realm, Callee::Fill, FOUR).await;

    assert_eq!(restored, Restored::Split);
    assert_eq!(
        step(&realm, "split", 1).await,
        (Some("unmerge".into()), false)
    );

    realm.seen.stop_refusing(Callee::Fill, FOUR);
    closed_until(&realm, "split", 1).await;
    assert_eq!(
        merge_rows(&realm).await,
        [(Some(MERGED.into()), true)]
    );
    assert_eq!(decided(&realm, "split").await, 1);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_erase_whose_branch_drop_fails_is_taken_back() {
    // the ended 3's erase fails for good at its branch: 3 stays
    // ended, not erased, and the chief is told; /erase sent again
    // erases it.
    let (realm, _) = super::hamlet_with(2).await;
    end(&realm, THREE).await;
    realm.seen.fail_for_good(Callee::DropBranch, THREE);

    let erasing = chief(&realm, erase(THREE)).await;

    assert_eq!(
        erasing,
        Done::Refused {
            why: EndedRefusal::NotErased { hire: shown(THREE) }
        }
    );
    assert_eq!(agent(&realm, THREE).await.0, "ended");
    assert!(!erased(&realm, THREE).await);
    assert_eq!(step(&realm, "erase", 3).await, (None, true));
    assert_eq!(decided(&realm, "erased").await, 0);

    realm.seen.stop_refusing(Callee::DropBranch, THREE);
    assert_eq!(chief(&realm, erase(THREE)).await, Done::Done);
    assert!(erased(&realm, THREE).await);
    assert_eq!(decided(&realm, "erased").await, 1);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_erase_whose_agent_fails_is_retried_until_done() {
    let realm = answering_within(SHORT).await;
    end(&realm, THREE).await;
    closed_until(&realm, "retire", 3).await;
    realm.seen.fail_for_good(Callee::Erase, THREE);

    let erasing = chief(&realm, erase(THREE)).await;
    refused_twice(&realm, Callee::Erase, THREE).await;

    assert_eq!(erasing, Done::Done);
    assert_eq!(
        step(&realm, "erase", 3).await,
        (Some("branch".into()), false)
    );
    assert!(erased(&realm, THREE).await);

    realm.seen.stop_refusing(Callee::Erase, THREE);
    closed_until(&realm, "erase", 3).await;
    assert_eq!(kept(&realm)[3], 1);
    assert_eq!(decided(&realm, "erased").await, 1);
    realm.host.shutdown().await.unwrap();
}

fn fork() -> ForkProfile {
    ForkProfile {
        from: MASON,
        at: Some(VersionName("v1".to_owned())),
        name: "Mason two".to_owned(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fork_whose_profile_cannot_be_made_is_not_forked() {
    // the fork's profile 4 cannot be made: it is erased unseen, the
    // chief is told, and nothing is recorded; /fork again makes 5.
    let (realm, _) = masons_hamlet(0).await;
    realm.seen.fail_for_good(Callee::Fork, AgentNumber(4));

    let forked = chief(&realm, fork()).await;

    assert_eq!(
        forked,
        Forked::Refused {
            why: ForkFailure::NotForked {
                label: "scholar#3 (Mason)".into()
            }
            .into()
        }
    );
    assert_eq!(profile_status(&realm, ProfileId(4)).await, "erased");
    assert_eq!(step(&realm, "profile_fork", 4).await, (None, true));
    assert_eq!(decided(&realm, "forked").await, 0);

    let again = chief(&realm, fork()).await;
    assert!(
        matches!(
            again,
            Forked::Forked {
                profile: ProfileId(5),
                ..
            }
        ),
        "{again:?}"
    );
    assert_eq!(profile_status(&realm, ProfileId(5)).await, "living");
    assert_eq!(decided(&realm, "forked").await, 1);
    realm.host.shutdown().await.unwrap();
}

fn not_erased() -> Lifecycle {
    Lifecycle::Refused {
        why: LifeRefusal::NotErased {
            label: "scholar#3 (Mason)".into(),
        }
        .into(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profile_whose_erase_fails_is_archived_again() {
    // the archived Mason's erase fails for good at the profile: it is
    // archived again, its hire's erase stays done, and the chief is
    // told; /erase-scholar sent again erases it.
    let (realm, _) = masons_hamlet(1).await;
    chief(&realm, archive(MASON)).await;
    let profile = AgentNumber(MASON.0);
    realm.seen.fail_for_good(Callee::EraseProfile, profile);

    let erasing =
        chief(&realm, EraseProfile { profile: MASON }).await;

    assert_eq!(erasing, not_erased());
    assert_eq!(profile_status(&realm, MASON).await, "archived");
    assert_eq!(
        step(&realm, "profile_erase", 3).await,
        (Some("hires".into()), true)
    );
    assert!(erased(&realm, THREE).await);
    assert_eq!(decided(&realm, "erased").await, 1);
    assert_eq!(decided(&realm, "profile-erased").await, 0);

    realm.seen.stop_refusing(Callee::EraseProfile, profile);
    let again = chief(&realm, EraseProfile { profile: MASON }).await;
    assert!(matches!(again, Lifecycle::Done { .. }), "{again:?}");
    assert_eq!(profile_status(&realm, MASON).await, "erased");
    assert_eq!(decided(&realm, "profile-erased").await, 1);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profile_whose_hire_cannot_be_erased_is_archived_again() {
    // the branch of Mason's hire 3 cannot be dropped: 3 stays ended,
    // Mason is archived again and the chief is told; sent again once
    // the branch drops, both are erased.
    let (realm, _) = masons_hamlet(1).await;
    chief(&realm, archive(MASON)).await;
    realm.seen.fail_for_good(Callee::DropBranch, THREE);

    let erasing =
        chief(&realm, EraseProfile { profile: MASON }).await;

    assert_eq!(erasing, not_erased());
    assert_eq!(profile_status(&realm, MASON).await, "archived");
    assert_eq!(step(&realm, "profile_erase", 3).await, (None, true));
    assert!(!erased(&realm, THREE).await);
    assert_eq!(decided(&realm, "erased").await, 0);
    assert_eq!(decided(&realm, "profile-erased").await, 0);

    realm.seen.stop_refusing(Callee::DropBranch, THREE);
    let again = chief(&realm, EraseProfile { profile: MASON }).await;
    assert!(matches!(again, Lifecycle::Done { .. }), "{again:?}");
    assert!(erased(&realm, THREE).await);
    assert_eq!(decided(&realm, "erased").await, 1);
    assert_eq!(decided(&realm, "profile-erased").await, 1);
    realm.host.shutdown().await.unwrap();
}
