use hamlet_store::ids::{Actor, AgentId, HamletId};

use super::addresses::{self, Reach};
use super::agents::{self, Effort, EndHow};
use super::hamlets::{self, Founding};
use super::hires::{self, Hired, Hiring, Spec};
use super::refusal::Refusal;
use super::roads::{self, Agreed, Way};
use super::workers::{self, Start, Starter};
use super::{authority, chiefs, city, merges, tree};
use crate::conversations::{chats, models};
use crate::realm::settings::{
    EffectiveSettings, Overrides, Settings,
};
use crate::testing::Realm;

/// Founds a hamlet named `name` under a new lead named `lead`:
/// its id and its lead.
fn found(
    realm: &mut Realm,
    name: &str,
    lead: &str,
) -> (HamletId, AgentId) {
    let hired = realm.found(name, lead);
    (hired.hamlet, hired.agent)
}

/// Hires the profile `name`, new or existing, under `parent` in
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
    Hiring::Blank {
        name: name.into(),
        character: String::new(),
    }
}

/// The hiring of a new profile of the city named `name`, which,
/// unlike a blank agent's, can be hired again.
fn of_city(realm: &mut Realm, name: &str) -> Hiring {
    let chief = realm.chief;
    let profile = realm
        .chief(|cx| city::make(cx, name, chief, "", None))
        .unwrap();
    Hiring::Existing { profile, pin: None }
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
fn a_hire_is_addressed_by_its_id_never_by_name() {
    let mut realm = Realm::new();
    let lead = realm.found("docs", "ada");
    let docs = lead.hamlet.clone();
    // A hamlet's id is made for it: not its name.
    assert_ne!(docs.as_str(), "docs");
    let scouting = of_city(&mut realm, "scout");
    let scout =
        hire(&mut realm, &docs, lead.agent, scouting).unwrap();
    let again = hire(
        &mut realm,
        &docs,
        lead.agent,
        Hiring::Existing {
            profile: scout.profile,
            pin: None,
        },
    )
    .unwrap();
    assert_eq!(scout.profile, again.profile);
    assert_eq!(
        name(&mut realm, lead.agent),
        format!("hire#{} (ada)", lead.agent)
    );
    assert_eq!(
        name(&mut realm, again.agent),
        format!("hire#{} (scout)", again.agent)
    );
    assert_eq!(
        find(&mut realm, &format!("hire#{}", again.agent), None),
        Ok(again.agent)
    );
    assert!(matches!(
        find(&mut realm, "scout", Some(&docs)),
        Err(Refusal::BadAddress { .. })
    ));
    assert!(matches!(
        find(&mut realm, "hire#999", None),
        Err(Refusal::NoOne { .. })
    ));
    // The Hand is no hire.
    let hand = format!("hire#{}", realm.hand);
    assert!(matches!(
        find(&mut realm, &hand, None),
        Err(Refusal::NoOne { .. })
    ));
    assert_eq!(
        find(&mut realm, &format!("lead@{docs}"), None),
        Ok(lead.agent)
    );
    assert!(matches!(
        find(&mut realm, "lead@docs", None),
        Err(Refusal::BadAddress { .. })
    ));
    assert_eq!(find(&mut realm, "hand", None), Ok(realm.hand));
    // A name is a label: it may repeat, and a profile's is its id.
    let twin =
        hire(&mut realm, &docs, lead.agent, new("scout")).unwrap();
    assert_ne!(twin.profile, scout.profile);
    assert_eq!(
        addresses::parse_profile(&format!(
            "profile#{}",
            twin.profile
        )),
        Ok(twin.profile)
    );
    assert!(addresses::parse_profile("scout").is_err());
}

#[test]
fn a_profile_is_renamed_without_changing_its_address() {
    let mut realm = Realm::new();
    let lead = realm.found("docs", "ada");
    realm
        .chief(|cx| city::rename(cx, lead.profile, "Ada Lovelace"))
        .unwrap();
    assert_eq!(
        name(&mut realm, lead.agent),
        format!("hire#{} (Ada Lovelace)", lead.agent)
    );
    assert!(matches!(
        realm.chief(|cx| city::rename(cx, lead.profile, "a: b")),
        Err(Refusal::BadName { .. })
    ));
}

#[test]
fn the_tree_heals_when_a_parent_ends() {
    let mut realm = Realm::new();
    let (docs, lead) = found(&mut realm, "docs", "ada");
    let top =
        hire(&mut realm, &docs, lead, new("top")).unwrap().agent;
    let mid = hire(&mut realm, &docs, top, new("mid")).unwrap().agent;
    let low = hire(&mut realm, &docs, mid, new("low")).unwrap().agent;
    assert_eq!(tree::parent(realm.db(), low).unwrap(), Some(mid));
    assert_eq!(
        realm.chief(|cx| hires::end(cx, lead, EndHow::Ended, "done")),
        Err(Refusal::EndingLead)
    );
    realm
        .chief(|cx| hires::end(cx, mid, EndHow::Ended, "done"))
        .unwrap();
    assert_eq!(tree::parent(realm.db(), low).unwrap(), Some(top));
    assert!(tree::is_above(realm.db(), lead, low).unwrap());
    // Back under its old parent, which still lives.
    realm
        .chief(|cx| hires::come_back(cx, mid, "again"))
        .unwrap();
    assert_eq!(tree::parent(realm.db(), mid).unwrap(), Some(top));
}

#[test]
fn a_new_lead_takes_the_root_and_the_old_one_stays_under_it() {
    let mut realm = Realm::new();
    let (docs, old) = found(&mut realm, "docs", "ada");
    let next = hire(&mut realm, &docs, old, new("bo")).unwrap().agent;
    let other =
        hire(&mut realm, &docs, old, new("cy")).unwrap().agent;
    realm
        .chief(|cx| hires::set_lead(cx, &docs, next, "steadier"))
        .unwrap();
    assert_eq!(tree::lead(realm.db(), &docs).unwrap(), next);
    assert_eq!(tree::parent(realm.db(), next).unwrap(), None);
    assert_eq!(tree::parent(realm.db(), old).unwrap(), Some(next));
    assert_eq!(tree::parent(realm.db(), other).unwrap(), Some(next));
    assert_eq!(
        tree::children(realm.db(), None).unwrap(),
        vec![next],
        "the Hand is above the new lead"
    );
}

#[test]
fn a_removed_hamlet_comes_back_whole_only_while_its_profiles_live() {
    let mut realm = Realm::new();
    let lead = realm.found("docs", "ada");
    let docs = lead.hamlet.clone();
    let scout =
        hire(&mut realm, &docs, lead.agent, new("scout")).unwrap();
    realm
        .chief(|cx| hamlets::remove(cx, &docs, "done"))
        .unwrap();
    for id in [lead.agent, scout.agent] {
        let ended = agents::get(realm.db(), id).unwrap();
        assert_eq!(ended.ended_how, Some(EndHow::Hamlet));
    }
    assert!(matches!(
        find(&mut realm, &format!("hire#{}", scout.agent), None),
        Err(Refusal::NoOne { .. })
    ));
    realm
        .chief(|cx| city::archive(cx, scout.profile, "retired"))
        .unwrap();
    assert!(matches!(
        realm.chief(|cx| hamlets::restore(cx, &docs, "again")),
        Err(Refusal::ProfileGone { .. })
    ));
    realm
        .chief(|cx| city::restore(cx, scout.profile, "back"))
        .unwrap();
    realm
        .chief(|cx| hamlets::restore(cx, &docs, "again"))
        .unwrap();
    assert_eq!(
        tree::parent(realm.db(), scout.agent).unwrap(),
        Some(lead.agent)
    );
    assert_eq!(tree::lead(realm.db(), &docs).unwrap(), lead.agent);
    realm
        .chief(|cx| hamlets::remove(cx, &docs, "done"))
        .unwrap();
    realm.chief(|cx| hamlets::erase(cx, &docs)).unwrap();
    assert!(agents::get(realm.db(), scout.agent).unwrap().erased);
    assert_eq!(
        name(&mut realm, scout.agent),
        format!("hire#{}", scout.agent)
    );
    // Its name is free again, under an id never given before.
    let chief = realm.chief;
    let again = realm
        .chief(|cx| {
            hamlets::found(
                cx,
                Founding {
                    name: "docs".into(),
                    chief,
                    lead: new("zed"),
                    purpose: "again".into(),
                },
            )
        })
        .unwrap();
    assert_ne!(again.hamlet, docs);
}

#[test]
fn a_hamlet_must_have_a_name() {
    let mut realm = Realm::new();
    let chief = realm.chief;
    for name in ["", "  ", "a: b"] {
        assert!(matches!(
            realm.chief(|cx| hamlets::found(
                cx,
                Founding {
                    name: name.into(),
                    chief,
                    lead: new("ada"),
                    purpose: "Build it".into(),
                }
            )),
            Err(Refusal::BadName { .. })
        ));
    }
    let docs = realm.found("docs", "ada").hamlet;
    assert!(matches!(
        realm.chief(|cx| hamlets::rename(cx, &docs, " ")),
        Err(Refusal::BadName { .. })
    ));
    realm
        .chief(|cx| hamlets::rename(cx, &docs, "Docs site"))
        .unwrap();
    let named = hamlets::get(realm.db(), &docs).unwrap().name;
    assert_eq!(named, "Docs site");
}

#[test]
fn a_profile_leaves_with_its_hires_and_comes_back_with_them() {
    let mut realm = Realm::new();
    let (docs, lead) = found(&mut realm, "docs", "ada");
    let site = realm.found("site", "bea").hamlet;
    let scouting = of_city(&mut realm, "scout");
    let scout = hire(&mut realm, &docs, lead, scouting).unwrap();
    let site_lead = tree::lead(realm.db(), &site).unwrap();
    let there = hire(
        &mut realm,
        &site,
        site_lead,
        Hiring::Existing {
            profile: scout.profile,
            pin: None,
        },
    )
    .unwrap();
    let ada = hires::get(realm.db(), lead).unwrap().profile;
    assert_eq!(
        realm.chief(|cx| city::archive(cx, ada, "retired")),
        Err(Refusal::EndingLead),
        "a lead's profile stays while it holds the office"
    );
    realm
        .chief(|cx| city::archive(cx, scout.profile, "retired"))
        .unwrap();
    assert!(
        !agents::get(realm.db(), there.agent).unwrap().is_living()
    );
    realm
        .chief(|cx| city::restore(cx, scout.profile, "back"))
        .unwrap();
    assert!(
        agents::get(realm.db(), scout.agent).unwrap().is_living()
    );
    assert!(
        agents::get(realm.db(), there.agent).unwrap().is_living()
    );
}

#[test]
fn roads_follow_the_tree_and_join_hamlets_once_both_leads_agree() {
    let mut realm = Realm::new();
    let (docs, ada) = found(&mut realm, "docs", "ada");
    let (site, bea) = found(&mut realm, "site", "bea");
    let scout =
        hire(&mut realm, &docs, ada, new("scout")).unwrap().agent;
    let mill =
        hire(&mut realm, &docs, ada, new("mill")).unwrap().agent;
    let far = hire(&mut realm, &site, bea, new("far")).unwrap().agent;
    let hand = realm.hand;
    let db = realm.db();
    assert_eq!(roads::way(db, scout, ada).unwrap(), Some(Way::Tree));
    assert_eq!(roads::way(db, hand, ada).unwrap(), Some(Way::Tree));
    assert_eq!(roads::way(db, ada, bea).unwrap(), Some(Way::Leads));
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
    let (docs, ada) = found(&mut realm, "docs", "ada");
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
    assert_eq!(found.hamlet, Some(docs.clone()));
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
fn a_profile_a_hire_makes_waits_at_the_harbor_until_let_in() {
    use crate::society::oversight::{self, WaitKind};
    let mut realm = Realm::new();
    let (_, lead) = found(&mut realm, "docs", "ada");
    let chief = realm.chief;
    // The Hand makes profiles for the chiefs: they move in at once.
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
        .by(Actor::Agent(lead), |cx| {
            city::make(cx, "sam", chief, "", None)
        })
        .unwrap();
    assert_eq!(
        arrival(&mut realm, sam),
        (city::Arrival::Waiting, false)
    );
    let waits = |realm: &mut Realm| {
        oversight::open(realm.db()).unwrap().iter().any(|w| {
            w.kind == WaitKind::Newcomer && w.profile == Some(sam)
        })
    };
    assert!(waits(&mut realm));
    realm.chief(|cx| city::welcome(cx, sam, "")).unwrap();
    assert_eq!(
        arrival(&mut realm, sam),
        (city::Arrival::Housed, true)
    );
    assert!(!waits(&mut realm));
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
fn a_parent_leaves_a_hire_on_a_chiefs_task_alone() {
    let mut realm = Realm::new();
    let (docs, lead) = found(&mut realm, "docs", "ada");
    let bo = hire(&mut realm, &docs, lead, new("bo")).unwrap().agent;
    let cy = hire(&mut realm, &docs, lead, new("cy")).unwrap().agent;
    let env = EffectiveSettings {
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
    // Unanswered, the chief's line keeps it from its parent.
    let protected = Err(Refusal::OnChiefsTask {
        name: format!("hire#{bo} (bo)"),
    });
    let ended = realm.by(Actor::Agent(lead), |cx| {
        hires::end(cx, bo, EndHow::Ended, "done")
    });
    assert_eq!(ended, protected);
    let merged = realm.by(Actor::Agent(lead), |cx| {
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
    let (docs, lead) = found(&mut realm, "docs", "ada");
    let owner = realm.chief;
    let bo = realm
        .chief(|cx| {
            hires::hire(
                cx,
                Spec {
                    hamlet: docs.clone(),
                    parent: Some(lead),
                    hiring: new("bo"),
                    purpose: String::new(),
                    reason: String::new(),
                    owner,
                },
            )
        })
        .unwrap()
        .agent;
    let env = EffectiveSettings {
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
    let (docs, lead) = found(&mut realm, "docs", "ada");
    let bo = hire(&mut realm, &docs, lead, new("bo")).unwrap().agent;
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
    assert_eq!(effort(&mut realm, lead), Some(Effort::Low));
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
    let (_, lead) = found(&mut realm, "docs", "ada");
    let other = chiefs::ensure(realm.db(), "sam", 2).unwrap();
    let hand = realm.hand;
    let (own, other) =
        (Actor::Chief(realm.chief), Actor::Chief(other));
    let db = realm.db();
    assert!(authority::may_steer(db, own, lead).unwrap());
    assert!(!authority::may_steer(db, other, lead).unwrap());
    // The Hand serves every chief of the realm.
    assert!(authority::may_steer(db, other, hand).unwrap());
}
