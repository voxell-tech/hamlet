//! A merge of two hires and its split.

use std::time::Duration;

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
    decisions, hire, hires, shown, to, tool, until, words,
};

const FIVE: AgentNumber = AgentNumber(5);

/// The commit the stand-in profile answers a merge with.
const MERGED: CommitId = CommitId(Sha1([7; 20]));

pub(super) async fn chief<T: ChiefRequest>(
    realm: &Realm,
    act: T,
) -> T::Answer {
    realm
        .ask(&Asked {
            id: RequestId::new(),
            act,
        })
        .await
        .unwrap()
}

pub(super) fn merge(
    absorbed: AgentNumber,
    into: AgentNumber,
) -> Merge {
    Merge {
        absorbed: to(absorbed),
        into: to(into),
        why: None,
    }
}

pub(super) fn restore(hire: AgentNumber) -> Restore {
    Restore {
        hire: to(hire),
        why: None,
    }
}

pub(super) fn erase(hire: AgentNumber) -> EraseHire {
    EraseHire { hire: to(hire) }
}

pub(super) async fn end(realm: &Realm, hire: AgentNumber) {
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
pub(super) async fn answering_within(within: Duration) -> Realm {
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
pub(super) async fn agent(
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

pub(super) fn retires(realm: &Realm) -> Vec<AgentNumber> {
    realm.seen.retires.lock().unwrap().clone()
}

/// The last step `agent`'s hire row did.
pub(super) async fn hire_done(
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

pub(super) fn fills_of(realm: &Realm, agent: AgentNumber) -> usize {
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

/// How many merges, unmerges, retires, agent erases and branch drops
/// the callees keep.
pub(super) fn kept(realm: &Realm) -> [usize; 5] {
    let kept = realm.seen.kept.lock().unwrap();
    [
        kept.merges.len(),
        kept.unmerges.len(),
        kept.retires.len(),
        kept.erases.len(),
        kept.branch_drops.len(),
    ]
}

/// How many of the realm's decisions are of `kind`.
pub(super) async fn decided(realm: &Realm, kind: &str) -> usize {
    decisions(realm)
        .await
        .iter()
        .filter(|seen| *seen == kind)
        .count()
}

/// Each merge row's tip and whether it was split.
async fn merge_rows(realm: &Realm) -> Vec<(Option<String>, bool)> {
    realm
        .stopped_rows(|db| {
            db.prepare("SELECT tip, split FROM merge ORDER BY id")?
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect()
        })
        .await
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

/// Sets the closed row `kind` `n` back to before each step that is
/// not its last, `NULL` first, and waits each time for it to close
/// again.
pub(super) async fn done_again(
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
        assert_eq!(kept(&realm), [1, 0, 1, 0, 0], "{callee:?}");
        assert_eq!(
            merge_rows(&realm).await,
            [(Some(MERGED.0.to_string()), false)]
        );
        assert_eq!(decided(&realm, "merged").await, 1);
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
        assert_eq!(kept(&realm), [1, 1, 1, 0, 0], "{callee:?}");
        assert_eq!(
            merge_rows(&realm).await,
            [(Some(MERGED.0.to_string()), true)]
        );
        assert_eq!(decided(&realm, "split").await, 1);
        assert_eq!(agent(&realm, THREE).await.0, "live");
        realm.host.shutdown().await.unwrap();
    }
}
