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
    assert_eq!(hamlet.chiefs, ["nixon"]);
    assert_eq!(hamlet.mode, HamletMode::Manual);
    let mayor = s.chief.agent("ada@docs").unwrap();
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
    let reply = s.chief.refused(hand, "/found docs ada again").await;
    assert!(reply.contains("was used before"), "{reply}");
    let reply = s.chief.refused(hand, "/found Mill ada x").await;
    assert!(reply.contains("is not a hamlet's name"), "{reply}");
}

#[tokio::test]
async fn hires_form_a_tree_and_ending_one_can_be_undone() {
    let mut s = Session::start(Faux::new()).await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    s.chief.command(ada, "/hire bob write tests").await;
    let bob = s.chief.id("bob@docs");
    assert_eq!(s.chief.agent("bob@docs").unwrap().parent, Some(ada));

    // The same scholar twice needs numbers to tell them apart.
    let reply = s.chief.command(ada, "/hire ada more words").await;
    assert_eq!(reply, "`ada#2@docs` is hired.");
    let reply = s.chief.refused(ada, "/end ada gone").await;
    assert!(reply.contains("`ada#0@docs`, `ada#2@docs`"), "{reply}");
    let deep = s.chief.id("ada#2@docs");

    s.chief
        .command(ada, "/hire eve under ada#2 deep work")
        .await;
    let eve = s.chief.id("eve@docs");
    assert_eq!(s.chief.agent("eve@docs").unwrap().parent, Some(deep));

    // A mayor cannot be ended, and a lead's hires move up.
    let reply = s.chief.refused(ada, "/end ada#0 no").await;
    assert!(reply.contains("seat another mayor first"), "{reply}");
    s.chief.command(ada, "/end ada#2 restructure").await;
    let ended = s.chief.agent("ada#2@docs").unwrap();
    let left = ended.left.unwrap();
    assert_eq!(left.how, LeftHow::Ended);
    assert_eq!(left.by, "nixon");
    assert_eq!(left.reason, "restructure");
    assert_eq!(s.chief.agent("eve@docs").unwrap().parent, Some(ada));
    assert_eq!(s.chief.scholar("bob").unwrap().hires, [bob]);

    // /undo brings back the hire ended last, under the same id.
    s.chief.command(ada, "/end bob done").await;
    assert_eq!(
        s.chief.command(ada, "/undo").await,
        "bob@docs is back."
    );
    s.chief
        .until("bob back", |c| {
            c.agent("bob@docs").is_some_and(|a| a.left.is_none())
        })
        .await;
    assert_eq!(s.chief.id("bob@docs"), bob);

    // Only an ended hire is erased.
    s.chief.refused(ada, "/erase eve").await;
    s.chief.command(ada, "/restore ada#2").await;
    assert!(s.chief.agent("ada#2@docs").unwrap().left.is_none());
    s.chief.command(ada, "/end eve x").await;
    s.chief.command(ada, "/erase eve").await;
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
        .command(hand, "/scholar new sam A careful reader.")
        .await;
    let sam = s.chief.scholar("sam").unwrap();
    assert!(sam.left.is_none() && sam.hires.is_empty());
    let reply = s.chief.refused(hand, "/scholar new sam").await;
    assert!(reply.contains("belongs to a living scholar"), "{reply}");
    let reply = s.chief.refused(hand, "/scholar new hand").await;
    assert!(reply.contains("is reserved"), "{reply}");

    // Hiring a scholar of the city, then removing it ends its hires.
    s.chief.command(ada, "/hire sam read the docs").await;
    let hire = s.chief.id("sam@docs");
    s.chief
        .command(hand, "/remove-scholar sam moved away")
        .await;
    s.chief
        .until("sam archived", |c| {
            c.scholar("sam").is_some_and(|s| s.left.is_some())
        })
        .await;
    let left = s.chief.agent("sam@docs").unwrap().left.unwrap();
    assert_eq!(left.how, LeftHow::Scholar);
    assert!(
        s.chief
            .command(hand, "/scholars")
            .await
            .contains("sam (archived)")
    );

    // Restoring brings the scholar and the hires its removal ended.
    s.chief.command(hand, "/restore-scholar sam").await;
    s.chief
        .until("sam back", |c| {
            c.scholar("sam").is_some_and(|s| s.left.is_none())
                && c.agent("sam@docs")
                    .is_some_and(|a| a.left.is_none())
        })
        .await;
    assert_eq!(s.chief.id("sam@docs"), hire);

    // A scholar must be removed before it is erased.
    s.chief.refused(hand, "/erase-scholar sam").await;
    s.chief.command(hand, "/remove-scholar sam again").await;
    s.chief.command(hand, "/erase-scholar sam").await;
    s.chief
        .until("sam gone", |c| c.scholar("sam").is_none())
        .await;
}

#[tokio::test]
async fn a_chief_runs_a_hamlet_by_commands() {
    let mut s = Session::start(Faux::new()).await;
    let hand = s.hand();
    let ada = s.found("docs", "ada", "keep the docs").await;
    s.chief.command(ada, "/hire bob write").await;
    let bob = s.chief.id("bob@docs");
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
    s.chief.command(ada, "/hire eve write").await;
    let eve = s.chief.id("eve@docs");
    s.chief
        .command(ada, "/road eve bob they share a file")
        .await;
    s.chief
        .until("the laid road", |c| {
            c.hamlet("docs").unwrap().roads.iter().any(|r| {
                r.kind == RoadKind::Laid && [r.a, r.b] == [bob, eve]
            })
        })
        .await;
    s.chief.command(ada, "/unroad eve bob done").await;
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
    s.chief.command(ada, "/mayor bob handing over").await;
    s.chief
        .until("bob mayor", |c| {
            c.hamlet("docs").unwrap().mayor == bob
        })
        .await;
    assert!(s.chief.agent("bob@docs").unwrap().mayor);
    assert!(!s.chief.agent("ada@docs").unwrap().mayor);
    assert_eq!(s.chief.agent("ada@docs").unwrap().parent, Some(bob));

    // Removing a hamlet ends its hires; restoring brings them back.
    s.chief.command(hand, "/remove-hamlet docs finished").await;
    s.chief
        .until("hamlet removed", |c| {
            c.hamlet("docs").is_none()
                && c.realm().closed.iter().any(|h| h.id == "docs")
        })
        .await;
    assert_eq!(
        s.chief.agent("bob@docs").unwrap().left.unwrap().how,
        LeftHow::Hamlet
    );
    s.chief.command(hand, "/restore-hamlet docs").await;
    s.chief
        .until("hamlet back", |c| {
            c.hamlet("docs").is_some()
                && c.agent("bob@docs")
                    .is_some_and(|a| a.left.is_none())
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
