use hamlet_messages::library::DropOfferParts;
use hamlet_messages::profile::DropSaveParts;
use hamlet_messages::realm::{Done, End, Named, ProfileId};

use super::{Callee, MAYOR, Realm, THREE, hamlet_with, tool, until};

fn dropped(
    realm: &Realm,
) -> (Vec<(ProfileId, DropSaveParts)>, Vec<DropOfferParts>) {
    (
        realm.seen.save_drops.lock().unwrap().clone(),
        realm.seen.offer_drops.lock().unwrap().clone(),
    )
}

/// The profile fails the first drop as an actor not ready; the realm
/// calls it again a second later.
#[tokio::test(flavor = "multi_thread")]
async fn an_ended_hires_parts_are_dropped_by_its_profile_and_the_library()
 {
    let (realm, _) = hamlet_with(1).await;
    realm.seen.fail_once(Callee::DropSaveParts, THREE);

    let ended = realm
        .tool(
            MAYOR,
            &tool(
                "e1",
                End {
                    hire: Named::Agent { agent: THREE },
                    why: None,
                },
            ),
        )
        .await
        .unwrap();

    assert_eq!(ended, Done::Done);
    until("both drops", || {
        let (saves, offers) = dropped(&realm);
        !saves.is_empty() && !offers.is_empty()
    })
    .await;
    assert_eq!(
        dropped(&realm),
        (
            vec![(ProfileId(3), DropSaveParts { agent: THREE })],
            vec![DropOfferParts { agent: THREE }],
        )
    );
    realm.host.shutdown().await.unwrap();
}
