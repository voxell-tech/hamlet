use hamlet_store::ids::{Actor, AgentId, HamletId};

use super::addresses::{self, Address, Reach};
use super::agents::{self, Effort, EndHow};
use super::hamlets::{self, Founding};
use super::hires::{self, Hired, Hiring, Spec};
use super::refusal::Refusal;
use super::roads::{self, Agreed, Way};
use super::workers::{self, Start, Starter};
use super::{authority, chiefs, city, merges, tree};
use crate::settings::{Overrides, Settings};
use crate::talk::{Env, chats, models};
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

#[test]
fn houses_keep_apart_and_clear_of_the_middle() {
    let mut realm = Realm::new();
    let chief = realm.chief;
    for name in ["ada", "bo", "cy", "di", "ed", "flo"] {
        realm
            .chief(|cx| city::make(cx, name, chief, "", None))
            .unwrap();
    }
    let spots = city::all(realm.db())
        .unwrap()
        .into_iter()
        .map(|s| (s.house_x.unwrap(), s.house_z.unwrap()))
        .collect::<Vec<_>>();
    for (at, (x, z)) in spots.iter().enumerate() {
        assert!(x.hypot(*z) >= city::MIDDLE);
        for (ox, oz) in &spots[at + 1..] {
            assert!((x - ox).hypot(z - oz) >= city::APART);
        }
    }
}

#[test]
fn a_scholar_a_hire_makes_waits_at_the_harbor_until_let_in() {
    use crate::oversight::{self, WaitKind};
    let mut realm = Realm::new();
    let mayor = realm.found("docs", "ada").agent;
    let chief = realm.chief;
    // The Hand makes scholars for the chiefs: they move in at once.
    let hand = realm.hand;
    let kept = realm
        .by(Actor::Agent(hand), |cx| {
            city::make(cx, "kept", chief, "", None)
        })
        .unwrap();
    let arrival = |realm: &mut Realm, id| {
        let found = city::get(realm.db(), id).unwrap();
        (found.arrival, found.house_x.is_some())
    };
    assert_eq!(
        arrival(&mut realm, kept),
        (city::Arrival::Housed, true)
    );
    let sam = realm
        .by(Actor::Agent(mayor), |cx| {
            city::make(cx, "sam", chief, "", None)
        })
        .unwrap();
    assert_eq!(
        arrival(&mut realm, sam),
        (city::Arrival::Waiting, false)
    );
    let asks = |realm: &mut Realm| {
        oversight::open(realm.db()).unwrap().iter().any(|w| {
            w.kind == WaitKind::Newcomer && w.scholar == Some(sam)
        })
    };
    assert!(asks(&mut realm));
    realm.chief(|cx| city::welcome(cx, sam, "")).unwrap();
    assert_eq!(
        arrival(&mut realm, sam),
        (city::Arrival::Housed, true)
    );
    assert!(!asks(&mut realm));
    // A chief can send it back again.
    realm
        .chief(|cx| city::turn_away(cx, sam, "not yet"))
        .unwrap();
    assert_eq!(
        arrival(&mut realm, sam),
        (city::Arrival::Turned, false)
    );
}

#[test]
fn a_lead_leaves_a_hire_on_a_chiefs_task_alone() {
    let mut realm = Realm::new();
    let mayor = realm.found("docs", "ada").agent;
    let bo =
        hire(&mut realm, &docs(), mayor, new("bo")).unwrap().agent;
    let cy =
        hire(&mut realm, &docs(), mayor, new("cy")).unwrap().agent;
    let env = Env {
        settings: Settings::default(),
        overrides: Overrides {
            faux: true,
            ..Overrides::default()
        },
    };
    let chief = realm.chief;
    realm
        .chief(|cx| {
            chats::say(
                cx,
                &env,
                bo,
                chief,
                "r1",
                "Count the files.",
            )?;
            Ok(Ok(()))
        })
        .unwrap();
    // Unanswered, the chief's line keeps it from its lead.
    let protected = Err(Refusal::OnChiefsTask {
        name: "bo@docs".into(),
    });
    let ended = realm.by(Actor::Agent(mayor), |cx| {
        hires::end(cx, bo, EndHow::Ended, "done")
    });
    assert_eq!(ended, protected);
    let merged = realm.by(Actor::Agent(mayor), |cx| {
        merges::merge(cx, cy, bo, "they overlap").map(|m| m.map(drop))
    });
    assert_eq!(merged, protected);
    // A chief may.
    realm
        .chief(|cx| hires::end(cx, bo, EndHow::Ended, "done"))
        .unwrap();
}

#[test]
fn a_hire_without_a_purpose_takes_its_first_message() {
    let mut realm = Realm::new();
    let mayor = realm.found("docs", "ada").agent;
    let owner = realm.chief;
    let bo = realm
        .chief(|cx| {
            hires::hire(
                cx,
                Spec {
                    hamlet: docs(),
                    parent: Some(mayor),
                    hiring: new("bo"),
                    purpose: String::new(),
                    reason: String::new(),
                    owner,
                },
            )
        })
        .unwrap()
        .agent;
    let env = Env {
        settings: Settings::default(),
        overrides: Overrides {
            faux: true,
            ..Overrides::default()
        },
    };
    for (request, text) in
        [("r1", "\nSort the shelves.\nThen rest."), ("r2", "Sweep.")]
    {
        realm
            .chief(|cx| {
                chats::say(cx, &env, bo, owner, request, text)?;
                Ok(Ok(()))
            })
            .unwrap();
    }
    let purpose = hires::get(realm.db(), bo).unwrap().purpose;
    assert_eq!(purpose, "Sort the shelves.");
}

#[test]
fn an_agents_own_effort_wins_and_the_realms_fills_in() {
    let mut realm = Realm::new();
    let mayor = realm.found("docs", "ada").agent;
    let bo =
        hire(&mut realm, &docs(), mayor, new("bo")).unwrap().agent;
    let settings = Settings {
        effort: Some(Effort::Low),
        ..Settings::default()
    };
    let effort = |realm: &mut Realm, agent| {
        models::effort_of(realm.db(), &settings, agent).unwrap()
    };
    assert_eq!(effort(&mut realm, bo), Some(Effort::Low));
    realm
        .chief(|cx| agents::set_effort(cx, bo, Some(Effort::Max)))
        .unwrap();
    assert_eq!(effort(&mut realm, bo), Some(Effort::Max));
    assert_eq!(effort(&mut realm, mayor), Some(Effort::Low));
    // A worker thinks as hard as its starter unless it was told.
    let worker = realm
        .by(Actor::Agent(bo), |cx| {
            workers::start(
                cx,
                Start {
                    starter: Starter::Agent(bo),
                    hamlet: None,
                    brief: "Count the files".into(),
                    tools: vec![],
                    model: None,
                    waits: true,
                    call: None,
                },
            )
        })
        .unwrap();
    assert_eq!(effort(&mut realm, worker), Some(Effort::Max));
    realm.chief(|cx| agents::set_effort(cx, bo, None)).unwrap();
    assert_eq!(effort(&mut realm, worker), Some(Effort::Low));
}

#[test]
fn an_effort_a_model_lacks_gives_way_to_the_nearest_it_has() {
    let has = [Effort::Off, Effort::Low, Effort::High];
    assert_eq!(Effort::Max.clamped(&has), Effort::High);
    assert_eq!(Effort::Minimal.clamped(&has), Effort::Low);
    assert_eq!(Effort::Low.clamped(&has), Effort::Low);
    assert_eq!(Effort::High.clamped(&[Effort::Off]), Effort::Off);
}

#[test]
fn only_a_chief_of_the_hamlet_steers_its_agents() {
    let mut realm = Realm::new();
    let mayor = realm.found("docs", "ada").agent;
    let other = chiefs::ensure(realm.db(), "sam", 2).unwrap();
    let hand = realm.hand;
    let (own, other) =
        (Actor::Chief(realm.chief), Actor::Chief(other));
    let db = realm.db();
    assert!(authority::may_steer(db, own, mayor).unwrap());
    assert!(!authority::may_steer(db, other, mayor).unwrap());
    // The Hand serves every chief of the realm.
    assert!(authority::may_steer(db, other, hand).unwrap());
}
