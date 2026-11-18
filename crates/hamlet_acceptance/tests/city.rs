//! The city of profiles, the hamlets founded in it and the agents in
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
    assert_eq!(hand.address, "hand");
    assert!(realm.profiles.is_empty() && realm.hamlets.is_empty());
    let hand = s.hand();
    assert_eq!(
        s.chief.command(hand, "/hamlets").await,
        "No hamlet yet. /create-hamlet makes one."
    );
    assert_eq!(
        s.chief.command(hand, "/profiles").await,
        "The city has no profile yet."
    );
}

#[tokio::test]
async fn founds_a_hamlet_under_a_new_profile_then_an_existing_one() {
    let mut s = Session::start(Faux::new()).await;
    let hand = s.hand();
    let ada = s.found("docs", "ada", "keep the docs").await;
    let realm = s.chief.realm();
    let hamlet = s.chief.hamlet("docs").unwrap();
    assert_eq!(hamlet.lead, ada);
    assert_eq!(hamlet.chiefs, ["chief"]);
    assert_eq!(hamlet.mode, HamletMode::Manual);
    let lead = s.chief.agent("agent#2").unwrap();
    assert!(lead.lead);
    assert_eq!(lead.kind, AgentKind::Hamlet);
    assert_eq!(lead.parent, Some(hand));
    assert_eq!(lead.purpose, "keep the docs");
    let profile = s.chief.profile("ada").unwrap();
    assert_eq!(profile.agents, [ada]);
    assert_eq!(lead.profile, Some(profile.id));
    assert!(hamlet.channels.iter().any(|r| {
        r.kind == ChannelKind::Tree && r.a == hand && r.b == ada
    }));
    assert_eq!(realm.profiles.len(), 1);

    // The same profile may lead a second hamlet.
    let second = s.found("mill", "ada", "grind").await;
    assert_ne!(second, ada);
    assert_eq!(s.chief.profile("ada").unwrap().agents.len(), 2);
    let (docs, mill) =
        (s.chief.hamlet_id("docs"), s.chief.hamlet_id("mill"));
    assert_ne!(docs, "docs");
    assert_eq!(
        s.chief.command(hand, "/hamlets").await,
        format!("- docs (`{docs}`)\n- mill (`{mill}`)")
    );

    // A name is a label: it may hold spaces, but a hamlet needs one.
    let profile = format!("profile#{}", profile.id.0);
    s.chief
        .command(
            hand,
            &format!("/create-hamlet {profile} Old Mill: grind"),
        )
        .await;
    s.chief
        .until("Old Mill", |c| c.hamlet("Old Mill").is_some())
        .await;
    let reply = s
        .chief
        .rejected(
            hand,
            &format!("/create-hamlet {profile} : nameless"),
        )
        .await;
    assert!(reply.contains("/create-hamlet"), "{reply}");
}

#[tokio::test]
async fn agents_form_a_tree_and_ending_one_can_be_undone() {
    let mut s = Session::start(Faux::new()).await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    let bob = s.add_agent(ada, "bob", "write tests").await;
    assert_eq!(s.chief.agent("agent#3").unwrap().parent, Some(ada));

    // The same profile twice is two agents, told apart by id.
    let deep = s.add_agent(ada, "ada", "more words").await;
    assert_eq!(s.chief.agent("agent#4").unwrap().id, deep);
    let reply = s.chief.rejected(ada, "/end ada gone").await;
    assert!(reply.contains("is not an address"), "{reply}");

    let eve =
        s.add_agent_under(ada, "eve", "agent#4", "deep work").await;
    assert_eq!(s.chief.agent("agent#5").unwrap().id, eve);
    assert_eq!(s.chief.agent("agent#5").unwrap().parent, Some(deep));

    // A lead cannot be ended, and a parent's agents move up.
    let reply = s.chief.rejected(ada, "/end agent#2 no").await;
    assert!(reply.contains("set another lead first"), "{reply}");
    s.chief.command(ada, "/end agent#4 restructure").await;
    let ended = s.chief.agent("agent#4").unwrap();
    let left = ended.left.unwrap();
    assert_eq!(left.how, LeftHow::Ended);
    assert_eq!(left.by, "chief");
    assert_eq!(left.reason, "restructure");
    assert_eq!(s.chief.agent("agent#5").unwrap().parent, Some(ada));
    assert_eq!(s.chief.profile("bob").unwrap().agents, [bob]);

    // /restore brings back an ended agent, under the same id.
    s.chief.command(ada, "/end agent#3 done").await;
    s.chief.command(ada, "/restore agent#3").await;
    s.chief
        .until("bob back", |c| {
            c.agent("agent#3").is_some_and(|a| a.left.is_none())
        })
        .await;
    assert_eq!(s.chief.id("agent#3"), bob);

    // Only an ended agent is erased.
    s.chief.rejected(ada, "/erase agent#5").await;
    s.chief.command(ada, "/restore agent#4").await;
    assert!(s.chief.agent("agent#4").unwrap().left.is_none());
    s.chief.command(ada, "/end agent#5 x").await;
    s.chief.command(ada, "/erase agent#5").await;
    s.chief
        .until("eve erased", |c| {
            c.realm().agents.iter().all(|a| a.id != eve)
        })
        .await;
}

#[tokio::test]
async fn the_city_keeps_profiles_until_they_are_erased() {
    let mut s = Session::start(Faux::new()).await;
    let hand = s.hand();
    let ada = s.found("docs", "ada", "keep the docs").await;
    s.chief
        .command(hand, "/profile new sam: A careful reader.")
        .await;
    let sam = s.chief.profile("sam").unwrap();
    assert!(sam.left.is_none() && sam.agents.is_empty());
    // A name is a label: it repeats, and may hold spaces.
    s.chief.command(hand, "/profile new ada").await;
    s.chief
        .command(hand, "/profile new Sam the Elder: Slow and sure.")
        .await;
    let realm = s.chief.realm();
    let named = |name: &str| {
        realm.profiles.iter().filter(|s| s.name == name).count()
    };
    assert_eq!((named("ada"), named("Sam the Elder")), (2, 1));
    let reply =
        s.chief.rejected(hand, "/profile new : no name").await;
    assert!(reply.contains("1 to 40 characters"), "{reply}");

    // Adding an agent of a profile of the city, then removing the
    // profile ends its agents.
    let agent = s.add_agent(ada, "sam", "read the docs").await;
    let sam = format!("profile#{}", sam.id.0);
    s.chief
        .command(hand, &format!("/remove-profile {sam} moved away"))
        .await;
    s.chief
        .until("sam archived", |c| {
            c.profile("sam").is_some_and(|s| s.left.is_some())
        })
        .await;
    let left = s.chief.agent("agent#3").unwrap().left.unwrap();
    assert_eq!(left.how, LeftHow::Profile);
    assert!(
        s.chief
            .command(hand, "/profiles")
            .await
            .contains("(sam) (archived)")
    );

    // Restoring brings the profile and the agents its removal ended.
    s.chief
        .command(hand, &format!("/restore-profile {sam}"))
        .await;
    s.chief
        .until("sam back", |c| {
            c.profile("sam").is_some_and(|s| s.left.is_none())
                && c.agent("agent#3")
                    .is_some_and(|a| a.left.is_none())
        })
        .await;
    assert_eq!(s.chief.id("agent#3"), agent);

    // A profile must be removed before it is erased.
    s.chief
        .rejected(hand, &format!("/erase-profile {sam}"))
        .await;
    s.chief
        .command(hand, &format!("/remove-profile {sam} again"))
        .await;
    s.chief
        .command(hand, &format!("/erase-profile {sam}"))
        .await;
    s.chief
        .until("sam gone", |c| c.profile("sam").is_none())
        .await;
}

#[tokio::test]
async fn a_chief_runs_a_hamlet_by_commands() {
    let mut s = Session::start(Faux::new()).await;
    let hand = s.hand();
    let ada = s.found("docs", "ada", "keep the docs").await;
    let bob = s.add_agent(ada, "bob", "write").await;
    let folder = s.home.side().display().to_string();
    let docs = s.chief.hamlet_id("docs");

    s.chief
        .command(ada, &format!("/name-hamlet {docs} The Docs"))
        .await;
    s.chief
        .command(ada, &format!("/move-hamlet {docs} 3 4"))
        .await;
    s.chief.command(ada, "/tier 100").await;
    s.chief.command(ada, "/mode auto").await;
    s.chief.command(ada, &format!("/folder {folder}")).await;
    s.chief
        .until("the hamlet's settings", |c| {
            c.hamlet("The Docs").is_some_and(|h| {
                h.name == "The Docs"
                    && h.tier == 100
                    && h.mode == HamletMode::Auto
                    && h.folder.as_deref() == Some(folder.as_str())
                    && h.at == Some(TownAt { x: 3.0, z: 4.0 })
            })
        })
        .await;
    s.chief.rejected(ada, "/tier 7").await;
    let reply =
        s.chief.rejected(ada, "/folder /no/such/folder").await;
    assert!(reply.contains("/no/such/folder"), "{reply}");
    s.chief.command(ada, "/folder none").await;

    // A channel is opened between two agents and closed again.
    let eve = s.add_agent(ada, "eve", "write").await;
    s.chief
        .command(ada, "/channel agent#4 agent#3 they share a file")
        .await;
    s.chief
        .until("the opened channel", |c| {
            c.hamlet("The Docs").unwrap().channels.iter().any(|r| {
                r.kind == ChannelKind::Opened
                    && [r.a, r.b] == [bob, eve]
            })
        })
        .await;
    s.chief
        .command(ada, "/unchannel agent#4 agent#3 done")
        .await;
    s.chief
        .until("the channel gone", |c| {
            c.hamlet("The Docs")
                .unwrap()
                .channels
                .iter()
                .all(|r| r.kind != ChannelKind::Opened)
        })
        .await;
    // The lead's office passes to another agent.
    s.chief.command(ada, "/lead agent#3 handing over").await;
    s.chief
        .until("bob lead", |c| {
            c.hamlet("The Docs").unwrap().lead == bob
        })
        .await;
    assert!(s.chief.agent("agent#3").unwrap().lead);
    assert!(!s.chief.agent("agent#2").unwrap().lead);
    assert_eq!(s.chief.agent("agent#2").unwrap().parent, Some(bob));

    // Removing a hamlet ends its agents; restoring brings them back.
    s.chief
        .command(hand, &format!("/remove-hamlet {docs} finished"))
        .await;
    s.chief
        .until("hamlet removed", |c| {
            c.hamlet("The Docs").is_none()
                && c.realm().closed.iter().any(|h| h.id == docs)
        })
        .await;
    assert_eq!(
        s.chief.agent("agent#3").unwrap().left.unwrap().how,
        LeftHow::Hamlet
    );
    s.chief
        .command(hand, &format!("/restore-hamlet {docs}"))
        .await;
    s.chief
        .until("hamlet back", |c| {
            c.hamlet("The Docs").is_some()
                && c.agent("agent#3")
                    .is_some_and(|a| a.left.is_none())
        })
        .await;
    s.chief
        .command(hand, &format!("/remove-hamlet {docs} again"))
        .await;
    s.chief
        .command(hand, &format!("/erase-hamlet {docs}"))
        .await;
    s.chief
        .until("hamlet erased", |c| {
            c.realm().closed.iter().all(|h| h.id != docs)
        })
        .await;
}
