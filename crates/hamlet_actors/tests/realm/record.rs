use hamlet_messages::realm::choices::{
    ModelOf, SetModel, SetNickname,
};
use hamlet_messages::realm::hamlets::{
    NameHamlet, ReadRecord, Record, RecordLine, SetMode,
};
use hamlet_messages::realm::{
    Done, HamletId, HamletRefusal, SetTools, ToolName,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::reach::Mode;

use super::chiefs::chief;
use super::{MAYOR, THREE, hamlet_with, to, tool};

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
async fn the_history_lists_the_decisions() {
    // found with hire 3, rename, set the mode, nickname and the tools
    // of hire 3, choose its model: the chief and the Hand read the
    // hamlet's lines in today's words, oldest first and with hire 3's
    // nickname now; the realm's own record is empty.
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
        Done::Done
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
        Done::Done
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
        "- chief founded the hamlet under hire#2",
        "- hire#2 hired hire#3 (Spade): Check the links",
        "- chief changed ",
        "- chief changed ",
        "- hire#2 nicknamed hire#3 (Spade): short",
        "- hire#2 changed the tools of hire#3 (Spade): quiet",
        "- chief chose the model of hire#3 (Spade): same",
    ]);
    assert_eq!(by_chief, lines);
    assert_eq!(by_hand, lines);
    assert_eq!(realms, read(&[]));
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
