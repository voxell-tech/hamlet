use std::time::Duration;

use hamlet_messages::profile::{BadPath, By, Purge, PurgeAnswer};
use hamlet_messages::realm::ProfileId;
use hamlet_messages::realm::profiles::{
    ProfileRefusal, PurgeProfile, PurgeProfileRefusal,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::RequestId;

use super::profiles::{asked, create};
use super::{
    Callee, NOT_A_PATH, Realm, STILL_HOLDS, hamlet_with, until,
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
/// realm waits 8 s, so the answer has 3.5 s to spare, many times what
/// a relay takes on a busy machine.
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
