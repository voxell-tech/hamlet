use hamlet_messages::realm::choices::{
    ChoiceRefusal, ModelOf, SetModel, SetNickname,
};
use hamlet_messages::realm::hamlets::{ReadRecord, Record};
use hamlet_messages::realm::workers::StartWorker;
use hamlet_messages::realm::{
    Done, DoneTo, LeadRefusal, Named, Rank,
};
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::Effort;

use super::chiefs::chief;
use super::{
    FOUR, MAYOR, Realm, THREE, done_to, hamlet_with, shown, to, tool,
    words,
};

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
        .stopped_rows(|db| {
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
        .stopped_rows(|db| {
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

    assert_eq!(set, done_to(THREE));
    assert_eq!(
        long,
        DoneTo::Refused {
            why: LeadRefusal::Act(ChoiceRefusal::BadNickname)
        }
    );
    assert_eq!(
        upward,
        DoneTo::Refused {
            why: LeadRefusal::Rank(Rank::NotALead {
                hire: shown(MAYOR)
            })
        }
    );
    assert_eq!(after_set, [(3, "Spade".to_owned())]);
    assert_eq!(cleared, Done::Done);
    assert_eq!(nicknames(&realm).await, []);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_model_choice_is_kept() {
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

    let three = "agent/3".to_owned();
    assert_eq!(picked, Done::Done);
    assert_eq!(
        after_chief,
        [(
            three,
            Some("faux::faux-1".to_owned()),
            Some("xhigh".to_owned()),
            None
        )]
    );
    assert_eq!(
        overruled,
        DoneTo::Refused {
            why: LeadRefusal::Act(ChoiceRefusal::ChiefChose {
                hire: shown(THREE)
            })
        }
    );
    assert_eq!(
        of_hamlet,
        DoneTo::Refused {
            why: LeadRefusal::Act(ChoiceRefusal::HamletFromAnAgent)
        }
    );
    assert_eq!(model_choices(&realm).await, after_chief);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_hand_takes_no_nickname_and_a_chief_sets_its_model() {
    let (realm, _) = hamlet_with(0).await;

    let nicknamed = chief(
        &realm,
        SetNickname {
            hire: Named::Hand,
            ..nickname(Some("Boss"), None)
        },
    )
    .await;
    let modelled = chief(
        &realm,
        SetModel {
            model: Some("faux::faux-1".to_owned()),
            ..model_of(ModelOf::Hire { hire: Named::Hand })
        },
    )
    .await;

    let not_a_hire = Done::Refused {
        why: ChoiceRefusal::NotAHire {
            name: "hand".to_owned(),
        },
    };
    assert_eq!((&nicknamed, &modelled), (&not_a_hire, &Done::Done));
    assert_eq!(words(nicknamed), "`hand` is not a hire");
    assert_eq!(nicknames(&realm).await, Vec::new());
    assert_eq!(
        model_choices(&realm).await,
        [(
            "agent/1".to_owned(),
            Some("faux::faux-1".to_owned()),
            None,
            None
        )]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chiefs_choice_is_whole_and_a_leads_sets_only_the_model() {
    let (realm, _) = hamlet_with(1).await;
    let mayor = async |call: &str, model: Option<&str>| {
        let act = SetModel {
            model: model.map(str::to_owned),
            ..model_of(of_three())
        };
        realm.tool(MAYOR, &tool(call, act)).await.unwrap()
    };
    let kept = |model: Option<&str>, effort: Option<&str>, fast| {
        vec![(
            "agent/3".to_owned(),
            model.map(str::to_owned),
            effort.map(str::to_owned),
            fast,
        )]
    };

    let thinks = chief(
        &realm,
        SetModel {
            effort: Some(Effort::XHigh),
            ..model_of(of_three())
        },
    )
    .await;
    let thinking = model_choices(&realm).await;
    let by_lead = mayor("m1", Some("faux::faux-2")).await;
    let led = model_choices(&realm).await;
    let unset_by_lead = mayor("m2", None).await;
    let lead_cleared = model_choices(&realm).await;
    let whole = chief(
        &realm,
        SetModel {
            model: Some("faux::faux-2".to_owned()),
            effort: Some(Effort::XHigh),
            fast: Some(true),
            ..model_of(of_three())
        },
    )
    .await;
    let set = model_choices(&realm).await;
    let cleared = chief(&realm, model_of(of_three())).await;

    assert_eq!(
        (thinks, by_lead, unset_by_lead, whole, cleared),
        (
            Done::Done,
            done_to(THREE),
            done_to(THREE),
            Done::Done,
            Done::Done
        )
    );
    assert_eq!(thinking, kept(None, Some("xhigh"), None));
    assert_eq!(led, kept(Some("faux::faux-2"), Some("xhigh"), None));
    assert_eq!(lead_cleared, kept(None, Some("xhigh"), None));
    assert_eq!(
        set,
        kept(Some("faux::faux-2"), Some("xhigh"), Some(true))
    );
    assert_eq!(model_choices(&realm).await, kept(None, None, None));
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_leads_choice_changes_only_the_model() {
    let (realm, hamlet) = hamlet_with(1).await;
    let mayor = async |call: &str, act: SetModel| {
        realm.tool(MAYOR, &tool(call, act)).await.unwrap()
    };
    let lines = async || {
        let Record::Read { lines } = chief(
            &realm,
            ReadRecord {
                hamlet: Some(hamlet.clone()),
            },
        )
        .await
        else {
            panic!("the hamlet's record is read");
        };
        lines.into_iter().map(|line| line.text).collect::<Vec<_>>()
    };
    let before = lines().await;

    let done = (
        mayor(
            "m1",
            SetModel {
                effort: Some(Effort::XHigh),
                fast: Some(true),
                ..model_of(of_three())
            },
        )
        .await,
        chief(
            &realm,
            SetModel {
                effort: Some(Effort::Low),
                ..model_of(ModelOf::Hamlet {
                    hamlet: hamlet.clone(),
                })
            },
        )
        .await,
        chief(
            &realm,
            SetModel {
                model: Some("faux::faux-1".to_owned()),
                effort: Some(Effort::Low),
                ..model_of(ModelOf::Hamlet {
                    hamlet: hamlet.clone(),
                })
            },
        )
        .await,
    );
    let quiet = lines().await;
    let modelled = mayor(
        "m2",
        SetModel {
            model: Some("faux::faux-2".to_owned()),
            effort: Some(Effort::XHigh),
            why: Some("cheap".to_owned()),
            ..model_of(of_three())
        },
    )
    .await;

    assert_eq!(done, (done_to(THREE), Done::Done, Done::Done));
    assert_eq!(modelled, done_to(THREE));
    assert_eq!(quiet, before);
    assert_eq!(
        lines().await[before.len()..],
        ["- hire#2 (Mayor) chose the model of hire#3 (Hire 1): \
          cheap"]
    );
    assert_eq!(
        model_choices(&realm).await,
        [
            (
                "agent/3".to_owned(),
                Some("faux::faux-2".to_owned()),
                None,
                None
            ),
            (
                format!("hamlet/{}", hamlet.0),
                Some("faux::faux-1".to_owned()),
                Some("low".to_owned()),
                None
            ),
        ]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_choice_for_no_agent_names_its_address() {
    let (realm, _) = hamlet_with(0).await;
    let nicknamed = chief(
        &realm,
        SetNickname {
            hire: to(AgentNumber(9)),
            name: Some("Spade".to_owned()),
            why: None,
        },
    )
    .await;

    assert_eq!(words(nicknamed), "no agent has the address `hire#9`");
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chief_sets_a_workers_model_and_its_lead_may_not() {
    let (realm, _) = hamlet_with(1).await;
    let start = StartWorker {
        brief: "count the files".to_owned(),
        tools: Vec::new(),
        task: None,
        hamlet: None,
    };
    realm.tool(THREE, &tool("w1", start)).await.unwrap();
    let of_four = || SetModel {
        model: Some("faux::faux-1".to_owned()),
        ..model_of(ModelOf::Hire {
            hire: Named::Worker { worker: FOUR },
        })
    };

    let by_the_chief = chief(&realm, of_four()).await;
    let by_its_lead =
        realm.tool(THREE, &tool("m1", of_four())).await.unwrap();

    assert_eq!(by_the_chief, Done::Done);
    assert_eq!(
        by_its_lead,
        DoneTo::Refused {
            why: LeadRefusal::Act(ChoiceRefusal::NotAHire {
                name: "worker#4".to_owned()
            })
        }
    );
    assert_eq!(words(by_its_lead), "`worker#4` is not a hire");
    assert_eq!(
        model_choices(&realm).await,
        [(
            "agent/4".to_owned(),
            Some("faux::faux-1".to_owned()),
            None,
            None
        )]
    );
    realm.host.shutdown().await.unwrap();
}
