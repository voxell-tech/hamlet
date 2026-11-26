//! A profile a lead makes: where it arrives, and the wait that asks a
//! chief to let it in.

use hamlet_messages::realm::client::ReadState;
use hamlet_messages::realm::documents::{
    EditSettings, Newcomers, Setting,
};
use hamlet_messages::realm::profiles::{TurnAway, Welcome};
use hamlet_messages::realm::{Done, ProfileId};
use hamlet_messages::sender::Hand;
use hamlet_protocol::client::{self as protocol, Choice, WaitDetail};

use super::chiefs::chief;
use super::profiles::create;
use super::{MAYOR, Realm, answer, hamlet_with};

async fn arrivals(realm: &Realm) -> Vec<(u32, String)> {
    realm
        .stopped_rows(|db| {
            db.prepare(
                "SELECT id, arrival FROM profile WHERE id >= 3 \
                 ORDER BY id",
            )?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<Vec<(u32, String)>>>()
        })
        .await
}

async fn waits(realm: &Realm) -> Vec<protocol::Wait> {
    let state: protocol::State = realm.ask(&ReadState).await.unwrap();
    state.waits
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profile_a_lead_makes_waits_at_the_harbor_unless_the_hand_makes_it_or_the_city_is_open()
 {
    let (realm, _) = hamlet_with(0).await;
    create(&realm, MAYOR, "c1", "Mason", None).await;
    create(&realm, Hand::NUMBER, "c2", "Digger", None).await;
    let open = EditSettings {
        set: vec![Setting::Newcomers {
            newcomers: Newcomers::Open,
        }],
    };
    assert_eq!(chief(&realm, open).await, Done::Done);
    create(&realm, MAYOR, "c3", "Smith", None).await;

    assert_eq!(
        arrivals(&realm).await,
        [
            (3, "harbor".to_owned()),
            (4, "housed".to_owned()),
            (5, "housed".to_owned())
        ]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profile_at_the_harbor_waits_for_a_chief_until_let_in_or_turned_away()
 {
    let (realm, _) = hamlet_with(0).await;
    for (call, name) in
        [("c1", "Newbie"), ("c2", "Mason"), ("c3", "Smith")]
    {
        create(&realm, MAYOR, call, name, None).await;
    }

    let asked = waits(&realm).await;
    let newbie = &asked[0];
    assert_eq!(asked.len(), 3);
    assert_eq!(newbie.about, "scholar#3 (Newbie)");
    assert_eq!(newbie.by, format!("hire#{}", MAYOR.0));
    assert_eq!(newbie.answers, [Choice::Welcome, Choice::Dismiss]);
    assert_eq!(
        newbie.detail,
        WaitDetail::Newcomer {
            scholar: protocol::ScholarId(3)
        }
    );
    let welcomed = realm
        .ask(&answer(&newbie.id, Choice::Welcome, Some("come in")))
        .await
        .unwrap();
    assert_eq!(welcomed, Done::Done);
    let dismissed = realm
        .ask(&answer(&asked[1].id, Choice::Dismiss, None))
        .await
        .unwrap();
    assert_eq!(dismissed, Done::Done);
    chief(
        &realm,
        Welcome {
            profile: ProfileId(5),
            why: None,
        },
    )
    .await;
    assert!(waits(&realm).await.is_empty());
    chief(
        &realm,
        TurnAway {
            profile: ProfileId(4),
            why: None,
        },
    )
    .await;

    assert_eq!(
        arrivals(&realm).await,
        [
            (3, "housed".to_owned()),
            (4, "turned".to_owned()),
            (5, "housed".to_owned())
        ]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn no_chief_is_asked_of_a_profile_the_hand_makes_or_one_the_hand_judges()
 {
    let (realm, _) = hamlet_with(0).await;
    create(&realm, Hand::NUMBER, "c1", "Digger", None).await;
    let to_the_hand = EditSettings {
        set: vec![Setting::Newcomers {
            newcomers: Newcomers::Hand,
        }],
    };
    assert_eq!(chief(&realm, to_the_hand).await, Done::Done);
    create(&realm, MAYOR, "c2", "Mason", None).await;

    assert!(waits(&realm).await.is_empty());
    assert_eq!(
        arrivals(&realm).await,
        [(3, "housed".to_owned()), (4, "harbor".to_owned())]
    );
    realm.host.shutdown().await.unwrap();
}
