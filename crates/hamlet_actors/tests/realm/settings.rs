use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use hamlet_actor::{Actor, Ctx, Handler};
use hamlet_host::{Caller, Host, HostOptions, HostRef};
use hamlet_messages::address::{self, Party};
use hamlet_messages::agent::copies::{Copies, CopyVersion, Price};
use hamlet_messages::agent::{AgentInput, Fill, Place};
use hamlet_messages::realm::budget::{BudgetLimit, Payer};
use hamlet_messages::realm::documents::{
    CardSetting, CatalogueRefusal, EditCatalogue, EditCharter,
    EditSettings, ReadCharter, ReadCopies, Setting, SettingRefusal,
};
use hamlet_messages::realm::{
    Asked, ChiefRequest, Done, HamletRefusal, Limit, Refusable,
    SetLimits,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;
use hamlet_messages::workspace::reach::Reach;
use hamlet_protocol::client::RequestId;
use hamlet_testing::Scratch;

use super::world::seen_until;
use super::{
    Forward, ProfileStandIn, Realm, Seen, WorkspaceStandIn, forward,
    hamlet_with, key, kind, until,
};

const FAUX: &str = "faux::faux-1";

/// The copies each agent was pushed.
#[derive(Default)]
struct Pushed(Mutex<Vec<(AgentNumber, Copies)>>);

impl Pushed {
    fn to(&self, agent: AgentNumber) -> Vec<Copies> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(to, _)| *to == agent)
            .map(|(_, copies)| copies.clone())
            .collect()
    }
}

/// An agent that keeps the copies it is pushed and takes the calls a
/// hire's steps make of it.
struct CopiesStandIn {
    pushed: Arc<Pushed>,
    agent: AgentNumber,
}

#[async_trait::async_trait]
impl Actor for CopiesStandIn {
    type Input = Option<AgentInput>;
    type Actions = (Copies, Place, Fill, Forward);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> anyhow::Result<Self> {
        Ok(Self {
            pushed: HostRef::of(ctx.inner())?
                .value::<Pushed>()
                .expect("the host holds the copies pushed"),
            agent: AgentNumber(key(ctx)?),
        })
    }
}

impl Handler<Copies> for CopiesStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        copies: Copies,
    ) -> anyhow::Result<()> {
        self.pushed.0.lock().unwrap().push((self.agent, copies));
        Ok(())
    }
}

impl Handler<Place> for CopiesStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        _: Place,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

impl Handler<Fill> for CopiesStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        _: Fill,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

impl Handler<Forward> for CopiesStandIn {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: Forward,
    ) -> anyhow::Result<Vec<u8>> {
        forward(&ctx, asked).await
    }
}

/// A realm whose agents keep the copies they are pushed, with the
/// Hand live.
async fn realm_with_copies() -> (Realm, Arc<Pushed>) {
    let scratch = Scratch::new("realm-copies");
    let seen = Arc::new(Seen::default());
    let pushed = Arc::new(Pushed::default());
    let kinds = HashMap::from([
        (
            address::Realm::KIND.to_owned(),
            hamlet_actors::realm::factory(),
        ),
        (
            address::Agent::KIND.to_owned(),
            kind::<CopiesStandIn>(address::Agent::KIND),
        ),
        (
            address::Profile::KIND.to_owned(),
            kind::<ProfileStandIn>(address::Profile::KIND),
        ),
        (
            address::Workspace::KIND.to_owned(),
            kind::<WorkspaceStandIn>(address::Workspace::KIND),
        ),
    ]);
    let options = HostOptions::default()
        .with_value(seen.clone())
        .with_value(pushed.clone());
    let host = Host::open_with(&scratch, kinds, options).unwrap();
    for at in [address::Workspace.address(), address::Realm.address()]
    {
        host.get_or_start(&at.unwrap()).await.unwrap();
    }
    let realm = Realm {
        host,
        seen,
        options: None,
        scratch,
    };
    realm.live(Hand::NUMBER).await;
    (realm, pushed)
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
    let (realm, pushed) = realm_with_copies().await;

    let edited = chief(&realm, charter("Be brief.")).await;

    assert_eq!(edited, Done::Done);
    until("the Hand is pushed the new charter", || {
        pushed.to(Hand::NUMBER).iter().any(|copies| {
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
    let (realm, _) = realm_with_copies().await;
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
    let (realm, _) = realm_with_copies().await;
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
    // a cost of $1 comes before a limit the other kind of row has, so
    // each whole edit is refused and both budgets stay $50.
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

    assert_eq!(on_hamlet, Done::refused(HamletRefusal::RealmsLimit));
    assert_eq!(on_realm, Done::refused(HamletRefusal::HamletsLimit));
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
    let (realm, pushed) = realm_with_copies().await;
    chief(&realm, settings(Setting::Model { model: FAUX.into() }))
        .await;
    let model_pushes = || {
        pushed
            .to(Hand::NUMBER)
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
    let (realm, _) = realm_with_copies().await;
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
