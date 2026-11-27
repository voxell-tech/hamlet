//! A hamlet removed, restored and erased, and a profile forked,
//! archived, restored, erased and placed in the city.

use std::sync::atomic::{AtomicBool, Ordering};

use hamlet_actor_core::ShutdownKind;
use hamlet_messages::address::{self, Party as _};
use hamlet_messages::profile::{
    CommitId, Missing, ProfileInput, Sha1, VersionName,
};
use hamlet_messages::realm::client::{ReadState, ReadWorld};
use hamlet_messages::realm::hamlets::{
    EraseHamlet, RemoveHamlet, RestoreHamlet,
};
use hamlet_messages::realm::profiles::{
    ArchiveProfile, CreateRefusal, EraseProfile, ForkProfile,
    ForkRefusal, Forked, LifeRefusal, Lifecycle, LifecycleRefusal,
    MoveHouse, ProfileRefusal, RestoreProfile, Site, Spot, TurnAway,
    Welcome,
};
use hamlet_messages::realm::world::{Seen, Status};
use hamlet_messages::realm::{
    AgentRefusal, Done, HamletId, HamletRefusal, Hire, HireAnswer,
    HireRefusal, HowEnded, LeadRefusal, Left, ProfileId,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::{self as protocol, Choice};

use super::chiefs::chief;
use super::profiles::create;
use super::reserved::{in_turn, notices, waits};
use super::stand_ins::Callee;
use super::world::{agent, seen_until};
use super::{
    FOUR, MAYOR, Realm, THREE, answer, closed_until, hamlet_with,
    hire, hires, named_as, shown, tool, until,
};

const FIVE: AgentNumber = AgentNumber(5);

/// The profile a hamlet's hires are made of in these tests.
pub(super) const MASON: ProfileId = ProfileId(3);

fn mason() -> String {
    "profile#3 (Mason)".to_owned()
}

fn remove(hamlet: &HamletId) -> RemoveHamlet {
    RemoveHamlet {
        hamlet: hamlet.clone(),
        why: None,
    }
}

fn restore(hamlet: &HamletId) -> RestoreHamlet {
    RestoreHamlet {
        hamlet: hamlet.clone(),
        why: None,
    }
}

fn erase(hamlet: &HamletId) -> EraseHamlet {
    EraseHamlet {
        hamlet: hamlet.clone(),
    }
}

fn hamlet_refused(why: HamletRefusal) -> Done<HamletRefusal> {
    Done::Refused { why }
}

fn status(seen: &Seen, of: AgentNumber) -> Option<Status> {
    agent(seen, of).map(|shown| shown.status.clone())
}

fn ended(how: HowEnded, why: Option<&str>) -> Status {
    Status::Ended {
        left: Left {
            how,
            why: why.map(str::to_owned),
        },
    }
}

async fn world(realm: &Realm) -> Seen {
    realm.ask(&ReadWorld).await.unwrap()
}

/// A hamlet whose mayor leads hires 3 and 4, and 3 leads hire 5.
async fn hamlet_of_four() -> (Realm, HamletId) {
    let (realm, hamlet) = hamlet_with(2).await;
    hires(&realm, THREE, "h5").await;
    realm.live(FIVE).await;
    (realm, hamlet)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removed_hamlets_hires_end_and_come_back_on_restore() {
    // remove; every hire ended with its hamlet, the tree kept, the
    // hamlet not shown; restore; every hire live under its lead.
    let (realm, hamlet) = hamlet_of_four().await;

    let removed = chief(&realm, remove(&hamlet)).await;
    let gone = world(&realm).await;
    let restored = chief(&realm, restore(&hamlet)).await;
    let back = seen_until(&realm, "the hires are back", |seen| {
        [MAYOR, THREE, FOUR, FIVE]
            .iter()
            .all(|&hire| status(seen, hire) == Some(Status::Live))
    })
    .await;

    assert_eq!((removed, restored), (Done::Done, Done::Done));
    let removal = ended(
        HowEnded::HamletRemoved { by: None },
        Some("closed by the chief"),
    );
    for hire in [MAYOR, THREE, FOUR, FIVE] {
        assert_eq!(status(&gone, hire), Some(removal.clone()));
    }
    assert!(gone.hamlets.is_empty(), "{:?}", gone.hamlets);
    assert_eq!(back.hamlets.len(), 1);
    let leads = |seen: &Seen| {
        [THREE, FOUR, FIVE]
            .map(|hire| agent(seen, hire).unwrap().lead)
    };
    assert_eq!(leads(&gone), [Some(MAYOR), Some(MAYOR), Some(THREE)]);
    assert_eq!(leads(&back), leads(&gone));
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_a_hamlet_refuses_a_missing_then_an_erased_then_a_removed_one()
 {
    let (realm, hamlet) = hamlet_with(0).await;
    let missing = HamletId("nowhere".to_owned());

    let unknown = chief(&realm, remove(&missing)).await;
    chief(&realm, remove(&hamlet)).await;
    let removed = chief(&realm, remove(&hamlet)).await;
    chief(&realm, erase(&hamlet)).await;
    let erased = chief(&realm, remove(&hamlet)).await;

    assert_eq!(
        [unknown, removed, erased],
        [
            hamlet_refused(HamletRefusal::NoSuchHamlet {
                id: missing
            }),
            hamlet_refused(HamletRefusal::Removed {
                id: hamlet.clone()
            }),
            hamlet_refused(HamletRefusal::Erased { id: hamlet }),
        ]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn restoring_a_hamlet_refuses_a_missing_erased_or_living_one_and_one_whose_profile_left()
 {
    // a hire of Mason in the hamlet; remove; archive Mason; restore
    // is refused for it; restore Mason; restore; erase; restore.
    let (realm, hamlet) = masons_hamlet(1).await;
    let missing = HamletId("nowhere".to_owned());

    let unknown = chief(&realm, restore(&missing)).await;
    let living = chief(&realm, restore(&hamlet)).await;
    chief(&realm, remove(&hamlet)).await;
    chief(&realm, archive(MASON)).await;
    let profile_gone = chief(&realm, restore(&hamlet)).await;
    chief(&realm, restore_profile(MASON)).await;
    let restored = chief(&realm, restore(&hamlet)).await;
    chief(&realm, remove(&hamlet)).await;
    chief(&realm, erase(&hamlet)).await;
    let erased = chief(&realm, restore(&hamlet)).await;

    assert_eq!(
        [unknown, living, profile_gone, restored, erased],
        [
            hamlet_refused(HamletRefusal::NoSuchHamlet {
                id: missing
            }),
            hamlet_refused(HamletRefusal::Living {
                id: hamlet.clone()
            }),
            hamlet_refused(HamletRefusal::ProfileGone {
                label: mason()
            }),
            Done::Done,
            hamlet_refused(HamletRefusal::Erased { id: hamlet }),
        ]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn erasing_a_hamlet_refuses_a_missing_living_or_erased_one_and_hides_its_hires()
 {
    let (realm, hamlet) = hamlet_of_four().await;
    let missing = HamletId("nowhere".to_owned());

    let unknown = chief(&realm, erase(&missing)).await;
    let living = chief(&realm, erase(&hamlet)).await;
    chief(&realm, remove(&hamlet)).await;
    let erased = chief(&realm, erase(&hamlet)).await;
    let again = chief(&realm, erase(&hamlet)).await;
    let seen = world(&realm).await;

    assert_eq!(
        [unknown, living, erased, again],
        [
            hamlet_refused(HamletRefusal::NoSuchHamlet {
                id: missing
            }),
            hamlet_refused(HamletRefusal::Living {
                id: hamlet.clone()
            }),
            Done::Done,
            hamlet_refused(HamletRefusal::Erased { id: hamlet }),
        ]
    );
    let shown = seen
        .agents
        .iter()
        .map(|shown| shown.agent)
        .collect::<Vec<_>>();
    assert_eq!(shown, [Hand::NUMBER]);
    assert!(seen.hamlets.is_empty());
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chiefs_hire_into_a_removed_then_an_erased_hamlet_is_refused()
 {
    let (realm, hamlet) = hamlet_of_four().await;
    let into = || Hire {
        hamlet: Some(hamlet.clone()),
        ..hire(None)
    };

    chief(&realm, remove(&hamlet)).await;
    let removed = chief(&realm, into()).await;
    chief(&realm, erase(&hamlet)).await;
    let erased = chief(&realm, into()).await;

    assert_eq!(
        [removed, erased],
        [
            HireAnswer::Refused {
                why: HireRefusal::HamletRemoved {
                    hamlet: hamlet.clone()
                }
            },
            HireAnswer::Refused {
                why: HireRefusal::HamletErased {
                    hamlet: hamlet.clone()
                }
            },
        ]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_hands_remove_hamlet_waits_for_a_chief_and_runs_once_allowed()
 {
    let (realm, hamlet) = hamlet_with(1).await;
    let removing = RemoveHamlet {
        hamlet: hamlet.clone(),
        why: Some("done with it".to_owned()),
    };

    let kept = realm
        .tool(Hand::NUMBER, &in_turn("r1", removing, None))
        .await
        .unwrap();
    let before = world(&realm).await;
    let subjects = waits(&realm)
        .await
        .into_iter()
        .map(|(_, kind, subject, _)| (kind, subject))
        .collect::<Vec<_>>();
    let allowed =
        realm.ask(&answer("#1", Choice::Allow, None)).await.unwrap();
    let after = seen_until(&realm, "the hires end", |seen| {
        status(seen, THREE) != Some(Status::Live)
    })
    .await;
    until("the Hand is told", || {
        !super::submits_to(&realm, Hand::NUMBER).is_empty()
    })
    .await;

    assert_eq!(
        kept,
        Done::Refused {
            why: AgentRefusal::Gate(
                hamlet_messages::realm::Gate::Reserved {
                    wait: hamlet_messages::realm::world::WaitId(1)
                }
            )
        }
    );
    assert_eq!(
        before.hamlets.len(),
        1,
        "nothing runs before a chief"
    );
    assert_eq!(
        subjects,
        [(
            "reserved".to_owned(),
            format!(
                r#"remove_hamlet {{"hamlet":"{}","reason":"done with it"}}"#,
                hamlet.0
            )
        )]
    );
    assert_eq!(allowed, Done::Done);
    assert_eq!(
        notices(&realm, Hand::NUMBER).await,
        [format!("Removed {}.", hamlet.0)]
    );
    assert_eq!(
        status(&after, THREE),
        Some(ended(
            HowEnded::HamletRemoved {
                by: Some(shown(Hand::NUMBER))
            },
            Some("done with it")
        ))
    );
    assert!(after.hamlets.is_empty());
    realm.host.shutdown().await.unwrap();
}

fn of(profile: ProfileId) -> Hire {
    Hire {
        profile: Some(profile),
        ..hire(None)
    }
}

async fn mayor_hires(
    realm: &Realm,
    call: &str,
    hiring: Hire,
) -> HireAnswer<LeadRefusal<HireRefusal>> {
    realm.tool(MAYOR, &tool(call, hiring)).await.unwrap()
}

/// A hamlet whose mayor leads `count` hires of Mason, from 3 on.
pub(super) async fn masons_hamlet(count: u32) -> (Realm, HamletId) {
    let (realm, hamlet) = hamlet_with(0).await;
    create(&realm, Hand::NUMBER, "c1", "Mason", None).await;
    for n in 0..count {
        let hired =
            mayor_hires(&realm, &format!("h{n}"), of(MASON)).await;
        assert!(
            matches!(hired, HireAnswer::Hired { .. }),
            "{hired:?}"
        );
        realm.live(AgentNumber(3 + n)).await;
    }
    (realm, hamlet)
}

pub(super) fn archive(profile: ProfileId) -> ArchiveProfile {
    ArchiveProfile { profile, why: None }
}

fn restore_profile(profile: ProfileId) -> RestoreProfile {
    RestoreProfile { profile, why: None }
}

fn done(label: String) -> Lifecycle {
    Lifecycle::Done { label }
}

fn life_refused(why: impl Into<LifecycleRefusal>) -> Lifecycle {
    Lifecycle::Refused { why: why.into() }
}

fn no_such(profile: ProfileId) -> ProfileRefusal {
    ProfileRefusal::NoSuchProfile { profile }
}

fn no_such_hire(
    profile: ProfileId,
) -> HireAnswer<LeadRefusal<HireRefusal>> {
    HireAnswer::Refused {
        why: LeadRefusal::Act(HireRefusal::NoSuchProfile { profile }),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_archived_profiles_hires_end() {
    // two hires of Mason; archive; both ended, a hire of Mason
    // refused; restore; both live, a hire of Mason made.
    let (realm, _) = masons_hamlet(2).await;

    let archived = chief(&realm, archive(MASON)).await;
    let gone = world(&realm).await;
    let refused = mayor_hires(&realm, "h9", of(MASON)).await;
    let restored = chief(&realm, restore_profile(MASON)).await;
    seen_until(&realm, "the hires are back", |seen| {
        [THREE, FOUR]
            .iter()
            .all(|&hire| status(seen, hire) == Some(Status::Live))
    })
    .await;
    let hired = mayor_hires(&realm, "h10", of(MASON)).await;

    assert_eq!(archived, done(mason()));
    for hire in [THREE, FOUR] {
        assert_eq!(
            status(&gone, hire),
            Some(ended(HowEnded::ProfileArchived, None))
        );
    }
    assert_eq!(refused, no_such_hire(MASON));
    assert_eq!(restored, done(mason()));
    assert_eq!(
        hired,
        HireAnswer::Hired {
            agent: named_as(5, "hire#5 (Mason)")
        }
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn archiving_a_profile_refuses_a_missing_one_then_one_a_mayor_is_of()
 {
    let (realm, _) = masons_hamlet(1).await;

    let missing = chief(&realm, archive(ProfileId(99))).await;
    let mayors = chief(&realm, archive(ProfileId(2))).await;
    chief(&realm, archive(MASON)).await;
    let again = chief(&realm, archive(MASON)).await;
    let seen = world(&realm).await;

    assert_eq!(
        [missing, mayors, again],
        [
            life_refused(no_such(ProfileId(99))),
            life_refused(LifeRefusal::TheMayor),
            life_refused(no_such(MASON)),
        ]
    );
    assert_eq!(status(&seen, MAYOR), Some(Status::Live));
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn restoring_a_profile_refuses_a_missing_living_or_erased_one()
{
    let (realm, _) = masons_hamlet(1).await;

    let missing = chief(&realm, restore_profile(ProfileId(99))).await;
    let living = chief(&realm, restore_profile(MASON)).await;
    chief(&realm, archive(MASON)).await;
    chief(&realm, EraseProfile { profile: MASON }).await;
    let erased = chief(&realm, restore_profile(MASON)).await;

    assert_eq!(
        [missing, living, erased],
        [
            life_refused(no_such(ProfileId(99))),
            life_refused(LifeRefusal::Living { label: mason() }),
            life_refused(LifeRefusal::Erased { label: mason() }),
        ]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn restoring_a_profile_into_a_hamlet_with_one_place_left_is_refused_whole()
 {
    // two hires of Mason; archive; 48 other hires fill the hamlet to
    // 49 of its 50; restore is refused, and neither hire is back.
    let (realm, hamlet) = masons_hamlet(2).await;
    chief(&realm, archive(MASON)).await;
    realm
        .write(&format!(
            "WITH RECURSIVE n(i) AS (SELECT 100 UNION ALL SELECT i \
             + 1 FROM n WHERE i < 147) INSERT INTO agent (id, kind, \
             status, at) SELECT i, 'hire', 'live', 0 FROM n; WITH \
             RECURSIVE n(i) AS (SELECT 100 UNION ALL SELECT i + 1 \
             FROM n WHERE i < 147) INSERT INTO hire (agent, hamlet, \
             profile, lead) SELECT i, '{}', 2, 2 FROM n;",
            hamlet.0
        ))
        .await;

    let full = chief(&realm, restore_profile(MASON)).await;
    let seen = world(&realm).await;

    assert_eq!(
        full,
        life_refused(LifeRefusal::Full { hamlet, most: 50 })
    );
    assert_eq!(
        status(&seen, THREE),
        Some(ended(HowEnded::ProfileArchived, None))
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_erased_profile_answers_no_call() {
    // two hires of Mason; archive; erase; the profile is erased once,
    // its hires are not shown, and a hire or a second erase of it is
    // refused with no call to it.
    let (realm, _) = masons_hamlet(2).await;
    chief(&realm, archive(MASON)).await;

    let living = chief(
        &realm,
        EraseProfile {
            profile: ProfileId(99),
        },
    )
    .await;
    let erased = chief(&realm, EraseProfile { profile: MASON }).await;
    until("the profile is erased", || {
        !realm.seen.profile_erases.lock().unwrap().is_empty()
    })
    .await;
    let calls_before = profile_calls(&realm);
    let hire = mayor_hires(&realm, "h9", of(MASON)).await;
    let again = chief(&realm, EraseProfile { profile: MASON }).await;
    let seen = world(&realm).await;

    assert_eq!(living, life_refused(no_such(ProfileId(99))));
    assert_eq!(erased, done(mason()));
    assert_eq!(hire, no_such_hire(MASON));
    assert_eq!(
        again,
        life_refused(LifeRefusal::Erased { label: mason() })
    );
    assert_eq!(*realm.seen.profile_erases.lock().unwrap(), [MASON]);
    assert_eq!(profile_calls(&realm), calls_before);
    assert!(
        agent(&seen, THREE).is_none() && agent(&seen, FOUR).is_none()
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profiles_erase_and_its_answer_wait_for_its_hires_branches_to_drop()
 {
    let (realm, _) = masons_hamlet(2).await;
    chief(&realm, archive(MASON)).await;
    realm.seen.hold(Callee::DropBranch);

    let answered = AtomicBool::new(false);
    let erasing = async {
        let erased =
            chief(&realm, EraseProfile { profile: MASON }).await;
        answered.store(true, Ordering::SeqCst);
        erased
    };
    let held = async {
        until("both hires' branch drops wait", || {
            realm.seen.is_waiting(Callee::DropBranch, THREE)
                && realm.seen.is_waiting(Callee::DropBranch, FOUR)
        })
        .await;
        let early = (
            answered.load(Ordering::SeqCst),
            realm.seen.profile_erases.lock().unwrap().clone(),
        );
        realm.seen.release(Callee::DropBranch);
        early
    };
    let (erased, early) = tokio::join!(erasing, held);

    assert_eq!(early, (false, Vec::new()));
    assert_eq!(erased, done(mason()));
    assert_eq!(*realm.seen.profile_erases.lock().unwrap(), [MASON]);
    let dropped = realm
        .seen
        .branch_drops
        .lock()
        .unwrap()
        .iter()
        .map(|(profile, drop)| (*profile, drop.name.clone()))
        .collect::<Vec<_>>();
    assert_eq!(dropped.len(), 2);
    for hire in ["hires/3", "hires/4"] {
        assert!(dropped.contains(&(MASON, hire.into())), "{hire}");
    }
    realm.host.shutdown().await.unwrap();
}

/// How many calls of each kind Mason's profile took.
fn profile_calls(realm: &Realm) -> [usize; 3] {
    fn count<T>(calls: &[(ProfileId, T)]) -> usize {
        calls
            .iter()
            .filter(|(profile, _)| *profile == MASON)
            .count()
    }
    [
        count(&realm.seen.branches.lock().unwrap()),
        count(&realm.seen.reads.lock().unwrap()),
        count(&realm.seen.branch_drops.lock().unwrap()),
    ]
}

#[tokio::test(flavor = "multi_thread")]
async fn erasing_a_profile_refuses_a_living_one() {
    let (realm, _) = masons_hamlet(0).await;

    let living = chief(&realm, EraseProfile { profile: MASON }).await;

    assert_eq!(
        living,
        life_refused(LifeRefusal::Living { label: mason() })
    );
    assert!(realm.seen.profile_erases.lock().unwrap().is_empty());
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn every_profile_erase_step_twice_erases_it_and_closes_its_row()
{
    // erase Mason with its Erase failing once, then set its row back
    // before each step in turn: the profile is asked again each time
    // and the row closes.
    let (realm, _) = masons_hamlet(1).await;
    chief(&realm, archive(MASON)).await;
    realm
        .seen
        .fail_once(Callee::EraseProfile, AgentNumber(MASON.0));

    chief(&realm, EraseProfile { profile: MASON }).await;
    closed_until(&realm, "profile_erase", 3).await;
    for done in ["NULL", "'hires'"] {
        let before = realm.seen.profile_erases.lock().unwrap().len();
        realm
            .write(&format!(
                "UPDATE step SET done = {done}, closed = 0 WHERE \
                 kind = 'profile_erase' AND n = 3;"
            ))
            .await;
        closed_until(&realm, "profile_erase", 3).await;
        let after = realm.seen.profile_erases.lock().unwrap().len();
        assert!(after > before, "set back to {done}");
    }

    assert!(
        realm.seen.has_failed(Callee::EraseProfile, AgentNumber(3))
    );
    let erased = realm.seen.profile_erases.lock().unwrap().clone();
    assert!(erased.iter().all(|&profile| profile == MASON));
    realm.host.shutdown().await.unwrap();
}

/// Mason's hamlet with one hire, Mason archived and erased, and
/// Mason's actor gone as a profile goes once it has answered an
/// erase.
async fn mason_gone() -> Realm {
    let (realm, _) = masons_hamlet(1).await;
    chief(&realm, archive(MASON)).await;
    chief(&realm, EraseProfile { profile: MASON }).await;
    closed_until(&realm, "profile_erase", 3).await;
    closed_until(&realm, "drop_parts", 3).await;
    realm
        .host
        .stop(
            &address::Profile(MASON).address().unwrap(),
            ShutdownKind::Destroy,
        )
        .await
        .unwrap();
    realm
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profile_erase_done_again_after_the_profile_is_gone_closes()
{
    let realm = mason_gone().await;

    realm
        .write(
            "UPDATE step SET done = 'hires', closed = 0 WHERE kind \
             = 'profile_erase' AND n = 3;",
        )
        .await;

    closed_until(&realm, "profile_erase", 3).await;
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_drop_parts_row_done_again_after_its_profile_is_gone_closes()
 {
    let realm = mason_gone().await;

    realm
        .write(
            "UPDATE step SET done = NULL, closed = 0 WHERE kind = \
             'drop_parts' AND n = 3;",
        )
        .await;

    closed_until(&realm, "drop_parts", 3).await;
    realm.host.shutdown().await.unwrap();
}

fn fork(at: Option<&str>, name: &str) -> ForkProfile {
    ForkProfile {
        from: MASON,
        at: at.map(|at| VersionName(at.to_owned())),
        name: name.to_owned(),
    }
}

/// The commit every stand-in profile names a version by.
const STAND_IN_COMMIT: CommitId = CommitId(Sha1([7; 20]));

fn forked_from(profile: u32) -> (ProfileId, ProfileInput) {
    (
        ProfileId(profile),
        ProfileInput::Fork {
            profile: MASON,
            at: STAND_IN_COMMIT,
        },
    )
}

fn made_profiles(realm: &Realm) -> Vec<(ProfileId, ProfileInput)> {
    realm.seen.profiles.lock().unwrap().clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profile_forked_at_a_version_hires_with_that_versions_files()
 {
    // fork Mason at v1 as profile 4; it is made from v1's commit; a
    // hire of it branches from the fork's own profile.
    let (realm, _) = masons_hamlet(0).await;

    let forked = chief(&realm, fork(Some("v1"), "Mason two")).await;
    let hired = mayor_hires(&realm, "h1", of(ProfileId(4))).await;
    realm.live(THREE).await;
    let state: protocol::State = realm.ask(&ReadState).await.unwrap();
    let mason_two = state
        .profiles
        .iter()
        .find(|profile| profile.name == "Mason two")
        .unwrap();

    assert_eq!(
        forked,
        Forked::Forked {
            profile: ProfileId(4),
            from: mason()
        }
    );
    assert!(made_profiles(&realm).contains(&forked_from(4)));
    assert!(mason_two.since > 0);
    assert_eq!(mason_two.arrival_by.as_deref(), Some("chief"));
    assert_eq!(
        hired,
        HireAnswer::Hired {
            agent: named_as(3, "hire#3 (Mason two)")
        }
    );
    let branched = realm.seen.branches.lock().unwrap().clone();
    assert!(
        branched.iter().any(|(profile, branch)| {
            *profile == ProfileId(4) && branch.name == "hires/3"
        }),
        "{branched:?}"
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn forking_refuses_a_missing_profile_then_a_missing_version_then_a_bad_name()
 {
    let (realm, _) = masons_hamlet(0).await;

    let missing = chief(
        &realm,
        ForkProfile {
            from: ProfileId(99),
            ..fork(Some("v9"), "a: b")
        },
    )
    .await;
    let version = chief(&realm, fork(Some("v9"), "a: b")).await;
    let name = chief(&realm, fork(Some("v1"), "a: b")).await;

    let refused = |why: ForkRefusal| Forked::Refused { why };
    assert_eq!(
        [missing, version, name],
        [
            refused(no_such(ProfileId(99)).into()),
            refused(
                Missing::NoVersion {
                    profile: MASON,
                    version: VersionName("v9".to_owned())
                }
                .into()
            ),
            refused(
                CreateRefusal::BadName {
                    name: "a: b".to_owned()
                }
                .into()
            ),
        ]
    );
    assert_eq!(made_profiles(&realm).len(), 3);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn every_fork_step_twice_leaves_one_profile() {
    // fork; set its row back before each step in turn, the profile
    // making again: each run ends with one profile made from v1,
    // live.
    let (realm, _) = masons_hamlet(0).await;
    chief(&realm, fork(Some("v1"), "Mason two")).await;

    for done in ["NULL", "'create'"] {
        realm
            .write(&format!(
                "UPDATE step SET done = {done}, closed = 0 WHERE \
                 kind = 'profile_fork' AND n = 4; UPDATE profile \
                 SET status = 'making' WHERE id = 4;"
            ))
            .await;
        closed_until(&realm, "profile_fork", 4).await;
        let status = realm
            .stopped_rows(|db| {
                db.query_row(
                    "SELECT status FROM profile WHERE id = 4",
                    [],
                    |row| row.get::<_, String>(0),
                )
            })
            .await;
        assert_eq!(status, "living", "set back to {done}");
    }

    let made = made_profiles(&realm);
    assert_eq!(made.last(), Some(&forked_from(4)));
    assert_eq!(made.len(), 4);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn placing_a_profile_refuses_in_order() {
    // Mason, housed; turn it away twice; move its house; welcome it
    // twice; move its house to the middle, then to a clear spot.
    let (realm, _) = masons_hamlet(0).await;
    let place = |x, z| MoveHouse {
        profile: MASON,
        site: Site::Spot(Spot {
            x,
            z,
            degrees: None,
        }),
    };
    let welcome = || Welcome {
        profile: MASON,
        why: None,
    };
    let turn_away = TurnAway {
        profile: MASON,
        why: None,
    };

    let answers = [
        chief(
            &realm,
            Welcome {
                profile: ProfileId(99),
                why: None,
            },
        )
        .await,
        chief(
            &realm,
            Welcome {
                profile: ProfileId(2),
                why: None,
            },
        )
        .await,
        chief(&realm, turn_away.clone()).await,
        chief(&realm, turn_away).await,
        chief(&realm, place(100.0, 0.0)).await,
        chief(&realm, welcome()).await,
        chief(&realm, welcome()).await,
        chief(&realm, place(0.0, 0.0)).await,
        chief(&realm, place(100.0, 0.0)).await,
    ];
    let house = realm
        .stopped_rows(|db| {
            db.query_row(
                "SELECT house_x, house_z FROM profile WHERE id = 3",
                [],
                |row| {
                    Ok((row.get::<_, f64>(0)?, row.get::<_, f64>(1)?))
                },
            )
        })
        .await;

    assert_eq!(
        answers,
        [
            life_refused(no_such(ProfileId(99))),
            life_refused(ProfileRefusal::Apart {
                label: "profile#2 (Mayor)".to_owned()
            }),
            done(mason()),
            life_refused(LifeRefusal::TurnedAway { label: mason() }),
            life_refused(LifeRefusal::AtHarbor { label: mason() }),
            done(mason()),
            life_refused(LifeRefusal::Housed { label: mason() }),
            life_refused(LifeRefusal::Crowded { label: mason() }),
            done(mason()),
        ]
    );
    assert_eq!(house, (100.0, 0.0));
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_removed_with_its_hamlet_while_it_was_made_is_made_once_restored()
 {
    let (realm, hamlet) = hamlet_with(0).await;
    realm.seen.hold(Callee::Create);
    hires(&realm, MAYOR, "h0").await;
    until("the create waits", || {
        realm.seen.is_waiting(Callee::Create, THREE)
    })
    .await;

    chief(&realm, remove(&hamlet)).await;
    chief(&realm, restore(&hamlet)).await;
    realm.seen.release(Callee::Create);
    closed_until(&realm, "hire", 3).await;

    let (status, done) = realm
        .stopped_rows(|db| {
            db.query_row(
                "SELECT agent.status, step.done FROM agent JOIN \
                 step ON step.kind = 'hire' AND step.n = agent.id \
                 WHERE agent.id = 3",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get(1)?)),
            )
        })
        .await;
    assert_eq!(
        (status.as_str(), done),
        ("live", Some("live".to_owned()))
    );
    let fills = realm.seen.fills.lock().unwrap().clone();
    assert_eq!(
        fills.iter().filter(|(to, _)| *to == THREE).count(),
        1
    );
    realm.host.shutdown().await.unwrap();
}
