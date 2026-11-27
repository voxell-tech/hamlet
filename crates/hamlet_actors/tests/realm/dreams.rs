use std::sync::atomic::Ordering;
use std::time::Duration;

use hamlet_actors::realm::StepTold;
use hamlet_messages::profile::{
    BadPath, By, DreamId, DreamOf, DreamOpen, PrepareDream, Purge,
    PurgeAnswer,
};
use hamlet_messages::realm::profiles::{
    Dream, DreamAskedRefusal, DreamRefusal, Dreamer, Dreamt,
    ProfileRefusal, PurgeProfile, PurgeProfileRefusal,
};
use hamlet_messages::realm::{ProfileId, Refusable as _};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::RequestId;

use super::chiefs::chief;
use super::profile_stand_in::{NOT_A_PATH, STILL_HOLDS};
use super::profiles::{asked, create};
use super::stand_ins::Callee;
use super::{
    Realm, THREE, dream_ends, hamlet_with, hire_3_ends, of_hire,
    steps_told, until,
};

fn purge(profile: u32, path: &str) -> PurgeProfile {
    PurgeProfile {
        profile: ProfileId(profile),
        path: path.to_owned(),
    }
}

fn purges(realm: &Realm) -> Vec<(ProfileId, Purge)> {
    realm.seen.purges.lock().unwrap().clone()
}

/// The longest a profile asks each of its hires to remove the path
/// before it rewrites its history.
const PROFILE_ASKS_WITHIN: Duration = Duration::from_secs(4);

/// The stand-in profile answers half a second past the time the
/// profile gives its hires, as a purge with one silent hire does. The
/// realm waits 8 s, so the answer has 3.5 s to spare, over ten times
/// the tenth of a second the rest of this test takes.
#[tokio::test(flavor = "multi_thread")]
async fn a_chiefs_purge_is_answered_from_the_profile_past_its_asks() {
    let (realm, _) = hamlet_with(0).await;
    create(&realm, Hand::NUMBER, "c1", "Mason", None).await;
    let id = RequestId::new();
    realm.seen.hold(Callee::Purge);
    let purged = {
        let (realm, id) = (&realm, id.clone());
        async move { asked(realm, &id, purge(3, "notes.md")).await }
    };
    let released = async {
        until("the profile is asked to purge", || {
            realm.seen.is_waiting(Callee::Purge, AgentNumber(3))
        })
        .await;
        tokio::time::sleep(
            PROFILE_ASKS_WITHIN + Duration::from_millis(500),
        )
        .await;
        realm.seen.release(Callee::Purge);
    };
    let (purged, ()) = tokio::join!(purged, released);

    assert_eq!(
        purged,
        PurgeAnswer::Purged {
            branches: 2,
            still_hold: vec![STILL_HOLDS],
        }
    );
    assert_eq!(
        purges(&realm),
        [(
            ProfileId(3),
            Purge {
                id,
                path: "notes.md".to_owned(),
                by: By::Chief { chief: 1 },
            }
        )]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_purge_is_refused_in_the_profiles_words_or_the_realms() {
    let (realm, _) = hamlet_with(0).await;
    create(&realm, Hand::NUMBER, "c1", "Mason", None).await;

    let bad =
        asked(&realm, &RequestId::new(), purge(3, NOT_A_PATH)).await;
    let missing =
        asked(&realm, &RequestId::new(), purge(9, "notes.md")).await;

    assert_eq!(
        bad,
        PurgeAnswer::Refused {
            why: PurgeProfileRefusal::Path(BadPath::NotAPath {
                path: NOT_A_PATH.to_owned(),
            }),
        }
    );
    assert_eq!(
        missing,
        PurgeAnswer::Refused {
            why: PurgeProfileRefusal::Profile(
                ProfileRefusal::NoSuchProfile {
                    profile: ProfileId(9),
                }
            ),
        }
    );
    assert_eq!(purges(&realm).len(), 1);
    realm.host.shutdown().await.unwrap();
}

fn prepares(realm: &Realm) -> Vec<(ProfileId, PrepareDream, bool)> {
    realm.seen.prepares.lock().unwrap().clone()
}

fn of_profile(profile: u32) -> Dream {
    Dream {
        of: Dreamer::Profile {
            profile: ProfileId(profile),
        },
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hires_dream_checks_its_folders_out_then_ends() {
    let (realm, _) = hamlet_with(1).await;
    realm.seen.nothing_to_dream.store(true, Ordering::SeqCst);

    let answer = chief(&realm, of_hire(THREE)).await;

    assert_eq!(answer, Dreamt::Nothing);
    until("the dream ends", || !dream_ends(&realm).is_empty()).await;
    let prepared = prepares(&realm);
    let [(profile, prepare, _)] = prepared.as_slice() else {
        panic!("one preparation: {prepared:?}");
    };
    assert_eq!(
        *prepare,
        PrepareDream {
            dream: DreamId(1),
            of: DreamOf::Hire { agent: THREE },
        }
    );
    assert_eq!(
        dream_ends(&realm),
        [(*profile, hire_3_ends(1, false, ""))]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn one_dream_is_open_in_the_realm_at_a_time() {
    let (realm, _) = hamlet_with(1).await;
    realm.seen.nothing_to_dream.store(true, Ordering::SeqCst);
    create(&realm, Hand::NUMBER, "c1", "Mason", None).await;
    realm.seen.hold(Callee::PrepareDream);

    let (first, second) =
        tokio::join!(chief(&realm, of_profile(4)), async {
            until(
                "the first dream's folders are being checked out",
                || {
                    realm.seen.is_waiting(
                        Callee::PrepareDream,
                        AgentNumber(4),
                    )
                },
            )
            .await;
            let second = chief(&realm, of_hire(THREE)).await;
            realm.seen.release(Callee::PrepareDream);
            second
        });
    until("the first dream ends", || !dream_ends(&realm).is_empty())
        .await;
    let third = chief(&realm, of_hire(THREE)).await;

    assert_eq!(first, Dreamt::Nothing);
    assert_eq!(
        second,
        Dreamt::Refused {
            why: DreamAskedRefusal::Dream(DreamRefusal::Running),
        }
    );
    assert_eq!(third, Dreamt::Nothing);
    until("the third dream ends", || dream_ends(&realm).len() == 2)
        .await;
    realm.host.shutdown().await.unwrap();
}

/// The dream's end is held until the chief's ask waits for it, so an
/// answer that comes first finds the end not kept.
#[tokio::test(flavor = "multi_thread")]
async fn a_dream_with_nothing_to_dream_has_ended_when_it_is_answered()
{
    let (realm, _) = hamlet_with(1).await;
    realm.seen.nothing_to_dream.store(true, Ordering::SeqCst);
    realm.seen.hold(Callee::EndDream);

    let ask = chief(&realm, of_hire(THREE));
    tokio::pin!(ask);
    let waits = StepTold::Waited {
        kind: "dream_end".to_owned(),
        n: 1,
    };
    let first = tokio::select! {
        first = &mut ask => first,
        () = until("the ask waits for the dream's end", || {
            steps_told(&realm, &waits)
        }) => {
            realm.seen.release(Callee::EndDream);
            ask.await
        }
    };
    let ended = dream_ends(&realm).len();
    realm.seen.release(Callee::EndDream);
    let next = chief(&realm, of_hire(THREE)).await;

    assert_eq!(
        (first, ended, next),
        (Dreamt::Nothing, 1, Dreamt::Nothing)
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dream_of_no_hire_or_no_shared_profile_is_refused() {
    let (realm, _) = hamlet_with(0).await;

    let no_hire = chief(&realm, of_hire(AgentNumber(77))).await;
    let apart = chief(&realm, of_profile(2)).await;
    let missing = chief(&realm, of_profile(9)).await;

    assert_eq!(
        no_hire,
        Dreamt::Refused {
            why: DreamAskedRefusal::Dream(DreamRefusal::NoSuchHire {
                hire: AgentNumber(77),
            }),
        }
    );
    assert_eq!(
        apart,
        Dreamt::Refused {
            why: DreamAskedRefusal::Profile(ProfileRefusal::Apart {
                label: "profile#2 (Mayor)".to_owned(),
            }),
        }
    );
    assert_eq!(
        missing,
        Dreamt::Refused {
            why: DreamAskedRefusal::Profile(
                ProfileRefusal::NoSuchProfile {
                    profile: ProfileId(9),
                }
            ),
        }
    );
    assert!(prepares(&realm).is_empty());
    realm.host.shutdown().await.unwrap();
}

/// The purge is held a second past the chief's dream, or until the
/// dream is prepared: a realm that does not hold the dream prepares
/// it within milliseconds, while the purge is still held.
#[tokio::test(flavor = "multi_thread")]
async fn no_dream_is_prepared_while_a_purge_is_in_flight() {
    let (realm, _) = hamlet_with(0).await;
    create(&realm, Hand::NUMBER, "c1", "Mason", None).await;
    realm.seen.hold(Callee::Purge);
    let id = RequestId::new();
    let purged = asked(&realm, &id, purge(3, "notes.md"));
    let dreamt = async {
        until("the profile is asked to purge", || {
            realm.seen.is_waiting(Callee::Purge, AgentNumber(3))
        })
        .await;
        let (answer, ()) =
            tokio::join!(chief(&realm, of_profile(3)), async {
                let _ = tokio::time::timeout(
                    Duration::from_secs(1),
                    until("the dream is prepared", || {
                        !prepares(&realm).is_empty()
                    }),
                )
                .await;
                realm.seen.release(Callee::Purge);
            });
        answer
    };
    let (purged, dreamt) = tokio::join!(purged, dreamt);

    assert!(matches!(purged, PurgeAnswer::Purged { .. }));
    assert!(matches!(dreamt, Dreamt::Began { .. }));
    until("the dream is prepared", || !prepares(&realm).is_empty())
        .await;
    let held = prepares(&realm).iter().any(|&(_, _, held)| held);
    assert!(!held, "a dream was prepared during the purge");
    realm.host.shutdown().await.unwrap();
}

/// The dream's folders stay checked out until both purges are
/// answered: a purge that waited on them would not be answered at
/// all.
#[tokio::test(flavor = "multi_thread")]
async fn a_purge_does_not_wait_on_a_dreams_preparation() {
    let (realm, _) = hamlet_with(0).await;
    create(&realm, Hand::NUMBER, "c1", "Mason", None).await;
    create(&realm, Hand::NUMBER, "c2", "Smith", None).await;
    realm.seen.hold(Callee::PrepareDream);
    let (began, (dreamt, other)) =
        tokio::join!(chief(&realm, of_profile(3)), async {
            until(
                "the dream's folders are being checked out",
                || {
                    realm.seen.is_waiting(
                        Callee::PrepareDream,
                        AgentNumber(3),
                    )
                },
            )
            .await;
            let dreamt =
                asked(&realm, &RequestId::new(), purge(3, "a.md"))
                    .await;
            let other =
                asked(&realm, &RequestId::new(), purge(4, "a.md"))
                    .await;
            realm.seen.release(Callee::PrepareDream);
            (dreamt, other)
        });

    assert!(matches!(began, Dreamt::Began { .. }));

    assert_eq!(
        dreamt,
        PurgeAnswer::refused(
            DreamOpen::Dreaming { dream: DreamId(1) }.into()
        )
    );
    assert!(matches!(other, PurgeAnswer::Purged { .. }));
    let purged = purges(&realm);
    assert_eq!(
        purged
            .iter()
            .map(|(profile, _)| *profile)
            .collect::<Vec<_>>(),
        [ProfileId(4)]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dream_is_not_prepared_again_after_a_restart() {
    let (realm, _) = hamlet_with(0).await;
    realm.seen.nothing_to_dream.store(true, Ordering::SeqCst);
    create(&realm, Hand::NUMBER, "c1", "Mason", None).await;
    realm.seen.hold(Callee::EndDream);
    assert_eq!(chief(&realm, of_profile(3)).await, Dreamt::Nothing);
    until("the dream is to end", || {
        realm.seen.is_waiting(Callee::EndDream, AgentNumber(3))
    })
    .await;

    let realm = realm.restart().await;
    realm.seen.release(Callee::EndDream);
    until("the dream ends", || !dream_ends(&realm).is_empty()).await;

    assert_eq!(prepares(&realm).len(), 1);
    realm.host.shutdown().await.unwrap();
}
