use hamlet_messages::agent::copies::{Copies, CopyVersion, Price};
use hamlet_messages::realm::budget::{BudgetLimit, Payer};
use hamlet_messages::realm::documents::{
    CardSetting, CatalogueRefusal, EditCatalogue, EditCharter,
    EditSettings, Endpoint, ReadCharter, ReadCopies, Setting,
    SettingRefusal,
};
use hamlet_messages::realm::world::ReachSeen;
use hamlet_messages::realm::{
    Asked, ChiefRequest, Done, HamletRefusal, Limit, Refusable,
    SetLimits,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;
use hamlet_messages::workspace::reach::Reach;
use hamlet_protocol::client::RequestId;

use super::world::seen_until;
use super::{
    Callee, MAYOR, Realm, copies_to, hamlet_with, until, words,
};

const FAUX: &str = "faux::faux-1";

/// A realm with the Hand live.
async fn realm_with_hand() -> Realm {
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;
    realm
}

async fn chief<T: ChiefRequest>(realm: &Realm, act: T) -> T::Answer {
    realm
        .ask(&Asked {
            id: RequestId::new(),
            act,
        })
        .await
        .unwrap()
}

async fn read_copies(realm: &Realm, have: &ReadCopies) -> Copies {
    realm.tool(Hand::NUMBER, have).await.unwrap()
}

fn charter(text: &str) -> EditCharter {
    EditCharter {
        text: Some(text.to_owned()),
    }
}

fn settings(set: Setting) -> EditSettings {
    EditSettings { set: vec![set] }
}

fn price(input: f64) -> Price {
    Price {
        input,
        output: 0.34,
        cache_read: 0.12,
        cache_write: 0.12,
    }
}

fn priced(model: &str, price: Option<Price>) -> EditCatalogue {
    EditCatalogue {
        model: model.to_owned(),
        set: vec![CardSetting::Price { price }],
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_charter_edit_pushes_a_copy() {
    let realm = realm_with_hand().await;

    let edited = chief(&realm, charter("Be brief.")).await;

    assert_eq!(edited, Done::Done);
    until("the Hand is pushed the new charter", || {
        copies_to(&realm, Hand::NUMBER).iter().any(|copies| {
            copies
                .charter
                .as_ref()
                .is_some_and(|charter| charter.value == "Be brief.")
        })
    })
    .await;
    let read = realm.ask(&ReadCharter).await.unwrap();
    assert!(read.own && read.text == "Be brief.", "{read:?}");
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_blank_charter_gives_back_the_built_in_one() {
    let realm = realm_with_hand().await;
    chief(&realm, charter("Be brief.")).await;

    let edited = chief(&realm, charter("  ")).await;

    assert_eq!(edited, Done::Done);
    let read = realm.ask(&ReadCharter).await.unwrap();
    assert!(!read.own && read.text.contains("`bash`"), "{read:?}");
    let copies = read_copies(&realm, &ReadCopies::default()).await;
    assert_eq!(
        copies.charter.map(|charter| charter.value),
        Some(read.text)
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn read_copies_gives_only_newer_parts() {
    // the Hand holds every part as first read; one settings edit
    // moves only the settings part.
    let realm = realm_with_hand().await;
    chief(&realm, settings(Setting::Model { model: FAUX.into() }))
        .await;
    let first = read_copies(&realm, &ReadCopies::default()).await;
    let held = ReadCopies {
        charter: first.charter.as_ref().map(|part| part.version),
        character: first.character.as_ref().map(|part| part.version),
        model: first.model.as_ref().map(|part| part.version),
        settings: first.settings.as_ref().map(|part| part.version),
    };

    let same = read_copies(&realm, &held).await;
    chief(&realm, settings(Setting::ReflectEvery { turns: 4 })).await;
    let after = read_copies(&realm, &held).await;

    assert!(
        first
            .character
            .unwrap()
            .value
            .starts_with("You are a scholar")
    );
    assert_eq!(first.model.unwrap().value.model, FAUX);
    assert_eq!(same, Copies::default());
    let moved = after.settings.clone().expect("the settings moved");
    assert_eq!(
        after,
        Copies {
            settings: Some(moved.clone()),
            ..Copies::default()
        }
    );
    assert_eq!(moved.value.reflect_every, 4);
    assert!(moved.version > held.settings.unwrap_or(CopyVersion(0)));
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_misshapen_limit_changes_nothing() {
    // a cost of $1 comes before a limit the other kind of row has, or
    // a road price past what the realm counts, so each whole edit is
    // refused and both budgets stay $50.
    let (realm, hamlet) = hamlet_with(0).await;
    let budget_of = async |payer: &Payer| {
        let seen =
            seen_until(&realm, "the budgets are shown", |seen| {
                seen.spend.iter().any(|spend| spend.payer == *payer)
            })
            .await;
        seen.spend
            .into_iter()
            .find(|spend| spend.payer == *payer)
            .and_then(|spend| spend.budget_micros)
    };
    let hamlets = Payer::Hamlet {
        hamlet: hamlet.clone(),
    };
    let limits = |hamlet, limit| SetLimits {
        hamlet,
        budget: vec![BudgetLimit::Cost { micros: 1_000_000 }],
        limits: vec![limit],
    };

    let on_hamlet = chief(
        &realm,
        limits(
            Some(hamlet.clone()),
            Limit::RoadsBetween { allowed: false },
        ),
    )
    .await;
    let on_realm =
        chief(&realm, limits(None, Limit::ToldDays { days: 2 }))
            .await;
    let past_a_price = chief(
        &realm,
        limits(
            Some(hamlet.clone()),
            Limit::RoadPrice { micros: u64::MAX },
        ),
    )
    .await;

    let misshapen = Done::refused(HamletRefusal::MisshapenLimits);
    assert_eq!(
        [&on_hamlet, &on_realm, &past_a_price],
        [&misshapen, &misshapen, &misshapen]
    );
    assert_eq!(
        words(on_hamlet),
        "Those limits do not fit their form."
    );
    assert_eq!(budget_of(&hamlets).await, Some(50_000_000));
    assert_eq!(budget_of(&Payer::Realm).await, Some(50_000_000));
    let set = chief(
        &realm,
        limits(Some(hamlet), Limit::ToldDays { days: 2 }),
    )
    .await;
    assert_eq!(set, Done::Done);
    assert_eq!(budget_of(&hamlets).await, Some(1_000_000));
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn allow_net_raises_every_reach() {
    let (realm, _) = hamlet_with(1).await;
    let living = [Hand::NUMBER, AgentNumber(2), AgentNumber(3)];

    let edited =
        chief(&realm, settings(Setting::AllowNet { allow: true }))
            .await;

    assert_eq!(edited, Done::Done);
    until("every living agent may reach the network", || {
        let reaches = realm.seen.reaches.lock().unwrap();
        living.iter().all(|&agent| {
            reaches.iter().any(|set| {
                set.agent == agent
                    && matches!(
                        &set.reach,
                        Reach::Agent(footing) if footing.network
                    )
            })
        })
    })
    .await;
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_price_change_pushes_the_model_copy() {
    let realm = realm_with_hand().await;
    chief(&realm, settings(Setting::Model { model: FAUX.into() }))
        .await;
    let model_pushes = || {
        copies_to(&realm, Hand::NUMBER)
            .into_iter()
            .filter_map(|copies| copies.model)
            .collect::<Vec<_>>()
    };
    until("the Hand is pushed its model", || {
        !model_pushes().is_empty()
    })
    .await;
    let note = EditCatalogue {
        model: FAUX.into(),
        set: vec![CardSetting::Note {
            note: "Preferred.".into(),
        }],
    };

    chief(&realm, note).await;
    let edited = chief(&realm, priced(FAUX, Some(price(0.12)))).await;

    assert_eq!(edited, Done::Done);
    until("the Hand is pushed its model's price", || {
        model_pushes()
            .iter()
            .any(|model| model.value.price == Some(price(0.12)))
    })
    .await;
    let models = model_pushes();
    assert_eq!(models.len(), 2, "a note moves no model copy");
    assert_eq!(models[1].value.model, FAUX);
    assert!(models[1].version > models[0].version);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_edit_changes_nothing() {
    let realm = realm_with_hand().await;
    let before = read_copies(&realm, &ReadCopies::default()).await;
    let blank = EditSettings {
        set: vec![
            Setting::ReflectEvery { turns: 4 },
            Setting::Character { text: " ".into() },
        ],
    };

    let refusals = (
        chief(&realm, blank).await,
        chief(&realm, priced(FAUX, Some(price(-1.0)))).await,
        chief(&realm, priced("missing", None)).await,
    );

    assert_eq!(
        refusals,
        (
            Done::refused(SettingRefusal::BlankCharacter),
            Done::refused(CatalogueRefusal::NegativeCost),
            Done::refused(CatalogueRefusal::NoSuchModel {
                model: "missing".into()
            }),
        )
    );
    assert_eq!(
        read_copies(&realm, &ReadCopies::default()).await,
        before
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn allow_net_shows_each_reach_pending_until_the_workspace_answers()
 {
    let (realm, _) = hamlet_with(0).await;
    realm.seen.hold(Callee::SetReach);

    chief(&realm, settings(Setting::AllowNet { allow: true })).await;

    seen_until(
        &realm,
        "the mayor's reach waits for the workspace",
        |seen| {
            seen.agents.iter().any(|shown| {
                shown.agent == MAYOR
                    && matches!(
                        shown.reach,
                        ReachSeen::Pending { .. }
                    )
            })
        },
    )
    .await;
    realm.seen.release(Callee::SetReach);
    realm.live(MAYOR).await;
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_endpoint_is_refused_in_todays_words() {
    let realm = realm_with_hand().await;
    let endpoint = |address: &str| {
        settings(Setting::Endpoints {
            endpoints: vec![Endpoint {
                name: "local".into(),
                base_url: address.into(),
            }],
        })
    };
    let refused = |address: &str| {
        let address = address.to_owned();
        move |refusal: fn(String) -> SettingRefusal| {
            Done::refused(refusal(address))
        }
    };

    let mut answers = Vec::new();
    for address in [
        "ftp://host",
        "localhost:11434",
        "nowhere",
        "http://me:key@host",
        " http://127.0.0.1:11434/v1 ",
    ] {
        answers.push(chief(&realm, endpoint(address)).await);
    }

    assert_eq!(
        answers,
        [
            refused("ftp://host")(|address| {
                SettingRefusal::NotHttp { address }
            }),
            refused("localhost:11434")(|address| {
                SettingRefusal::NotHttp { address }
            }),
            refused("nowhere")(|address| {
                SettingRefusal::NotAnAddress { address }
            }),
            Done::refused(SettingRefusal::AddressHoldsKey),
            Done::Done,
        ]
    );
    realm.host.shutdown().await.unwrap();
}
