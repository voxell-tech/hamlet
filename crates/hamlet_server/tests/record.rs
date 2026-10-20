//! The TypeScript record's contracts through marker-backed Rust
//! operations.

use hamlet_server::record::{
    Action, Actor, Admit, AgentRef, Amount, Audience, Change,
    DecisionKind, Departure, End, FoundRecord, Hamlet, Limits,
    MergeFiles, MergeNotes, NameFallback, OpenVote, Protection,
    Reason, RecordOperation, RecordResult, Refusal, RoadBudget,
};
use hamlet_server::{
    OperationId, Outcome, Reply, RuntimeInstance, Store,
};
use hamlet_testing::Scratch;

fn id(key: &str) -> OperationId {
    OperationId {
        runtime: RuntimeInstance::new("pi-one").unwrap(),
        key: key.into(),
    }
}

fn place(hamlet: &str, number: u64) -> AgentRef {
    AgentRef {
        hamlet: hamlet.into(),
        number,
    }
}

fn request(hamlet: &str, change: Change) -> RecordOperation {
    RecordOperation {
        hamlet: hamlet.into(),
        actor: Actor::Chief("chief".into()),
        at: 100,
        reason: " Needed ".into(),
        task: None,
        change,
    }
}

fn admit(name: &str) -> Change {
    Change::Admit(Admit {
        name: name.into(),
        purpose: " Maps ".into(),
        model: None,
        character: "Careful".into(),
        roads: Vec::new(),
    })
}

fn road(a: &str, b: &str) -> Change {
    Change::LayRoad {
        a: a.into(),
        b: b.into(),
    }
}
fn remove(name: &str) -> Change {
    Change::Depart {
        name: name.into(),
        how: Departure::Removed,
    }
}
fn restore(archive: &str, name: &str) -> Change {
    Change::Restore {
        archive: archive.into(),
        name: name.into(),
    }
}

fn run(
    store: &mut Store,
    key: &str,
    request: &RecordOperation,
) -> Outcome<RecordResult, Refusal> {
    match store.run(&id(key), request).unwrap() {
        Reply::Outcome(outcome) => outcome,
        Reply::Conflict => panic!("unexpected conflict: {key}"),
    }
}

fn done(
    store: &mut Store,
    key: &str,
    request: &RecordOperation,
) -> RecordResult {
    match run(store, key, request) {
        Outcome::Done(done) => done,
        Outcome::Refused(refusal) => {
            panic!("{}", refusal.text(Audience::Chief))
        }
    }
}

fn refused(
    store: &mut Store,
    key: &str,
    request: &RecordOperation,
) -> Refusal {
    match run(store, key, request) {
        Outcome::Refused(refusal) => refusal,
        Outcome::Done(done) => panic!("unexpected success: {done:?}"),
    }
}

fn record(store: &Store, hamlet: &str) -> Hamlet {
    store.record(hamlet).unwrap().unwrap()
}

fn found(store: &mut Store, hamlet: &str, name: &str) {
    assert_eq!(
        store
            .run(
                &id(&format!("found-{hamlet}")),
                &FoundRecord {
                    hamlet: hamlet.into(),
                    name: name.into(),
                    purpose: "Works".into(),
                    chief: "chief".into(),
                    at: 0
                }
            )
            .unwrap(),
        Reply::Outcome(Outcome::Done(place(hamlet, 0)))
    );
}

fn seeded(tag: &str) -> (Scratch, Store) {
    let scratch = Scratch::new(tag);
    let mut store =
        Store::open(&scratch.join("realm.sqlite")).unwrap();
    found(&mut store, "home", "founder");
    (scratch, store)
}

fn limits(
    store: &mut Store,
    key: &str,
    hamlet: &str,
    change: impl FnOnce(&mut Limits),
) {
    let mut limits = record(store, hamlet).limits;
    change(&mut limits);
    done(
        store,
        key,
        &request(
            hamlet,
            Change::SetLimits {
                limits,
                runs: Vec::new(),
            },
        ),
    );
}

fn departed(store: &mut Store, key: &str, name: &str) -> String {
    match done(store, key, &request("home", remove(name))) {
        RecordResult::Departed { archive } => archive,
        other => panic!("{other:?}"),
    }
}

fn carry(store: &mut Store, key: &str, from: u64, to: u64) {
    let mut call = request("home", Change::CarryRoad { to });
    call.actor = Actor::Member(place("home", from));
    done(store, key, &call);
}

#[test]
fn a_departed_namesake_reports_the_latest_departure() {
    let (_scratch, mut store) = seeded("record-namesake");
    done(&mut store, "first", &request("home", admit("scout")));
    departed(&mut store, "first-left", "scout");
    done(&mut store, "second", &request("home", admit("scout")));
    let mut call = request("home", remove("scout"));
    call.reason = "The second scout finished".into();
    done(&mut store, "second-left", &call);
    assert_eq!(
        refused(&mut store, "gone", &call).text(Audience::Agent),
        "`scout` was removed from the hamlet by the chief chief \
         (\"The second scout finished\"). Its house is archived and \
         its roads went with it; only a chief can restore it."
    );
}

#[test]
fn a_model_is_set_by_the_mayor_alone_within_the_chiefs_limits() {
    let (_scratch, mut store) = seeded("record-model");
    done(&mut store, "scout", &request("home", admit("scout")));
    done(&mut store, "smith", &request("home", admit("smith")));
    let set = |name: &str, model: Option<&str>| {
        let mut call = request(
            "home",
            Change::SetModel {
                name: name.into(),
                model: model.map(String::from),
                allowed: vec!["fast".into()],
                pinned: vec!["smith".into()],
                own: "house".into(),
            },
        );
        call.actor = Actor::Mayor(place("home", 0));
        call
    };
    let mut member = set("scout", Some("fast"));
    member.actor = Actor::Member(place("home", 2));
    assert_eq!(
        refused(&mut store, "member", &member).reason,
        Reason::NotMayor
    );
    assert_eq!(
        refused(&mut store, "office", &set("mayor", Some("fast")))
            .reason,
        Reason::AgentMissing {
            name: "mayor".into(),
            local: false
        }
    );
    assert_eq!(
        refused(&mut store, "pinned", &set("smith", Some("fast")))
            .reason,
        Reason::ModelPinned {
            name: "smith".into()
        }
    );
    assert!(matches!(
        refused(&mut store, "slow", &set("scout", Some("slow")))
            .reason,
        Reason::ModelNotAllowed { .. }
    ));
    done(&mut store, "fast", &set("scout", Some("fast")));
    assert_eq!(
        record(&store, "home").named("scout").unwrap().model,
        Some("fast".into())
    );
    done(&mut store, "own", &set("scout", None));
    let r = record(&store, "home");
    assert_eq!(r.named("scout").unwrap().model, None);
    assert_eq!(
        r.decisions.last().map(|d| (&d.by, &d.reason, &d.kind)),
        Some((
            &"mayor".to_owned(),
            &"Needed".to_owned(),
            &DecisionKind::Model {
                agent: 1,
                model: "house".into()
            }
        ))
    );
    done(
        &mut store,
        "hold",
        &request(
            "home",
            Change::SetGuards {
                open_votes: vec![OpenVote {
                    id: 1,
                    called: 100,
                    motion: "replace the mayor".into(),
                }],
                protected: Vec::new(),
            },
        ),
    );
    assert!(matches!(
        refused(&mut store, "held", &set("scout", Some("fast")))
            .reason,
        Reason::VoteOpen { vote: 1, .. }
    ));
}

#[test]
fn admission_and_departure_use_javascript_whitespace_rules() {
    let (_scratch, mut store) = seeded("record-whitespace");
    done(
        &mut store,
        "bom",
        &request("home", admit("\u{feff}Scout\u{feff}")),
    );
    assert_eq!(
        record(&store, "home").named("scout").unwrap().number,
        1
    );
    assert_eq!(
        refused(
            &mut store,
            "next-line",
            &request("home", admit("\u{85}Smith\u{85}"))
        )
        .reason,
        Reason::NameFormat
    );
    let mut call = request("home", admit("smith"));
    call.actor = Actor::Mayor(place("home", 0));
    if let Change::Admit(given) = &mut call.change {
        given.purpose = "\u{feff}".into();
    }
    assert_eq!(
        refused(&mut store, "empty-purpose", &call).reason,
        Reason::PurposeEmpty
    );
    call.change = remove("scout");
    call.reason = "\u{feff}".into();
    assert_eq!(
        refused(&mut store, "empty-reason", &call).reason,
        Reason::ReasonEmpty
    );
    assert!(record(&store, "home").named("scout").is_some());
}

#[test]
fn admission_has_realm_names_stable_numbers_and_a_cap_beside_the_current_mayor()
 {
    let (_scratch, mut store) = seeded("record-membership");
    found(&mut store, "mill", "miller");
    limits(&mut store, "cap", "home", |l| l.chief.most_agents = 2);
    let mut call = request("home", admit("FelineBiologist"));
    call.actor = Actor::Mayor(place("home", 0));
    assert_eq!(
        done(&mut store, "cat", &call),
        RecordResult::Admitted {
            agent: place("home", 1),
            road_refusals: Vec::new()
        }
    );
    let taken = refused(
        &mut store,
        "taken",
        &request("mill", admit("feline-biologist")),
    );
    assert_eq!(
        taken.reason,
        Reason::NameTaken {
            name: "feline-biologist".into(),
            archive: None
        }
    );
    assert_eq!(
        taken.text(Audience::Agent),
        "an agent named `feline-biologist` already exists"
    );
    done(&mut store, "second", &request("home", admit("second")));
    done(
        &mut store,
        "seat",
        &request("home", Change::Seat { number: 1 }),
    );
    let full =
        refused(&mut store, "full", &request("home", admit("third")));
    assert_eq!(
        full.text(Audience::Agent),
        "the hamlet is full (2 agents). Only the chief can raise \
         mostAgents on the hamlet's Bylaws page"
    );
    let archive = departed(&mut store, "depart-founder", "founder");
    assert!(
        matches!(done(&mut store, "restore-founder", &request("home", restore(&archive, "founder"))), RecordResult::Restored { agent, .. } if agent.number == 0)
    );
    let archive = departed(&mut store, "depart-second", "second");
    done(&mut store, "third", &request("home", admit("third")));
    assert_eq!(
        refused(
            &mut store,
            "restore-full",
            &request("home", restore(&archive, "second"))
        )
        .text(Audience::Chief),
        "the hamlet is full (2 agents); remove one first, or raise \
         mostAgents in the limits"
    );
    assert_eq!(record(&store, "home").next, 4);
    assert_eq!(
        store.agent_named("third").unwrap(),
        Some(place("home", 3))
    );
}

#[test]
fn names_validate_after_tidying_and_admission_keeps_partial_roads_and_allowed_models()
 {
    let (_scratch, mut store) = seeded("record-admit");
    for (i, name) in [
        "mayor",
        "founder",
        "all",
        "abc2",
        "é",
        "abcdefghijklmnopqrstuvwxyz",
    ]
    .iter()
    .enumerate()
    {
        let refusal = refused(
            &mut store,
            &format!("invalid-{i}"),
            &request("home", admit(name)),
        );
        assert!(matches!(
            refusal.reason,
            Reason::NameReserved { .. } | Reason::NameFormat
        ));
    }
    done(&mut store, "peer", &request("home", admit("peer")));
    limits(&mut store, "limits", "home", |l| {
        l.chief.models = vec!["local::model".into()];
        l.chief.most_roads = 1;
    });
    let mut call = request(
        "home",
        Change::Admit(Admit {
            name: "  Feline__Biologist  ".into(),
            purpose: " Maps ".into(),
            model: Some("local::model".into()),
            character: "Careful".into(),
            roads: vec![
                "peer".into(),
                "missing".into(),
                "peer".into(),
                "mayor".into(),
            ],
        }),
    );
    call.actor = Actor::Mayor(place("home", 0));
    call.task = Some("task-7".into());
    let RecordResult::Admitted { road_refusals, .. } =
        done(&mut store, "admit", &call)
    else {
        panic!()
    };
    assert_eq!(
        road_refusals
            .iter()
            .map(|r| r.refusal.text(Audience::Agent))
            .collect::<Vec<_>>(),
        vec![
            "no agent named `missing` in this hamlet",
            "`feline-biologist` and `peer` already have a road",
            "every agent already has a road to the mayor"
        ]
    );
    let r = record(&store, "home");
    let agent = r.named("feline-biologist").unwrap();
    assert_eq!(
        (&*agent.purpose, agent.model.as_deref(), &*agent.character),
        ("Maps", Some("local::model"), "Careful")
    );
    assert_eq!(r.decisions.len(), 4);
    assert!(r.decisions.iter().all(|d| d.reason == "Needed"));
    assert_eq!(
        store.road_charges("home").unwrap()[0].task.as_deref(),
        Some("task-7")
    );
}

#[test]
fn road_refusals_caps_and_office_changes_preserve_traffic_without_refunds()
 {
    let (_scratch, mut store) = seeded("record-roads");
    for name in ["scout", "smith", "poet"] {
        done(&mut store, name, &request("home", admit(name)));
    }
    limits(&mut store, "limit", "home", |l| l.chief.most_roads = 1);
    assert_eq!(
        refused(
            &mut store,
            "self",
            &request("home", road("scout", "scout"))
        )
        .text(Audience::Agent),
        "a road needs two different agents"
    );
    assert_eq!(
        refused(
            &mut store,
            "office",
            &request("home", road("founder", "smith"))
        )
        .reason,
        Reason::OfficeRoad
    );
    done(
        &mut store,
        "road",
        &request("home", road("scout", "smith")),
    );
    assert_eq!(
        refused(
            &mut store,
            "duplicate-reversed",
            &request("home", road("smith", "scout"))
        )
        .reason,
        Reason::RoadExists {
            a: "smith".into(),
            b: "scout".into()
        }
    );
    assert_eq!(
        refused(
            &mut store,
            "cap",
            &request("home", road("scout", "poet"))
        )
        .reason,
        Reason::RoadsFull { most: 1 }
    );
    carry(&mut store, "out", 0, 1);
    carry(&mut store, "back", 1, 0);
    let r = record(&store, "home");
    let office = r.roads.iter().find(|r| r.a == End::Office).unwrap();
    assert_eq!(
        (office.messages, office.last_from, office.last_used),
        (2, Some(End::Agent(1)), 100)
    );
    assert_eq!(
        refused(
            &mut store,
            "take-office",
            &request(
                "home",
                Change::TakeUpRoad {
                    a: "mayor".into(),
                    b: "scout".into()
                }
            )
        )
        .text(Audience::Chief),
        "an agent's road to the mayor stays for as long as the \
         agent does"
    );
    done(
        &mut store,
        "seat",
        &request("home", Change::Seat { number: 1 }),
    );
    let r = record(&store, "home");
    assert_eq!(r.named("mayor").unwrap().name, "scout");
    assert_eq!(r.address_of(0), "founder");
    assert!(r.has_road(0, 1) && r.has_road(1, 3));
    assert!(!r.has_road(0, 2));
    assert!(
        r.roads_of()
            .iter()
            .all(|r| r.a != End::Agent(1) && r.b != End::Agent(1))
    );
    done(
        &mut store,
        "seat-back",
        &request("home", Change::Seat { number: 0 }),
    );
    done(
        &mut store,
        "take",
        &request(
            "home",
            Change::TakeUpRoad {
                a: "smith".into(),
                b: "scout".into(),
            },
        ),
    );
    assert!(!record(&store, "home").has_road(1, 2));
    let charges = store.road_charges("home").unwrap();
    assert_eq!(charges.len(), 1);
    assert_eq!(charges[0].amount.micros, 10_000);
}

#[test]
fn replay_after_reopening_returns_the_marked_result_even_after_authority_and_time_change()
 {
    let (scratch, mut store) = seeded("record-replay");
    for name in ["scout", "smith"] {
        done(&mut store, name, &request("home", admit(name)));
    }
    let mut call = request("home", road("scout", "smith"));
    call.actor = Actor::Mayor(place("home", 0));
    call.task = Some("task".into());
    let first = run(&mut store, "road", &call);
    done(
        &mut store,
        "seat",
        &request("home", Change::Seat { number: 1 }),
    );
    store.close().unwrap();
    let mut store =
        Store::open(&scratch.join("realm.sqlite")).unwrap();
    call.at = 99_999;
    assert_eq!(run(&mut store, "road", &call), first);
    assert_eq!(
        refused(&mut store, "new-road", &call).text(Audience::Agent),
        "Only the mayor may do that."
    );
    call.reason = "Different request".into();
    assert_eq!(
        store.run(&id("road"), &call).unwrap(),
        Reply::Conflict
    );
    assert_eq!(store.road_charges("home").unwrap().len(), 1);
    assert_eq!(
        record(&store, "home")
            .decisions
            .iter()
            .filter(|d| matches!(
                d.kind,
                DecisionKind::RoadBuilt { .. }
            ))
            .count(),
        1
    );
}

#[test]
fn restoring_keeps_identity_and_surviving_roads_not_namesakes_and_does_not_charge()
 {
    let (_scratch, mut store) = seeded("record-restore");
    found(&mut store, "mill", "miller");
    for name in ["scout", "smith", "poet", "scribe"] {
        done(&mut store, name, &request("home", admit(name)));
    }
    for (i, peer) in ["smith", "poet", "scribe"].iter().enumerate() {
        done(
            &mut store,
            &format!("road-{i}"),
            &request("home", road("scout", peer)),
        );
        carry(&mut store, &format!("carry-{i}"), 1, i as u64 + 2);
    }
    let archive = departed(&mut store, "depart", "scout");
    let departed_again = refused(
        &mut store,
        "depart-again",
        &request("home", remove("scout")),
    );
    assert!(
        departed_again
            .text(Audience::Agent)
            .contains("by the chief chief (\"Needed\")")
    );
    departed(&mut store, "depart-poet", "poet");
    done(&mut store, "new-poet", &request("home", admit("poet")));
    done(&mut store, "new-scout", &request("mill", admit("scout")));
    let taken = refused(
        &mut store,
        "taken",
        &request("home", restore(&archive, "scout")),
    );
    assert!(
        taken
            .text(Audience::Chief)
            .contains(&format!("/restore {archive} as <new-name>"))
    );
    limits(&mut store, "road-cap", "home", |l| {
        l.chief.most_roads = 0
    });
    let charges = store.road_charges("home").unwrap();
    let RecordResult::Restored { agent, text } = done(
        &mut store,
        "restore",
        &request("home", restore(&archive, "old-scout")),
    ) else {
        panic!()
    };
    assert_eq!(
        (agent.number, agent.since, &*agent.purpose),
        (1, 100, "Maps")
    );
    assert!(text.contains("more than the 0 your limits allow"));
    let r = record(&store, "home");
    assert!(r.has_road(1, 2) && r.has_road(1, 4) && r.has_road(1, 0));
    assert!(!r.has_road(1, 5));
    assert_eq!(r.next, 6);
    assert_eq!(r.name_of(1, NameFallback::Recorded), "old-scout");
    assert_eq!(store.road_charges("home").unwrap(), charges);
    assert_eq!(r.archive[0].back.as_deref(), Some("old-scout"));
    let mut historical = r;
    historical.agents.retain(|a| a.number != 1);
    historical.archive.retain(|a| a.agent.number != 1);
    assert_eq!(
        historical.name_of(1, NameFallback::Recorded),
        "old-scout"
    );
    assert_eq!(historical.name_of(1, NameFallback::Created), "scout");
    assert_eq!(
        historical.name_of(1, NameFallback::AgentNumber),
        "agent #1"
    );
}

#[test]
fn road_budget_wording_rounds_like_the_typescript_server() {
    let (_scratch, mut store) = seeded("record-money");
    for name in ["scout", "smith"] {
        done(&mut store, name, &request("home", admit(name)));
    }
    for (price, text) in [
        (124_999, "$0.12"),
        (125_000, "$0.13"),
        (125_001, "$0.13"),
        (15_000, "$0.01"),
    ] {
        limits(&mut store, &format!("limit-{price}"), "home", |l| {
            l.chief.road.cost = price as f64 / 1_000_000.0;
            l.road_budget = Some(RoadBudget {
                start: 0,
                end: 1000,
                month: false,
                other: Amount::default(),
                cost: Some(0),
                ms: None,
            });
        });
        assert_eq!(
            refused(
                &mut store,
                &format!("road-{price}"),
                &request("home", road("scout", "smith"))
            )
            .text(Audience::Chief),
            format!(
                "the hamlet cannot pay for it today: it costs \
                 {text} and would bring the hamlet to {text} of \
                 $0.00. Only the chief can raise the budget, on the \
                 hamlet's Bylaws page"
            )
        );
    }
    assert!(store.road_charges("home").unwrap().is_empty());
}

#[test]
fn road_budget_is_checked_with_the_charge_and_exhaustion_never_leaves_a_road()
 {
    let (_scratch, mut store) = seeded("record-budget");
    for name in ["scout", "smith", "poet"] {
        done(&mut store, name, &request("home", admit(name)));
    }
    limits(&mut store, "budget", "home", |l| {
        l.road_budget = Some(RoadBudget {
            start: 0,
            end: 1000,
            month: false,
            other: Amount::default(),
            cost: Some(15_000),
            ms: None,
        })
    });
    done(
        &mut store,
        "first",
        &request("home", road("scout", "smith")),
    );
    let refusal = refused(
        &mut store,
        "second",
        &request("home", road("scout", "poet")),
    );
    assert_eq!(
        refusal.text(Audience::Chief),
        "the hamlet cannot pay for it today: it costs $0.01 and \
         would bring the hamlet to $0.02 of $0.01. Only the chief \
         can raise the budget, on the hamlet's Bylaws page"
    );
    assert_eq!(store.road_charges("home").unwrap().len(), 1);
    limits(&mut store, "exact", "home", |l| {
        l.road_budget.as_mut().unwrap().cost = Some(20_000)
    });
    assert_eq!(
        refused(
            &mut store,
            "second",
            &request("home", road("scout", "poet"))
        ),
        refusal
    );
    done(
        &mut store,
        "fresh",
        &request("home", road("scout", "poet")),
    );
    let mut stale = request("home", road("smith", "poet"));
    stale.at = 1000;
    assert!(store.run(&id("expired-window"), &stale).is_err());
    limits(&mut store, "new-window", "home", |l| {
        let b = l.road_budget.as_mut().unwrap();
        b.start = 1000;
        b.end = 2000;
        b.cost = Some(10_000);
    });
    done(&mut store, "expired-window", &stale);
    assert_eq!(store.road_charges("home").unwrap().len(), 3);
    done(
        &mut store,
        "remove-last",
        &request(
            "home",
            Change::TakeUpRoad {
                a: "smith".into(),
                b: "poet".into(),
            },
        ),
    );
    limits(&mut store, "time-budget", "home", |l| {
        l.chief.road.minutes = 1;
        let b = l.road_budget.as_mut().unwrap();
        b.cost = None;
        b.ms = Some(150_000);
        b.other.ms = 120_000;
        b.month = true;
    });
    assert_eq!(
        refused(&mut store, "time-refused", &stale)
            .text(Audience::Chief),
        "the hamlet cannot pay for it this month: it costs 1m model \
         time and would bring the hamlet to 3m model time of 2m \
         model time. Only the chief can raise the budget, on the \
         hamlet's Bylaws page"
    );
    assert_eq!(store.road_charges("home").unwrap().len(), 3);
}

#[test]
fn current_vote_limits_and_chief_tasks_hold_effects_but_chiefs_can_overrule()
 {
    let (_scratch, mut store) = seeded("record-guards");
    done(&mut store, "scout", &request("home", admit("scout")));
    let hold = Change::SetGuards {
        open_votes: vec![OpenVote {
            id: 7,
            called: 100,
            motion: "replace the mayor with scout".into(),
        }],
        protected: vec![Protection {
            agent: 1,
            chief: "chief".into(),
            text: "Test it".into(),
            done: None,
        }],
    };
    done(&mut store, "guards", &request("home", hold));
    let mut call = request("home", remove("scout"));
    call.actor = Actor::Mayor(place("home", 0));
    let refusal = refused(&mut store, "held", &call);
    assert_eq!(
        refusal.text(Audience::Agent),
        "vote 7 is open (replace the mayor with scout); nothing \
         else is decided in this hamlet until it closes. Try again \
         then"
    );
    assert!(
        record(&store, "home")
            .may_decide(3_600_099, Action::Admit)
            .is_some()
    );
    assert!(
        record(&store, "home")
            .may_decide(3_600_100, Action::Admit)
            .is_none()
    );
    limits(&mut store, "expire-vote", "home", |l| {
        l.chief.votes.open_minutes = 0
    });
    call.reason = " ".into();
    assert_eq!(
        refused(&mut store, "blank", &call).reason,
        Reason::ReasonEmpty
    );
    call.reason = "Done".into();
    assert!(matches!(
        refused(&mut store, "protected", &call).reason,
        Reason::Protected { done: None, .. }
    ));
    assert!(record(&store, "home").archive.is_empty());
    assert_eq!(
        refused(
            &mut store,
            "office",
            &request("home", remove("mayor"))
        )
        .reason,
        Reason::MayorUntouchable
    );
    call.actor = Actor::Chief("stranger".into());
    assert_eq!(
        refused(&mut store, "stranger", &call).reason,
        Reason::RecordChief
    );
    call.actor = Actor::Chief("chief".into());
    let RecordResult::Departed { archive } =
        done(&mut store, "overrule", &call)
    else {
        panic!("expected the chief's removal")
    };
    done(
        &mut store,
        "bring-back",
        &request("home", restore(&archive, "scout")),
    );
    call.actor = Actor::Mayor(place("home", 0));
    assert!(matches!(
        refused(&mut store, "remove-restored", &call).reason,
        Reason::Protected { done: None, .. }
    ));
    assert!(record(&store, "home").named("scout").is_some());
}

#[test]
fn moving_requires_both_current_mayors_and_uses_the_receiving_next_number()
 {
    let (_scratch, mut store) = seeded("record-move");
    found(&mut store, "mill", "miller");
    for name in ["scout", "smith"] {
        done(&mut store, name, &request("home", admit(name)));
    }
    for name in ["cook", "poet"] {
        done(&mut store, name, &request("mill", admit(name)));
    }
    done(
        &mut store,
        "road",
        &request("home", road("scout", "smith")),
    );
    done(
        &mut store,
        "protect-mover",
        &request(
            "home",
            Change::SetGuards {
                open_votes: Vec::new(),
                protected: vec![Protection {
                    agent: 1,
                    chief: "chief".into(),
                    text: "Finish the map".into(),
                    done: None,
                }],
            },
        ),
    );
    let mut call = request(
        "home",
        Change::MoveMember {
            number: 1,
            to: "mill".into(),
        },
    );
    call.actor = Actor::Mayors {
        leaving: place("home", 0),
        joining: place("mill", 0),
    };
    done(
        &mut store,
        "seat",
        &request("mill", Change::Seat { number: 2 }),
    );
    assert_eq!(
        refused(&mut store, "stale-mayor", &call).reason,
        Reason::NotMayor
    );
    call.actor = Actor::Mayors {
        leaving: place("home", 0),
        joining: place("mill", 2),
    };
    limits(&mut store, "cap", "mill", |l| l.chief.most_agents = 2);
    assert_eq!(
        refused(&mut store, "full", &call).text(Audience::Agent),
        "the hamlet `mill` is full"
    );
    limits(&mut store, "room", "mill", |l| l.chief.most_agents = 3);
    assert_eq!(
        done(&mut store, "move", &call),
        RecordResult::Moved(place("mill", 3))
    );
    assert_eq!(
        store.agent_named("scout").unwrap(),
        Some(place("mill", 3))
    );
    let old = record(&store, "home");
    let target = record(&store, "mill");
    assert_eq!((old.next, target.next), (3, 4));
    assert!(!old.has_road(1, 2));
    assert!(target.has_road(2, 3));
    assert_eq!(target.named("scout").unwrap().character, "Careful");
    assert!(old.protected.is_empty());
    assert_eq!(
        target.protected,
        vec![Protection {
            agent: 3,
            chief: "chief".into(),
            text: "Finish the map".into(),
            done: None
        }]
    );
    assert!(matches!(
        old.decisions.last().unwrap().kind,
        DecisionKind::MovedOut { agent: 1, .. }
    ));
    assert!(matches!(
        target.decisions.last().unwrap().kind,
        DecisionKind::MovedIn { agent: 3, .. }
    ));
    assert_eq!(
        refused(
            &mut store,
            "move-mayor",
            &request(
                "home",
                Change::MoveMember {
                    number: 0,
                    to: "mill".into()
                }
            )
        )
        .reason,
        Reason::MoveMayor
    );
}

#[test]
fn a_failed_marker_rolls_back_admission_roads_charges_and_the_name_index()
 {
    let (_scratch, mut store) = seeded("record-atomic");
    done(&mut store, "peer", &request("home", admit("peer")));
    let call = request(
        "home",
        Change::Admit(Admit {
            name: "scout".into(),
            purpose: "Maps".into(),
            model: None,
            character: String::new(),
            roads: vec!["peer".into()],
        }),
    );
    store
        .connection()
        .execute_batch(
            "CREATE TEMP TRIGGER fail_marker BEFORE INSERT ON \
             marker WHEN NEW.key = 'fail' BEGIN SELECT RAISE(ABORT, \
             'injected marker failure'); END;",
        )
        .unwrap();
    assert!(store.run(&id("fail"), &call).is_err());
    assert_eq!(store.agent_named("scout").unwrap(), None);
    assert_eq!(record(&store, "home").next, 2);
    assert!(store.road_charges("home").unwrap().is_empty());
    store
        .connection()
        .execute_batch("DROP TRIGGER fail_marker")
        .unwrap();
    done(&mut store, "fail", &call);
    assert_eq!(
        store.agent_named("scout").unwrap(),
        Some(place("home", 2))
    );
    assert_eq!(store.road_charges("home").unwrap().len(), 1);
}

#[test]
fn merge_restoration_preserves_learned_traffic_and_enforces_reverse_order()
 {
    let (_scratch, mut store) = seeded("record-merge");
    found(&mut store, "mill", "miller");
    for name in ["scout", "smith", "poet", "scribe"] {
        done(&mut store, name, &request("home", admit(name)));
    }
    done(
        &mut store,
        "scout-road",
        &request("home", road("scout", "scribe")),
    );
    carry(&mut store, "scout-message", 1, 4);
    let merge = |name: &str| {
        request(
            "home",
            Change::Depart {
                name: name.into(),
                how: Departure::Merged {
                    into: 2,
                    purpose: Some("Both".into()),
                    notes: MergeNotes::Joined,
                    files: MergeFiles {
                        notes_before: "Before".into(),
                        notes_after: "After".into(),
                        brought: Vec::new(),
                        clashes: Vec::new(),
                    },
                },
            },
        )
    };
    let mut unauthorized = merge("scout");
    unauthorized.actor = Actor::Mayor(place("home", 3));
    assert_eq!(
        refused(&mut store, "not-mayor", &unauthorized)
            .text(Audience::Agent),
        "Only the mayor can merge agents."
    );
    let RecordResult::Departed { archive } =
        done(&mut store, "merge-scout", &merge("scout"))
    else {
        panic!()
    };
    assert_eq!(
        refused(
            &mut store,
            "unfinished",
            &request("home", restore(&archive, "scout"))
        )
        .reason,
        Reason::MergeUnfinished
    );
    done(
        &mut store,
        "finish-scout",
        &request(
            "home",
            Change::FinishMerge {
                archive: archive.clone(),
            },
        ),
    );
    carry(&mut store, "learned", 2, 4);
    let RecordResult::Departed { archive: later } =
        done(&mut store, "merge-poet", &merge("poet"))
    else {
        panic!()
    };
    done(
        &mut store,
        "finish-poet",
        &request(
            "home",
            Change::FinishMerge {
                archive: later.clone(),
            },
        ),
    );
    assert!(matches!(
        refused(
            &mut store,
            "wrong-order",
            &request("home", restore(&archive, "scout"))
        )
        .reason,
        Reason::LaterMerge { .. }
    ));
    assert!(matches!(
        refused(
            &mut store,
            "move-kept",
            &request(
                "home",
                Change::MoveMember {
                    number: 2,
                    to: "mill".into()
                }
            )
        )
        .reason,
        Reason::UnsplitMerge { .. }
    ));
    let charges = store.road_charges("home").unwrap();
    done(
        &mut store,
        "split-poet",
        &request("home", restore(&later, "poet")),
    );
    limits(&mut store, "full", "home", |l| l.chief.most_agents = 3);
    assert!(matches!(
        refused(
            &mut store,
            "split-full",
            &request("home", restore(&archive, "scout"))
        )
        .reason,
        Reason::HamletFull { most: 3 }
    ));
    limits(&mut store, "room", "home", |l| l.chief.most_agents = 4);
    done(
        &mut store,
        "split-scout",
        &request("home", restore(&archive, "scout")),
    );
    let r = record(&store, "home");
    assert_eq!(r.named("smith").unwrap().purpose, "Maps");
    assert!(r.has_road(1, 4) && r.has_road(2, 4));
    assert_eq!(
        r.roads
            .iter()
            .find(|road| road.a == End::Agent(2)
                && road.b == End::Agent(4))
            .unwrap()
            .messages,
        1
    );
    assert_eq!(store.road_charges("home").unwrap(), charges);
    for archive in [archive, later] {
        done(
            &mut store,
            &format!("finish-split-{archive}"),
            &request(
                "home",
                Change::FinishMerge {
                    archive: archive.clone(),
                },
            ),
        );
        let r = record(&store, "home");
        let entry = r
            .archive
            .iter()
            .find(|entry| entry.id == archive)
            .unwrap();
        assert!(entry.back.is_some());
        assert!(entry.merge.as_ref().unwrap().applied);
    }
}
