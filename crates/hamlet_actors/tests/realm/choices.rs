use hamlet_messages::realm::choices::{
    ChoiceRefusal, ModelOf, SetModel, SetNickname,
};
use hamlet_messages::realm::{
    Done, End, HowEnded, LeadRefusal, Left, Named, OnlyAHire, Rank,
    SetTools,
};
use hamlet_protocol::Effort;

use super::chiefs::chief;
use super::{FOUR, MAYOR, Realm, THREE, hamlet_with, to, tool};

fn nickname(name: Option<&str>, why: Option<&str>) -> SetNickname {
    SetNickname {
        hire: to(THREE),
        name: name.map(str::to_owned),
        why: why.map(str::to_owned),
    }
}

fn model_of(of: ModelOf) -> SetModel {
    SetModel {
        of,
        model: None,
        effort: None,
        fast: None,
        why: None,
    }
}

fn of_three() -> ModelOf {
    ModelOf::Hire { hire: to(THREE) }
}

async fn nicknames(realm: &Realm) -> Vec<(u32, String)> {
    realm
        .rows(|db| {
            db.prepare(
                "SELECT agent, name FROM nickname ORDER BY agent",
            )?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect()
        })
        .await
}

type Kept = (String, Option<String>, Option<String>, Option<bool>);

async fn model_choices(realm: &Realm) -> Vec<Kept> {
    realm
        .rows(|db| {
            db.prepare(
                "SELECT of, model, effort, fast FROM model_choice \
                 ORDER BY of",
            )?
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                ))
            })?
            .collect()
        })
        .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_nickname_is_set_and_cleared() {
    // the mayor calls hire 3 " Spade "; 25 letters are refused, and
    // hire 3 cannot name its lead. The chief clears it.
    let (realm, _) = hamlet_with(1).await;

    let set = realm
        .tool(
            MAYOR,
            &tool("n1", nickname(Some(" Spade "), Some("short"))),
        )
        .await
        .unwrap();
    let long = realm
        .tool(
            MAYOR,
            &tool("n2", nickname(Some(&"a".repeat(25)), None)),
        )
        .await
        .unwrap();
    let upward = realm
        .tool(
            THREE,
            &tool(
                "n3",
                SetNickname {
                    hire: to(MAYOR),
                    ..nickname(Some("Boss"), None)
                },
            ),
        )
        .await
        .unwrap();
    let after_set = nicknames(&realm).await;
    let cleared = chief(&realm, nickname(None, None)).await;

    assert_eq!(set, Done::Done);
    assert_eq!(
        long,
        Done::Refused {
            why: LeadRefusal::Act(ChoiceRefusal::BadNickname)
        }
    );
    assert_eq!(
        upward,
        Done::Refused {
            why: LeadRefusal::Rank(Rank::NotALead { hire: MAYOR })
        }
    );
    assert_eq!(after_set, [(3, "Spade".to_owned())]);
    assert_eq!(cleared, Done::Done);
    assert_eq!(nicknames(&realm).await, []);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_model_choice_is_kept() {
    // the chief picks hire 3's model and effort; the mayor may not
    // change it, nor a hamlet's. A choice is whole: the chief's
    // fast-only choice leaves no model, which the mayor may then
    // choose. The chief may choose the Hand's.
    let (realm, hamlet) = hamlet_with(1).await;
    let mayor = async |call: &str, act: SetModel| {
        realm.tool(MAYOR, &tool(call, act)).await.unwrap()
    };

    let picked = chief(
        &realm,
        SetModel {
            model: Some("faux::faux-1".to_owned()),
            effort: Some(Effort::XHigh),
            ..model_of(of_three())
        },
    )
    .await;
    let after_chief = model_choices(&realm).await;
    let overruled = mayor(
        "m1",
        SetModel {
            model: Some("faux::faux-2".to_owned()),
            ..model_of(of_three())
        },
    )
    .await;
    let of_hamlet = mayor(
        "m2",
        model_of(ModelOf::Hamlet {
            hamlet: hamlet.clone(),
        }),
    )
    .await;
    let fast_only = chief(
        &realm,
        SetModel {
            fast: Some(true),
            ..model_of(of_three())
        },
    )
    .await;
    let after_fast = model_choices(&realm).await;
    let by_the_mayor = mayor(
        "m3",
        SetModel {
            model: Some("faux::faux-2".to_owned()),
            ..model_of(of_three())
        },
    )
    .await;
    let the_hands = chief(
        &realm,
        SetModel {
            model: Some("faux::faux-1".to_owned()),
            ..model_of(ModelOf::Hire { hire: Named::Hand })
        },
    )
    .await;

    let three = "agent/3".to_owned();
    assert_eq!(picked, Done::Done);
    assert_eq!(
        after_chief,
        [(
            three.clone(),
            Some("faux::faux-1".to_owned()),
            Some("xhigh".to_owned()),
            None
        )]
    );
    assert_eq!(
        overruled,
        Done::Refused {
            why: LeadRefusal::Act(ChoiceRefusal::ChiefChose {
                hire: THREE
            })
        }
    );
    assert_eq!(
        of_hamlet,
        Done::Refused {
            why: LeadRefusal::Act(ChoiceRefusal::HamletFromAnAgent)
        }
    );
    assert_eq!(fast_only, Done::Done);
    assert_eq!(after_fast, [(three.clone(), None, None, Some(true))]);
    assert_eq!((by_the_mayor, the_hands), (Done::Done, Done::Done));
    assert_eq!(
        model_choices(&realm).await,
        [
            (
                "agent/1".to_owned(),
                Some("faux::faux-1".to_owned()),
                None,
                None
            ),
            (three, Some("faux::faux-2".to_owned()), None, None),
        ]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_choice_for_a_hire_that_left_is_refused_as_left() {
    // the mayor ends hire 4, then sets its tools, its nickname and
    // its model: each is refused naming how it left.
    let (realm, _) = hamlet_with(2).await;
    let ended = realm
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
    assert_eq!(ended, Done::Done);

    let tools = realm
        .tool(
            MAYOR,
            &tool(
                "t1",
                SetTools {
                    hire: to(FOUR),
                    off: Vec::new(),
                    on: Vec::new(),
                    why: None,
                },
            ),
        )
        .await
        .unwrap();
    let named = realm
        .tool(
            MAYOR,
            &tool(
                "n1",
                SetNickname {
                    hire: to(FOUR),
                    ..nickname(Some("Gone"), None)
                },
            ),
        )
        .await
        .unwrap();
    let model = realm
        .tool(
            MAYOR,
            &tool("m1", model_of(ModelOf::Hire { hire: to(FOUR) })),
        )
        .await
        .unwrap();

    let left = Left {
        how: HowEnded::ByALead { lead: MAYOR },
        why: None,
    };
    assert_eq!(
        tools,
        Done::Refused {
            why: LeadRefusal::Act(OnlyAHire::Left {
                hire: FOUR,
                left: left.clone()
            })
        }
    );
    let refused = Done::Refused {
        why: LeadRefusal::Act(ChoiceRefusal::Left {
            hire: FOUR,
            left,
        }),
    };
    assert_eq!((named, model), (refused.clone(), refused));
    realm.host.shutdown().await.unwrap();
}
