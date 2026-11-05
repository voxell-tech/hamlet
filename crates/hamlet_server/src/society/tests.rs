use hamlet_store::ids::{Actor, AgentId, HamletId};

use super::addresses::{self, Address, Reach};
use super::agents::{self, EndHow};
use super::hamlets::{self, Founding};
use super::hires::{self, Hired, Hiring, Spec};
use super::refusal::Refusal;
use super::roads::{self, Agreed, Way};
use super::workers::{self, Start, Starter};
use super::{city, tree};
use crate::testing::Realm;

fn docs() -> HamletId {
    HamletId("docs".into())
}

/// Hires the scholar `name`, new or existing, under `parent` in
/// `hamlet`.
fn hire(
    realm: &mut Realm,
    hamlet: &HamletId,
    parent: AgentId,
    hiring: Hiring,
) -> Result<Hired, Refusal> {
    let (owner, hamlet) = (realm.chief, hamlet.clone());
    realm.chief(|cx| {
        hires::hire(
            cx,
            Spec {
                hamlet,
                parent: Some(parent),
                hiring,
                purpose: "Help".into(),
                reason: "needed".into(),
                owner,
            },
        )
    })
}

fn new(name: &str) -> Hiring {
    Hiring::New {
        name: name.into(),
        character: String::new(),
    }
}

fn name(realm: &mut Realm, id: AgentId) -> String {
    addresses::name_of(realm.db(), id).unwrap()
}

fn find(
    realm: &mut Realm,
    text: &str,
    here: Option<&HamletId>,
) -> Result<AgentId, Refusal> {
    let address = addresses::parse(text)?;
    addresses::resolve(realm.db(), &address, here, Reach::Living)
        .unwrap()
}

#[test]
fn a_scholar_hired_twice_in_one_hamlet_is_named_by_number() {
    let mut realm = Realm::new();
    let mayor = realm.found("docs", "ada");
    let scout =
        hire(&mut realm, &docs(), mayor.agent, new("scout")).unwrap();
    let again = hire(
        &mut realm,
        &docs(),
        mayor.agent,
        Hiring::Existing {
            scholar: scout.scholar,
            pin: None,
        },
    )
    .unwrap();
    assert_eq!(scout.scholar, again.scholar);
    assert_eq!(name(&mut realm, mayor.agent), "ada@docs");
    assert_eq!(name(&mut realm, scout.agent), "scout#1@docs");
    assert_eq!(name(&mut realm, again.agent), "scout#2@docs");
    assert!(matches!(
        find(&mut realm, "scout", Some(&docs())),
        Err(Refusal::Ambiguous { .. })
    ));
    assert_eq!(
        find(&mut realm, "scout#2@docs", None),
        Ok(again.agent)
    );
    assert_eq!(find(&mut realm, "mayor@docs", None), Ok(mayor.agent));
    assert_eq!(find(&mut realm, "hand", None), Ok(realm.hand));
    // A name a living scholar holds is not made again.
    assert!(matches!(
        hire(&mut realm, &docs(), mayor.agent, new("scout")),
        Err(Refusal::NameTaken { .. })
    ));
}

#[test]
fn the_tree_keeps_its_depth_and_heals_when_a_lead_ends() {
    let mut realm = Realm::new();
    let mayor = realm.found("docs", "ada").agent;
    let lead =
        hire(&mut realm, &docs(), mayor, new("lead")).unwrap().agent;
    let mid =
        hire(&mut realm, &docs(), lead, new("mid")).unwrap().agent;
    let low =
        hire(&mut realm, &docs(), mid, new("low")).unwrap().agent;
    assert_eq!(tree::depth(realm.db(), low).unwrap(), 3);
    assert!(matches!(
        hire(&mut realm, &docs(), low, new("deep")),
        Err(Refusal::TooDeep { most: 3 })
    ));
    assert_eq!(
        realm.chief(|cx| hires::end(
            cx,
            mayor,
            EndHow::Ended,
            "done"
        )),
        Err(Refusal::EndingMayor)
    );
    realm
        .chief(|cx| hires::end(cx, mid, EndHow::Ended, "done"))
        .unwrap();
    assert_eq!(tree::parent(realm.db(), low).unwrap(), Some(lead));
    assert!(tree::is_above(realm.db(), mayor, low).unwrap());
    // Back under its old parent, which still lives.
    realm
        .chief(|cx| hires::come_back(cx, mid, "again"))
        .unwrap();
    assert_eq!(tree::parent(realm.db(), mid).unwrap(), Some(lead));
}

#[test]
fn a_new_mayor_takes_the_root_and_the_old_one_stays_under_it() {
    let mut realm = Realm::new();
    let old = realm.found("docs", "ada").agent;
    let next =
        hire(&mut realm, &docs(), old, new("bo")).unwrap().agent;
    let other =
        hire(&mut realm, &docs(), old, new("cy")).unwrap().agent;
    realm
        .chief(|cx| hires::seat(cx, &docs(), next, "steadier"))
        .unwrap();
    assert_eq!(tree::mayor(realm.db(), &docs()).unwrap(), next);
    assert_eq!(tree::parent(realm.db(), next).unwrap(), None);
    assert_eq!(tree::parent(realm.db(), old).unwrap(), Some(next));
    assert_eq!(tree::parent(realm.db(), other).unwrap(), Some(next));
    assert_eq!(
        tree::children(realm.db(), None).unwrap(),
        vec![next],
        "the Hand is above the new mayor"
    );
}

#[test]
fn a_removed_hamlet_comes_back_whole_only_while_its_scholars_live() {
    let mut realm = Realm::new();
    let mayor = realm.found("docs", "ada");
    let scout =
        hire(&mut realm, &docs(), mayor.agent, new("scout")).unwrap();
    realm
        .chief(|cx| hamlets::remove(cx, &docs(), "done"))
        .unwrap();
    for id in [mayor.agent, scout.agent] {
        let ended = agents::get(realm.db(), id).unwrap();
        assert_eq!(ended.ended_how, Some(EndHow::Hamlet));
    }
    assert!(matches!(
        find(&mut realm, "scout", None),
        Err(Refusal::NoOne { .. })
    ));
    realm
        .chief(|cx| city::archive(cx, scout.scholar, "retired"))
        .unwrap();
    assert!(matches!(
        realm.chief(|cx| hamlets::restore(cx, &docs(), "again")),
        Err(Refusal::ScholarGone { .. })
    ));
    realm
        .chief(|cx| city::restore(cx, scout.scholar, "back"))
        .unwrap();
    realm
        .chief(|cx| hamlets::restore(cx, &docs(), "again"))
        .unwrap();
    assert_eq!(
        tree::parent(realm.db(), scout.agent).unwrap(),
        Some(mayor.agent)
    );
    assert_eq!(
        tree::mayor(realm.db(), &docs()).unwrap(),
        mayor.agent
    );
    realm
        .chief(|cx| hamlets::remove(cx, &docs(), "done"))
        .unwrap();
    realm.chief(|cx| hamlets::erase(cx, &docs())).unwrap();
    assert!(agents::get(realm.db(), scout.agent).unwrap().erased);
    assert_eq!(name(&mut realm, scout.agent), "docs#1");
    let chief = realm.chief;
    assert!(matches!(
        realm.chief(|cx| hamlets::found(
            cx,
            Founding {
                id: docs(),
                chief,
                mayor: new("zed"),
                purpose: "again".into(),
            }
        )),
        Err(Refusal::HamletUsed { .. })
    ));
}

#[test]
fn a_scholar_leaves_with_its_hires_and_comes_back_with_them() {
    let mut realm = Realm::new();
    let mayor = realm.found("docs", "ada").agent;
    realm.found("site", "bea");
    let scout =
        hire(&mut realm, &docs(), mayor, new("scout")).unwrap();
    let site_mayor =
        tree::mayor(realm.db(), &HamletId("site".into())).unwrap();
    let there = hire(
        &mut realm,
        &HamletId("site".into()),
        site_mayor,
        Hiring::Existing {
            scholar: scout.scholar,
            pin: None,
        },
    )
    .unwrap();
    let ada = hires::get(realm.db(), mayor).unwrap().scholar;
    assert_eq!(
        realm.chief(|cx| city::archive(cx, ada, "retired")),
        Err(Refusal::EndingMayor),
        "a mayor's scholar stays while it holds the office"
    );
    realm
        .chief(|cx| city::archive(cx, scout.scholar, "retired"))
        .unwrap();
    assert!(
        !agents::get(realm.db(), there.agent).unwrap().is_living()
    );
    // Its name is free again, so it cannot come back under it.
    let taken =
        hire(&mut realm, &docs(), mayor, new("scout")).unwrap();
    assert!(matches!(
        realm.chief(|cx| city::restore(cx, scout.scholar, "back")),
        Err(Refusal::NameTaken { .. })
    ));
    let taker = hires::get(realm.db(), taken.agent).unwrap().scholar;
    realm
        .chief(|cx| hires::end(cx, taken.agent, EndHow::Ended, "x"))
        .unwrap();
    realm.chief(|cx| city::archive(cx, taker, "x")).unwrap();
    realm
        .chief(|cx| city::restore(cx, scout.scholar, "back"))
        .unwrap();
    assert!(
        agents::get(realm.db(), scout.agent).unwrap().is_living()
    );
    assert!(
        agents::get(realm.db(), there.agent).unwrap().is_living()
    );
}

#[test]
fn roads_follow_the_tree_and_join_hamlets_once_both_mayors_agree() {
    let mut realm = Realm::new();
    let ada = realm.found("docs", "ada").agent;
    let bea = realm.found("site", "bea").agent;
    let site = HamletId("site".into());
    let scout =
        hire(&mut realm, &docs(), ada, new("scout")).unwrap().agent;
    let mill =
        hire(&mut realm, &docs(), ada, new("mill")).unwrap().agent;
    let far = hire(&mut realm, &site, bea, new("far")).unwrap().agent;
    let hand = realm.hand;
    let db = realm.db();
    assert_eq!(roads::way(db, scout, ada).unwrap(), Some(Way::Tree));
    assert_eq!(roads::way(db, hand, ada).unwrap(), Some(Way::Tree));
    assert_eq!(roads::way(db, ada, bea).unwrap(), Some(Way::Mayors));
    assert_eq!(roads::way(db, scout, mill).unwrap(), None);
    assert_eq!(roads::way(db, hand, scout).unwrap(), None);

    let laid = realm
        .by(Actor::Agent(ada), |cx| {
            roads::lay(cx, scout, mill, "pair", None)
        })
        .unwrap();
    assert_eq!(
        roads::way(realm.db(), mill, scout).unwrap(),
        Some(Way::Road(laid))
    );
    assert!(matches!(
        realm.chief(|cx| roads::lay(cx, scout, far, "x", None)),
        Err(Refusal::OtherHamlet { .. })
    ));
    let first = realm
        .by(Actor::Agent(ada), |cx| {
            roads::agree(cx, ada, scout, far, "trade", None)
        })
        .unwrap();
    assert_eq!(first, Agreed::Waiting);
    assert_eq!(roads::way(realm.db(), scout, far).unwrap(), None);
    let second = realm
        .by(Actor::Agent(bea), |cx| {
            roads::agree(cx, bea, scout, far, "trade", None)
        })
        .unwrap();
    let Agreed::Laid(road) = second else {
        panic!("both agreed")
    };
    assert_eq!(
        roads::way(realm.db(), far, scout).unwrap(),
        Some(Way::Road(road))
    );
}

#[test]
fn a_worker_starts_no_worker_and_works_where_its_starter_does() {
    let mut realm = Realm::new();
    let ada = realm.found("docs", "ada").agent;
    let start = |starter| Start {
        starter,
        hamlet: None,
        brief: "Count the files".into(),
        tools: vec!["read".into()],
        model: None,
        waits: true,
        call: None,
    };
    let worker = realm
        .by(Actor::Agent(ada), |cx| {
            workers::start(cx, start(Starter::Agent(ada)))
        })
        .unwrap();
    let found = workers::get(realm.db(), worker).unwrap();
    assert_eq!(found.hamlet, Some(docs()));
    assert_eq!(found.starter, Some(ada));
    assert_eq!(
        realm.by(Actor::Agent(worker), |cx| {
            workers::start(cx, start(Starter::Agent(worker)))
        }),
        Err(Refusal::Worker)
    );
    let hand = realm.hand;
    let by_hand = realm
        .by(Actor::Agent(hand), |cx| {
            workers::start(cx, start(Starter::Agent(hand)))
        })
        .unwrap();
    assert_eq!(
        workers::get(realm.db(), by_hand).unwrap().hamlet,
        None
    );
    assert!(
        addresses::parse("worker#3")
            .is_ok_and(|a| a == Address::Worker(AgentId(3)))
    );
}
