//! The city of scholars, the hamlets founded in it and the hires in
//! their trees, by a chief's commands.

use hamlet_acceptance::*;
use hamlet_protocol::client::*;

#[tokio::test]
async fn a_new_realm_holds_the_hand_and_an_empty_city() {
    let mut s = Session::start(Faux::new()).await;
    let realm = s.chief.realm();
    assert_eq!(realm.agents.len(), 1);
    let hand = &realm.agents[0];
    assert_eq!(hand.id, realm.hand);
    assert_eq!(hand.kind, AgentKind::Hand);
    assert_eq!(hand.name, "hand");
    assert!(realm.scholars.is_empty() && realm.hamlets.is_empty());
    let hand = s.hand();
    assert_eq!(
        s.chief.command(hand, "/hamlets").await,
        "No hamlet yet. /found makes one."
    );
    assert_eq!(
        s.chief.command(hand, "/scholars").await,
        "The city has no scholar yet."
    );
}

#[tokio::test]
async fn founds_a_hamlet_under_a_new_scholar_then_an_existing_one() {
    let mut s = Session::start(Faux::new()).await;
    let hand = s.hand();
    let ada = s.found("docs", "ada", "keep the docs").await;
    let realm = s.chief.realm();
    let hamlet = s.chief.hamlet("docs").unwrap();
    assert_eq!(hamlet.mayor, ada);
    assert_eq!(hamlet.chiefs, ["chief"]);
    assert_eq!(hamlet.mode, HamletMode::Manual);
    let mayor = s.chief.agent("docs#0").unwrap();
    assert!(mayor.mayor);
    assert_eq!(mayor.kind, AgentKind::Hire);
    assert_eq!(mayor.parent, Some(hand));
    assert_eq!(mayor.purpose, "keep the docs");
    let scholar = s.chief.scholar("ada").unwrap();
    assert_eq!(scholar.hires, [ada]);
    assert_eq!(mayor.scholar, Some(scholar.id));
    assert!(hamlet.roads.iter().any(|r| {
        r.kind == RoadKind::Tree && r.a == hand && r.b == ada
    }));
    assert_eq!(realm.scholars.len(), 1);

    // The same scholar may mayor a second hamlet.
    let second = s.found("mill", "ada", "grind").await;
    assert_ne!(second, ada);
    assert_eq!(s.chief.scholar("ada").unwrap().hires.len(), 2);
    assert_eq!(
        s.chief.command(hand, "/hamlets").await,
        "- docs (`docs`)\n- mill (`mill`)"
    );

    // Ids are never used twice, and names are tidy.
    let scholar = format!("scholar#{}", scholar.id.0);
    let reply = s
        .chief
        .refused(hand, &format!("/found docs {scholar} again"))
        .await;
    assert!(reply.contains("was used before"), "{reply}");
    let reply = s
        .chief
        .refused(hand, &format!("/found Mill {scholar} x"))
        .await;
    assert!(reply.contains("is not a hamlet's name"), "{reply}");
}

#[tokio::test]
async fn hires_form_a_tree_and_ending_one_can_be_undone() {
    let mut s = Session::start(Faux::new()).await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    let bob = s.hire(ada, "bob", "write tests").await;
    assert_eq!(s.chief.agent("docs#1").unwrap().parent, Some(ada));

    // The same scholar twice is two hires, told apart by number.
    let deep = s.hire(ada, "ada", "more words").await;
    assert_eq!(s.chief.agent("docs#2").unwrap().id, deep);
    let reply = s.chief.refused(ada, "/end ada gone").await;
    assert!(reply.contains("is not an address"), "{reply}");

    let eve = s.hire_under(ada, "eve", "docs#2", "deep work").await;
    assert_eq!(s.chief.agent("docs#3").unwrap().id, eve);
    assert_eq!(s.chief.agent("docs#3").unwrap().parent, Some(deep));

    // A mayor cannot be ended, and a lead's hires move up.
    let reply = s.chief.refused(ada, "/end docs#0 no").await;
    assert!(reply.contains("seat another mayor first"), "{reply}");
    s.chief.command(ada, "/end docs#2 restructure").await;
    let ended = s.chief.agent("docs#2").unwrap();
    let left = ended.left.unwrap();
    assert_eq!(left.how, LeftHow::Ended);
    assert_eq!(left.by, "chief");
    assert_eq!(left.reason, "restructure");
    assert_eq!(s.chief.agent("docs#3").unwrap().parent, Some(ada));
    assert_eq!(s.chief.scholar("bob").unwrap().hires, [bob]);

    // /restore brings back an ended hire, under the same id.
    s.chief.command(ada, "/end docs#1 done").await;
    s.chief.command(ada, "/restore docs#1").await;
    s.chief
        .until("bob back", |c| {
            c.agent("docs#1").is_some_and(|a| a.left.is_none())
        })
        .await;
    assert_eq!(s.chief.id("docs#1"), bob);

    // Only an ended hire is erased.
    s.chief.refused(ada, "/erase docs#3").await;
    s.chief.command(ada, "/restore docs#2").await;
    assert!(s.chief.agent("docs#2").unwrap().left.is_none());
    s.chief.command(ada, "/end docs#3 x").await;
    s.chief.command(ada, "/erase docs#3").await;
    s.chief
        .until("eve erased", |c| {
            c.realm().agents.iter().all(|a| a.id != eve)
        })
        .await;
}

#[tokio::test]
async fn the_city_keeps_scholars_until_they_are_erased() {
    let mut s = Session::start(Faux::new()).await;
    let hand = s.hand();
    let ada = s.found("docs", "ada", "keep the docs").await;
    s.chief
        .command(hand, "/scholar new sam: A careful reader.")
        .await;
    let sam = s.chief.scholar("sam").unwrap();
    assert!(sam.left.is_none() && sam.hires.is_empty());
    // A name is a label: it repeats, and may hold spaces.
    s.chief.command(hand, "/scholar new ada").await;
    s.chief
        .command(hand, "/scholar new Sam the Elder: Slow and sure.")
        .await;
    let realm = s.chief.realm();
    let named = |name: &str| {
        realm.scholars.iter().filter(|s| s.name == name).count()
    };
    assert_eq!((named("ada"), named("Sam the Elder")), (2, 1));
    let reply = s.chief.refused(hand, "/scholar new : no name").await;
    assert!(reply.contains("1 to 40 characters"), "{reply}");

    // Hiring a scholar of the city, then removing it ends its hires.
    let hire = s.hire(ada, "sam", "read the docs").await;
    let sam = format!("scholar#{}", sam.id.0);
    s.chief
        .command(hand, &format!("/remove-scholar {sam} moved away"))
        .await;
    s.chief
        .until("sam archived", |c| {
            c.scholar("sam").is_some_and(|s| s.left.is_some())
        })
        .await;
    let left = s.chief.agent("docs#1").unwrap().left.unwrap();
    assert_eq!(left.how, LeftHow::Scholar);
    assert!(
        s.chief
            .command(hand, "/scholars")
            .await
            .contains("(sam) (archived)")
    );

    // Restoring brings the scholar and the hires its removal ended.
    s.chief
        .command(hand, &format!("/restore-scholar {sam}"))
        .await;
    s.chief
        .until("sam back", |c| {
            c.scholar("sam").is_some_and(|s| s.left.is_none())
                && c.agent("docs#1").is_some_and(|a| a.left.is_none())
        })
        .await;
    assert_eq!(s.chief.id("docs#1"), hire);

    // A scholar must be removed before it is erased.
    s.chief
        .refused(hand, &format!("/erase-scholar {sam}"))
        .await;
    s.chief
        .command(hand, &format!("/remove-scholar {sam} again"))
        .await;
    s.chief
        .command(hand, &format!("/erase-scholar {sam}"))
        .await;
    s.chief
        .until("sam gone", |c| c.scholar("sam").is_none())
        .await;
}

#[tokio::test]
async fn a_chief_runs_a_hamlet_by_commands() {
    let mut s = Session::start(Faux::new()).await;
    let hand = s.hand();
    let ada = s.found("docs", "ada", "keep the docs").await;
    let bob = s.hire(ada, "bob", "write").await;
    let folder = s.home.side().display().to_string();

    s.chief.command(ada, "/name-hamlet docs The Docs").await;
    s.chief.command(ada, "/move-hamlet docs 3 4").await;
    s.chief.command(ada, "/tier 100").await;
    s.chief.command(ada, "/mode auto").await;
    s.chief.command(ada, &format!("/folder {folder}")).await;
    s.chief
        .until("the hamlet's settings", |c| {
            c.hamlet("docs").is_some_and(|h| {
                h.name == "The Docs"
                    && h.tier == 100
                    && h.mode == HamletMode::Auto
                    && h.folder.as_deref() == Some(folder.as_str())
                    && h.at == Some(TownAt { x: 3.0, z: 4.0 })
            })
        })
        .await;
    s.chief.refused(ada, "/tier 7").await;
    let reply = s.chief.refused(ada, "/folder /no/such/folder").await;
    assert!(reply.contains("/no/such/folder"), "{reply}");
    s.chief.command(ada, "/folder none").await;

    // A road is laid between two hires and taken up again.
    let eve = s.hire(ada, "eve", "write").await;
    s.chief
        .command(ada, "/road docs#2 docs#1 they share a file")
        .await;
    s.chief
        .until("the laid road", |c| {
            c.hamlet("docs").unwrap().roads.iter().any(|r| {
                r.kind == RoadKind::Laid && [r.a, r.b] == [bob, eve]
            })
        })
        .await;
    s.chief.command(ada, "/unroad docs#2 docs#1 done").await;
    s.chief
        .until("the road gone", |c| {
            c.hamlet("docs")
                .unwrap()
                .roads
                .iter()
                .all(|r| r.kind != RoadKind::Laid)
        })
        .await;
    // The mayor's office passes to another hire.
    s.chief.command(ada, "/mayor docs#1 handing over").await;
    s.chief
        .until("bob mayor", |c| {
            c.hamlet("docs").unwrap().mayor == bob
        })
        .await;
    assert!(s.chief.agent("docs#1").unwrap().mayor);
    assert!(!s.chief.agent("docs#0").unwrap().mayor);
    assert_eq!(s.chief.agent("docs#0").unwrap().parent, Some(bob));

    // Removing a hamlet ends its hires; restoring brings them back.
    s.chief.command(hand, "/remove-hamlet docs finished").await;
    s.chief
        .until("hamlet removed", |c| {
            c.hamlet("docs").is_none()
                && c.realm().closed.iter().any(|h| h.id == "docs")
        })
        .await;
    assert_eq!(
        s.chief.agent("docs#1").unwrap().left.unwrap().how,
        LeftHow::Hamlet
    );
    s.chief.command(hand, "/restore-hamlet docs").await;
    s.chief
        .until("hamlet back", |c| {
            c.hamlet("docs").is_some()
                && c.agent("docs#1").is_some_and(|a| a.left.is_none())
        })
        .await;
    s.chief.command(hand, "/remove-hamlet docs again").await;
    s.chief.command(hand, "/erase-hamlet docs").await;
    s.chief
        .until("hamlet erased", |c| {
            c.realm().closed.iter().all(|h| h.id != "docs")
        })
        .await;
}
