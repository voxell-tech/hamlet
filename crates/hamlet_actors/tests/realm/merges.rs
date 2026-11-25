//! A merge of two hires and its split, and an ended hire restored
//! and erased.

use std::time::Duration;

use hamlet_actor_core::ShutdownKind;
use hamlet_messages::address::{self, Party as _};
use hamlet_messages::profile::{CommitId, Sha1, Source};
use hamlet_messages::realm::hires::{
    EraseHire, Merge, MergeRefusal, MergedTo, Restore, Restored,
};
use hamlet_messages::realm::{
    Asked, ChiefRequest, Done, End, Found, FoundAnswer, Gate,
    LeadRefusal, Limit, Named, ProfileId, ReservedAct, SetLimits,
    Tier,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::{Choice, RequestId};

use super::profiles::create;
use super::reserved::{lay as road, notices};
use super::stand_ins::{Callee, Seen};
use super::{
    FOUR, MAYOR, Realm, THREE, answer, chief_hires, closed_until,
    decisions, hire, hires, made, shown, to, tool, until, words,
};

const FIVE: AgentNumber = AgentNumber(5);

/// The commit the stand-in profile answers a merge with.
const MERGED: CommitId = CommitId(Sha1([7; 20]));

async fn chief<T: ChiefRequest>(realm: &Realm, act: T) -> T::Answer {
    realm
        .ask(&Asked {
            id: RequestId::new(),
            act,
        })
        .await
        .unwrap()
}

fn merge(absorbed: AgentNumber, into: AgentNumber) -> Merge {
    Merge {
        absorbed: to(absorbed),
        into: to(into),
        why: None,
    }
}

fn restore(hire: AgentNumber) -> Restore {
    Restore {
        hire: to(hire),
        why: None,
    }
}

fn erase(hire: AgentNumber) -> EraseHire {
    EraseHire { hire: to(hire) }
}

async fn end(realm: &Realm, hire: AgentNumber) {
    let ended = chief(
        realm,
        End {
            hire: to(hire),
            why: None,
        },
    )
    .await;
    assert_eq!(ended, Done::Done);
}

/// A realm whose acts wait for their steps at most `within`.
async fn answering_within(within: Duration) -> Realm {
    let options = hamlet_actors::realm::Options {
        answer_within: within,
        ..hamlet_actors::realm::Options::default()
    };
    let realm =
        Realm::start_with(Some(options), Seen::default()).await;
    realm.live(Hand::NUMBER).await;
    super::founded(&realm).await;
    realm.live(MAYOR).await;
    for n in 0..2 {
        hires(&realm, MAYOR, &format!("h{n}")).await;
        realm.live(AgentNumber(3 + n)).await;
    }
    realm
}

/// The agent's status, how it ended and its lead.
async fn agent(
    realm: &Realm,
    agent: AgentNumber,
) -> (String, Option<serde_json::Value>, Option<u32>) {
    let (status, how, lead) = realm
        .stopped_rows(|db| {
            db.query_row(
                "SELECT agent.status, agent.ended_how, hire.lead \
                 FROM agent JOIN hire ON hire.agent = agent.id \
                 WHERE agent.id = ?1",
                [agent.0],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get(2)?,
                    ))
                },
            )
        })
        .await;
    let how = how.map(|how| serde_json::from_str(&how).unwrap());
    (status, how, lead)
}

async fn lay(realm: &Realm, a: AgentNumber, b: AgentNumber) {
    assert_eq!(chief(realm, road(a, b)).await, Done::Done);
}

async fn open_roads(realm: &Realm) -> Vec<(u32, u32)> {
    realm
        .stopped_rows(|db| {
            db.prepare(
                "SELECT a, b FROM road WHERE up_at IS NULL ORDER BY \
                 a, b",
            )?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect()
        })
        .await
}

fn retires(realm: &Realm) -> Vec<AgentNumber> {
    realm.seen.retires.lock().unwrap().clone()
}

/// The last step `agent`'s hire row did.
async fn hire_done(
    realm: &Realm,
    agent: AgentNumber,
) -> Option<String> {
    realm
        .stopped_rows(move |db| {
            db.query_row(
                "SELECT done FROM step WHERE kind = 'hire' AND n = \
                 ?1",
                [agent.0],
                |row| row.get(0),
            )
        })
        .await
}

fn fills_of(realm: &Realm, agent: AgentNumber) -> usize {
    let fills = realm.seen.fills.lock().unwrap();
    fills.iter().filter(|(to, _)| *to == agent).count()
}

/// How many retires, merges and unmerges the stand-ins took.
fn counts(realm: &Realm) -> [usize; 3] {
    let seen = &realm.seen;
    [
        seen.retires.lock().unwrap().len(),
        seen.merges.lock().unwrap().len(),
        seen.unmerges.lock().unwrap().len(),
    ]
}

/// The hamlet's mayor, with hires 3 and 4 of one shared profile.
async fn two_of_one_profile() -> Realm {
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;
    let hamlet = super::founded(&realm).await;
    realm.live(MAYOR).await;
    create(&realm, Hand::NUMBER, "c1", "Mason", None).await;
    for hire_n in [THREE, FOUR] {
        let act = hamlet_messages::realm::Hire {
            profile: Some(ProfileId(3)),
            ..hire(None)
        };
        chief_hires(&realm, &hamlet, act).await;
        realm.live(hire_n).await;
    }
    realm
}

#[tokio::test(flavor = "multi_thread")]
async fn merging_moves_the_absorbed_hires_hires_to_the_one_that_stays()
 {
    // mayor 2 leads 3, 6 and 7, and 3 leads 4 and 5; 3 merges into
    // 4: 4 takes 3's lead and 5 goes under 4. 3's road to 7 moves to
    // 4; its road to 6 goes, as 4 has one there.
    let (realm, _) = super::hamlet_with(1).await;
    for (lead, call) in
        [(THREE, "h4"), (THREE, "h5"), (MAYOR, "h6"), (MAYOR, "h7")]
    {
        hires(&realm, lead, call).await;
    }
    let (six, seven) = (AgentNumber(6), AgentNumber(7));
    for agent in [FOUR, FIVE, six, seven] {
        realm.live(agent).await;
    }
    for (a, b) in [(THREE, six), (FOUR, six), (THREE, seven)] {
        lay(&realm, a, b).await;
    }

    let merged = chief(&realm, merge(THREE, FOUR)).await;

    assert_eq!(merged, Done::Done);
    assert_eq!(
        agent(&realm, THREE).await,
        (
            "ended".to_owned(),
            Some(serde_json::json!({"type": "merged", "into": 4})),
            Some(MAYOR.0)
        )
    );
    assert_eq!(agent(&realm, FOUR).await.2, Some(MAYOR.0));
    assert_eq!(agent(&realm, FIVE).await.2, Some(FOUR.0));
    assert_eq!(open_roads(&realm).await, [(4, 6), (4, 7)]);
    assert_eq!(decisions(&realm).await.last().unwrap(), "merged");
    until("4 and 5 are told their places", || {
        let places = realm.seen.places.lock().unwrap();
        let last =
            |agent| {
                places.iter().rev().find(|(to, _)| *to == agent).map(
                    |(_, place)| (place.lead, place.hires.clone()),
                )
            };
        last(FOUR) == Some((Some(MAYOR), vec![FIVE]))
            && last(FIVE) == Some((Some(FOUR), Vec::new()))
    })
    .await;
    assert_eq!(retires(&realm), [THREE], "one retire for the end");
    let seen = realm.seen.merges.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    assert_eq!(
        seen[0].0,
        ProfileId(4),
        "into the kept hire's profile"
    );
    assert_eq!(seen[0].1.into, "hires/4");
    assert_eq!(
        seen[0].1.message,
        "Took in what hire#3 learned: merged by the chief"
    );
    assert!(
        matches!(seen[0].1.from, Source::Part { last: true, .. }),
        "another profile's branch comes in parts"
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_merge_within_one_profile_takes_the_branch() {
    let realm = two_of_one_profile().await;
    lay(&realm, THREE, FOUR).await;

    chief(&realm, merge(THREE, FOUR)).await;

    assert!(
        open_roads(&realm).await.is_empty(),
        "the road to 4 goes"
    );

    let seen = realm.seen.merges.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    assert_eq!(
        (seen[0].0, &seen[0].1.from),
        (
            ProfileId(3),
            &Source::Branch {
                name: "hires/3".to_owned()
            }
        )
    );
    assert!(realm.seen.bundles.lock().unwrap().is_empty());
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_mayor_cannot_be_absorbed() {
    let (realm, _) = super::hamlet_with(1).await;

    let absorbed = chief(&realm, merge(MAYOR, THREE)).await;
    let kept = chief(&realm, merge(THREE, MAYOR)).await;

    assert_eq!(
        words(absorbed),
        "a mayor cannot be ended; seat another mayor first"
    );
    assert_eq!(kept, Done::Done, "a mayor may be kept");
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_merge_refuses_in_todays_order() {
    let (realm, _) = super::hamlet_with(2).await;
    let other = chief(
        &realm,
        Found {
            name: "Weir".to_owned(),
            mayor: None,
            tier: Tier::Fifty,
            purpose: None,
        },
    )
    .await;
    let FoundAnswer::Founded { mayor: other, .. } = other else {
        panic!("a second hamlet: {other:?}");
    };
    let nine = AgentNumber(9);
    let refused = async |act| words(chief(&realm, act).await);

    assert_eq!(
        refused(merge(nine, AgentNumber(10))).await,
        "no agent has the address `hire#9`"
    );
    assert_eq!(
        refused(Merge {
            absorbed: Named::Hand,
            into: to(nine),
            why: None,
        })
        .await,
        "no agent has the address `hire#9`",
        "the chief's names are both found first"
    );
    assert_eq!(
        refused(Merge {
            absorbed: Named::Hand,
            into: to(THREE),
            why: None,
        })
        .await,
        "`hand` is not a hire"
    );
    assert_eq!(
        refused(merge(THREE, THREE)).await,
        "a road needs two different hires"
    );
    assert_eq!(
        refused(merge(THREE, other)).await,
        "`hire#3 (Hire 1)` is in another hamlet"
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lead_merges_only_hires_under_it() {
    // mayor 2 leads 3 and 4; 3 leads 5. 3 may not merge 4, nor merge
    // 5 into itself; the mayor merges 5 into 4.
    let (realm, _) = super::hamlet_with(2).await;
    hires(&realm, THREE, "h3").await;
    realm.live(FIVE).await;
    let merges = async |by, call: &str, act| {
        realm.tool(by, &tool(call, act)).await.unwrap()
    };

    let not_under = merges(THREE, "m1", merge(FIVE, FOUR)).await;
    let into_itself = merges(THREE, "m2", merge(FIVE, THREE)).await;
    let merged = merges(MAYOR, "m3", merge(FIVE, FOUR)).await;

    assert_eq!(
        words(not_under),
        "only a lead of `hire#4 (Hire 2)` may do that"
    );
    assert_eq!(
        words(into_itself),
        "only a lead of `hire#3 (Hire 1)` may do that"
    );
    assert_eq!(
        merged,
        MergedTo::Done {
            absorbed: shown(FIVE),
            into: shown(FOUR),
        }
    );
    assert_eq!(
        agent(&realm, FIVE).await.1,
        Some(
            serde_json::json!({"type": "merged", "into": 4, "by": 2})
        )
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_merge_reserved_by_the_limits_waits_for_a_chief_and_runs_once_allowed()
 {
    let (realm, hamlet) = super::hamlet_with(2).await;
    let set = chief(
        &realm,
        SetLimits {
            hamlet: Some(hamlet),
            budget: Vec::new(),
            limits: vec![Limit::Reserved {
                act: ReservedAct::Merge,
                reserved: true,
            }],
        },
    )
    .await;
    assert_eq!(set, Done::Done);

    let kept = realm
        .tool(MAYOR, &tool("m1", merge(THREE, FOUR)))
        .await
        .unwrap();
    let before = agent(&realm, THREE).await.0;
    let allowed =
        realm.ask(&answer("#1", Choice::Allow, None)).await.unwrap();

    assert_eq!(
        kept,
        MergedTo::<LeadRefusal<MergeRefusal>>::Refused {
            why: LeadRefusal::Gate(Gate::Reserved {
                wait: hamlet_messages::realm::world::WaitId(1)
            })
        }
    );
    assert_eq!(before, "live", "nothing is merged before the chief");
    assert_eq!(allowed, Done::Done);
    assert_eq!(
        notices(&realm, MAYOR).await,
        ["Merged hire#3 (Hire 1) into hire#4 (Hire 2)."]
    );
    assert_eq!(
        agent(&realm, THREE).await.1,
        Some(
            serde_json::json!({"type": "merged", "into": 4, "by": 2})
        )
    );
    closed_until(&realm, "merge", 1).await;
    assert_eq!(counts(&realm), [1, 1, 0]);
    realm.host.shutdown().await.unwrap();
}

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
async fn a_restore_refuses_in_todays_order() {
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
        "the scholar `scholar#3 (Hire 1)` no longer lives in the \
         city"
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_restore_of_a_merged_hire_splits_it_again() {
    let (realm, _) = super::hamlet_with(2).await;
    chief(&realm, merge(THREE, FOUR)).await;

    let split = chief(&realm, restore(THREE)).await;

    assert_eq!(split, Restored::Split);
    assert_eq!(
        agent(&realm, THREE).await,
        ("live".to_owned(), None, Some(MAYOR.0))
    );
    let unmerges = realm.seen.unmerges.lock().unwrap().clone();
    assert_eq!(unmerges.len(), 1);
    assert_eq!(
        (unmerges[0].0, unmerges[0].1.branch.as_str()),
        (ProfileId(4), "hires/4")
    );
    assert_eq!(unmerges[0].1.merge, MERGED);
    assert_eq!(
        fills_of(&realm, FOUR),
        3,
        "the hire, merge and split"
    );
    assert!(decisions(&realm).await.ends_with(&[
        "merged".to_owned(),
        "restored".to_owned(),
        "split".to_owned()
    ]));
    let again = chief(&realm, merge(THREE, FOUR)).await;
    assert_eq!(again, Done::Done, "a hire split away merges again");
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_split_waits_for_its_merge_to_end() {
    // The acts answer while the merge is held; no assertion depends
    // on how long they wait.
    let realm = answering_within(Duration::from_millis(200)).await;
    realm.seen.hold(Callee::Merge);

    chief(&realm, merge(THREE, FOUR)).await;
    chief(&realm, restore(THREE)).await;
    until("the merge waits in the profile", || {
        realm.seen.is_waiting(Callee::Merge, THREE)
    })
    .await;
    let unmerged_early = realm.seen.unmerges.lock().unwrap().len();
    realm.seen.release(Callee::Merge);
    closed_until(&realm, "split", 1).await;

    assert_eq!(unmerged_early, 0);
    let unmerges = realm.seen.unmerges.lock().unwrap().clone();
    assert_eq!(unmerges.len(), 1);
    assert_eq!(unmerges[0].1.merge, MERGED, "the merge's own commit");
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

/// Sets the closed row `kind` `n` back to before each step that is
/// not its last, `NULL` first, and waits each time for it to close
/// again.
async fn done_again(
    realm: &Realm,
    kind: &str,
    n: i64,
    done: &[&str],
) {
    let set_back = std::iter::once("NULL".to_owned())
        .chain(done.iter().map(|step| format!("'{step}'")));
    for done in set_back {
        realm
            .write(&format!(
                "UPDATE step SET done = {done}, closed = 0 WHERE \
                 kind = '{kind}' AND n = {n};"
            ))
            .await;
        closed_until(realm, kind, n).await;
    }
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
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn every_merge_step_twice_merges_once() {
    // fail each step's callee once in turn; each run ends with one
    // retire and one merge, and the kept hire filled once more than
    // its hire filled it.
    for (callee, at) in [
        (Callee::Retire, THREE),
        (Callee::Bundle, AgentNumber(3)),
        (Callee::Merge, THREE),
        (Callee::Fill, FOUR),
    ] {
        let (realm, _) = super::hamlet_with(2).await;
        realm.seen.fail_once(callee, at);

        chief(&realm, merge(THREE, FOUR)).await;
        until("the stand-in fails its call", || {
            realm.seen.has_failed(callee, at)
        })
        .await;
        let realm = realm.restart().await;
        closed_until(&realm, "merge", 1).await;

        assert_eq!(
            (counts(&realm), fills_of(&realm, FOUR)),
            ([1, 1, 0], 2),
            "{callee:?} failed once"
        );

        done_again(&realm, "merge", 1, &["retire", "merge", "fill"])
            .await;
        realm.host.shutdown().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn every_split_step_twice_splits_once() {
    for (callee, at) in
        [(Callee::Unmerge, AgentNumber(4)), (Callee::Fill, FOUR)]
    {
        let (realm, _) = super::hamlet_with(2).await;
        chief(&realm, merge(THREE, FOUR)).await;
        closed_until(&realm, "merge", 1).await;
        realm.seen.fail_once(callee, at);

        chief(&realm, restore(THREE)).await;
        until("the stand-in fails its call", || {
            realm.seen.has_failed(callee, at)
        })
        .await;
        let realm = realm.restart().await;
        closed_until(&realm, "split", 1).await;

        assert_eq!(
            (counts(&realm)[2], fills_of(&realm, FOUR)),
            (1, 3),
            "{callee:?} failed once"
        );

        done_again(&realm, "split", 1, &["unmerge"]).await;
        realm.host.shutdown().await.unwrap();
    }
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
