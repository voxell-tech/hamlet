use std::sync::Arc;

use hamlet_actors::realm;
use hamlet_messages::agent::Input;
use hamlet_messages::realm::choices::{
    ModelOf, SetModel, SetNickname, SetTools,
};
use hamlet_messages::realm::hamlets::{
    EraseHamlet, NameHamlet, ReadRecord, Record, RecordLine,
    RemoveHamlet, SetMode,
};
use hamlet_messages::realm::profiles::EraseProfile;
use hamlet_messages::realm::{
    Done, DoneTo, HamletId, HamletRefusal, ToolName,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;
use hamlet_messages::workspace::reach::Mode;
use hamlet_protocol::client::RequestId;

use super::chiefs::chief;
use super::lifecycle::{MASON, archive, masons_hamlet};
use super::stand_ins::Seen;
use super::{
    FOUR, MAYOR, Realm, THREE, done_to, founded, hamlet_with,
    named_as, say, seen_submits, to, tool,
};

fn read(lines: &[&str]) -> Record {
    Record::Read {
        lines: lines
            .iter()
            .map(|&text| RecordLine {
                text: text.to_owned(),
            })
            .collect(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_history_lists_the_decisions_with_the_mayors_hire_before_the_founding()
 {
    let (realm, hamlet) = hamlet_with(1).await;
    let some = |why: &str| Some(why.to_owned());
    assert_eq!(
        chief(
            &realm,
            NameHamlet {
                hamlet: hamlet.clone(),
                name: "Weir".to_owned(),
            },
        )
        .await,
        Done::Done
    );
    assert_eq!(
        chief(
            &realm,
            SetMode {
                hamlet: hamlet.clone(),
                mode: Mode::Auto,
            },
        )
        .await,
        Done::Done
    );
    assert_eq!(
        realm
            .tool(
                MAYOR,
                &tool(
                    "n1",
                    SetNickname {
                        hire: to(THREE),
                        name: some("Spade"),
                        why: some("short"),
                    },
                ),
            )
            .await
            .unwrap(),
        done_to(THREE)
    );
    assert_eq!(
        realm
            .tool(
                MAYOR,
                &tool(
                    "t1",
                    SetTools {
                        hire: to(THREE),
                        off: vec![ToolName {
                            name: "bash".to_owned(),
                        }],
                        on: Vec::new(),
                        why: some("quiet"),
                    },
                ),
            )
            .await
            .unwrap(),
        DoneTo::Done {
            agent: named_as(3, "hire#3 (Spade)")
        }
    );
    assert_eq!(
        chief(
            &realm,
            SetModel {
                of: ModelOf::Hire { hire: to(THREE) },
                model: some("faux::faux-1"),
                effort: None,
                fast: None,
                why: some("same"),
            },
        )
        .await,
        Done::Done
    );

    let by_chief = chief(
        &realm,
        ReadRecord {
            hamlet: Some(hamlet.clone()),
        },
    )
    .await;
    let by_hand = realm
        .tool(
            Hand::NUMBER,
            &tool(
                "r1",
                ReadRecord {
                    hamlet: Some(hamlet.clone()),
                },
            ),
        )
        .await
        .unwrap();
    let realms = chief(&realm, ReadRecord { hamlet: None }).await;
    let nowhere = chief(
        &realm,
        ReadRecord {
            hamlet: Some(HamletId("x".to_owned())),
        },
    )
    .await;

    let lines = read(&[
        "- chief hired hire#2 (Mayor)",
        "- chief founded the hamlet under hire#2 (Mayor): Mayor of \
         Mill.",
        "- hire#2 (Mayor) hired hire#3 (Spade)",
        "- chief changed ",
        "- chief changed ",
        "- hire#2 (Mayor) nicknamed hire#3 (Spade): short",
        "- hire#2 (Mayor) changed the tools of hire#3 (Spade): quiet",
        "- chief chose the model of hire#3 (Spade): same",
    ]);
    assert_eq!(by_chief, lines);
    assert_eq!(by_hand, lines);
    assert_eq!(realms, read(&["Nothing is decided yet."]));
    assert_eq!(
        nowhere,
        Record::Refused {
            why: HamletRefusal::NoSuchHamlet {
                id: HamletId("x".to_owned())
            }
        }
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_record_and_a_line_name_the_chief_as_its_server_was_started()
 {
    let named = |chief: &str| realm::Options {
        chief: chief.to_owned(),
        ..realm::Options::default()
    };
    let realm =
        Realm::start_with(Some(named("ada")), Seen::default()).await;
    realm.live(Hand::NUMBER).await;
    let hamlet = founded(&realm).await;
    realm.live(MAYOR).await;
    let realm = Realm {
        options: Some(Arc::new(named("bo"))),
        ..realm
    }
    .restart()
    .await;
    realm.live(MAYOR).await;

    let renamed = chief(
        &realm,
        NameHamlet {
            hamlet: hamlet.clone(),
            name: "Weir".to_owned(),
        },
    )
    .await;
    realm
        .ask(&say(&RequestId::new(), MAYOR, "build the mill"))
        .await
        .unwrap();
    let record = chief(
        &realm,
        ReadRecord {
            hamlet: Some(hamlet),
        },
    )
    .await;
    let said = seen_submits(&realm, MAYOR, 1).await;

    assert_eq!(renamed, Done::Done);
    assert_eq!(
        record,
        read(&[
            "- ada hired hire#2 (Mayor)",
            "- ada founded the hamlet under hire#2 (Mayor): Mayor \
             of Mill.",
            "- bo changed ",
        ])
    );
    let chiefs = said
        .iter()
        .filter_map(|submit| match &submit.input {
            Input::Line { chief, .. } => Some(chief.as_deref()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(chiefs, [Some("bo")]);
    realm.host.shutdown().await.unwrap();
}

/// The kind and subject of each of the realm's decisions after the
/// first `from`.
async fn kept_after(
    realm: &Realm,
    from: usize,
) -> Vec<(String, Option<u32>)> {
    let kept = realm
        .stopped_rows(|db| {
            db.prepare(
                "SELECT kind, agent FROM decision ORDER BY id",
            )?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()
        })
        .await;
    kept[from..].to_vec()
}

fn erased(agent: AgentNumber) -> (String, Option<u32>) {
    ("erased".to_owned(), Some(agent.0))
}

#[tokio::test(flavor = "multi_thread")]
async fn erasing_a_hamlet_or_a_profile_keeps_an_erased_entry_for_each_hire()
 {
    let (realm, hamlet) = hamlet_with(2).await;
    chief(
        &realm,
        RemoveHamlet {
            hamlet: hamlet.clone(),
            why: None,
        },
    )
    .await;
    let before = super::decisions(&realm).await.len();
    chief(&realm, EraseHamlet { hamlet }).await;
    assert_eq!(
        kept_after(&realm, before).await,
        [
            erased(MAYOR),
            erased(THREE),
            erased(FOUR),
            ("erased-hamlet".to_owned(), None),
        ]
    );
    realm.host.shutdown().await.unwrap();

    let (realm, _) = masons_hamlet(2).await;
    chief(&realm, archive(MASON)).await;
    let before = super::decisions(&realm).await.len();
    chief(&realm, EraseProfile { profile: MASON }).await;
    assert_eq!(
        kept_after(&realm, before).await,
        [
            erased(THREE),
            erased(FOUR),
            ("profile-erased".to_owned(), None),
        ]
    );
    realm.host.shutdown().await.unwrap();
}
