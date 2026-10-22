//! The realm: founding hamlets, by command and by vote, roads and
//! moves between them, and closing, restoring and erasing them.

use hamlet_acceptance::*;
use hamlet_protocol::client::*;
use serde_json::json;

fn society() -> Script {
    Script::new().agent(
        "home:0",
        [
            call(
                "create_agent",
                json!({"name": "scout", "purpose": "Scout", "reason": "Explore"}),
            ),
            call(
                "create_agent",
                json!({"name": "smith", "purpose": "Smith", "reason": "Build", "roads": ["scout"]}),
            ),
            call(
                "create_agent",
                json!({"name": "poet", "purpose": "Poet", "reason": "Write"}),
            ),
            words("The society is ready"),
        ],
    )
}

async fn build(s: &mut Session) {
    s.chief.say(mayor(), "Build our society").await;
    s.chief
        .until("the society", |c| c.agent("home", "poet").is_some())
        .await;
    s.chief.idle("home", "founder").await;
}

fn roads(realm: &State) -> Vec<String> {
    realm
        .hamlets
        .iter()
        .flat_map(|h| h.roads.iter())
        .map(|r| {
            let mut ends = [
                format!("{}#{}", r.a.hamlet, r.a.number),
                format!("{}#{}", r.b.hamlet, r.b.number),
            ];
            ends.sort();
            ends.join("--")
        })
        .collect()
}

fn ids(realm: &State) -> Vec<String> {
    realm.hamlets.iter().map(|h| h.id.clone()).collect()
}

fn yes() -> Step {
    call(
        "cast_ballot",
        json!({"choice": "yes", "reason": "A new direction", "evidence": ""}),
    )
}

#[tokio::test]
async fn founds_a_hamlet_by_vote_has_the_asker_pay_and_overturns_it()
{
    let script = society()
        .agent(
            "home:1",
            [
                call(
                    "call_vote",
                    json!({
                        "kind": "found_hamlet",
                        "hamlet": "mill",
                        "mayor": "miller",
                        "purpose": "Grind flour",
                        "reason": "A new trade"
                    }),
                ),
                words("A new hamlet"),
            ],
        )
        .errands([words("Let them decide"), yes(), yes(), yes()])
        .agent(
            "home:0",
            [
                call(
                    "ask_agent",
                    json!({"name": "mayor@mill", "message": "How much flour?"}),
                ),
                words("The mill is working"),
            ],
        )
        .agent("mill:0", [words("Three sacks")])
        .price(1.0);
    let mut s = Session::start(script).await;
    build(&mut s).await;
    s.chief.say(to("home", "scout"), "Propose a mill").await;
    let founded = s.join().await;
    assert!(ids(&founded.realm()).contains(&"mill".into()));
    let miller = founded.agent("mill", "miller").unwrap();
    assert!(miller.mayor);
    assert!(s.home.file("houses/mill/0/NOTES.md").is_file());
    let spent = |c: &Client, id: &str| {
        let realm = c.realm();
        realm
            .hamlets
            .iter()
            .find(|h| h.id == id)
            .unwrap()
            .budget
            .spent
            .micros
    };
    let before = spent(&founded, "home");
    let mayor_chat = chat(mayor());
    let own_before = s.chief.open(&mayor_chat).await.usage.micros;
    s.chief.say(mayor(), "Ask the mill").await;
    let own =
        s.chief.open(&mayor_chat).await.usage.micros - own_before;
    let peer = s
        .chief
        .open(&ChatRef {
            from: Some(AgentRef {
                hamlet: "home".into(),
                number: 0,
            }),
            kind: Some(ChatRefKind::Questions),
            ..chat(to("mill", "mayor"))
        })
        .await;
    assert_eq!(peer.items.last().unwrap().text, "Three sacks");
    assert!(peer.usage.micros > 0, "the answer must have a charge");
    s.chief
        .until("the asked conversation in the realm", |c| {
            c.agent("mill", "miller")
                .is_some_and(|a| !a.conversations.is_empty())
        })
        .await;
    // The asked side lists the conversation.
    let asked = s.chief.agent("mill", "miller").unwrap();
    let questions = asked
        .conversations
        .into_iter()
        .map(|c| (c.kind, c.with.hamlet, c.with.number, c.count))
        .collect::<Vec<_>>();
    assert_eq!(
        questions,
        [(ConversationViewKind::Questions, "home".to_string(), 0, 1)]
    );
    // The hamlet that asks pays for the answer.
    s.chief
        .until("the home's spending includes both sides", |c| {
            spent(c, "home")
                == before
                    + i64::try_from(own + peer.usage.micros).unwrap()
        })
        .await;
    assert_eq!(spent(&s.join().await, "mill"), 0);
    let reply =
        s.chief.say(mayor(), "/overturn 1 Stay together").await;
    assert_eq!(reply.kind, ReplyKind::Notice);
    assert!(reply.text.contains("removed and archived"), "{reply:?}");
    let closed = s.join().await.realm();
    assert_eq!(
        closed
            .closed
            .iter()
            .map(|h| h.id.as_str())
            .collect::<Vec<_>>(),
        ["mill"]
    );
    assert!(!ids(&closed).contains(&"mill".into()));
}

#[tokio::test]
async fn agrees_a_road_and_a_move_between_hamlets() {
    let script = society()
        .agent(
            "mill:0",
            [
                call(
                    "create_agent",
                    json!({"name": "mason", "purpose": "Build the mill", "reason": "Stonework"}),
                ),
                words("Mason ready"),
                words("Considering the road"),
                call(
                    "agree_road",
                    json!({"a": "mason", "b": "scout", "reason": "Trade"}),
                ),
                words("Road agreed"),
                words("Considering the move"),
                call(
                    "agree_move",
                    json!({"agent": "scout", "to": "mill", "keep_roads": ["smith"], "reason": "Welcome"}),
                ),
                words("Move agreed"),
            ],
        )
        .agent("home:1", [words("My old chief conversation")])
        .agent(
            "home:0",
            [
                call(
                    "agree_road",
                    json!({"a": "scout", "b": "mason", "reason": "Trade"}),
                ),
                words("Road proposed"),
                call(
                    "agree_move",
                    json!({"agent": "scout", "to": "mill", "keep_roads": ["smith"], "reason": "Learn"}),
                ),
                words("Move proposed"),
            ],
        );
    let mut s = Session::start(script).await;
    build(&mut s).await;
    s.chief.say(mayor(), "/found mill miller Grind flour").await;
    let mill = to("mill", "mayor");
    s.chief.say(mill.clone(), "Make a mason").await;
    let scout = to("home", "scout");
    s.chief.say(scout.clone(), "Remember this").await;
    let number = s.chief.open(&chat(scout)).await.number;
    std::fs::write(
        s.home.file("houses/home/1/NOTES.md"),
        "# Notes\nIron in the hill",
    )
    .unwrap();
    s.chief.say(mayor(), "Propose the road").await;
    s.chief.say(mill.clone(), "Agree the road").await;
    s.chief
        .until("the road between hamlets", |c| {
            roads(&c.realm()).contains(&"home#1--mill#1".into())
        })
        .await;
    let prompt = s.chief.prompt(to("home", "scout")).await;
    assert!(
        prompt.contains(
            "Your roads to agents of other hamlets lead to: mason. \
             Whoever asks pays for the answer."
        ),
        "{prompt}"
    );
    s.chief.say(mayor(), "Propose the move").await;
    s.chief.say(mill, "Agree the move").await;
    s.chief
        .until("scout in the mill", |c| {
            c.agent("mill", "scout").is_some_and(|a| a.left.is_none())
        })
        .await;
    let moved = chat(to("mill", "scout"));
    let page = s.chief.open(&moved).await;
    assert!(
        page.items
            .iter()
            .any(|i| i.text == "My old chief conversation"),
        "{page:?}"
    );
    assert_eq!(page.number, number);
    assert_eq!(
        std::fs::read_to_string(
            s.home.file("houses/mill/2/NOTES.md")
        )
        .unwrap(),
        "# Notes\nIron in the hill"
    );
    assert!(!s.home.file("houses/home/1").exists());
    let realm = s.join().await.realm();
    let scout = realm
        .agents
        .iter()
        .find(|a| a.name == "scout" && a.left.is_none())
        .unwrap();
    assert_eq!(
        scout.ref_,
        AgentRef {
            hamlet: "mill".into(),
            number: 2
        }
    );
    let roads = roads(&realm);
    assert!(roads.contains(&"home#2--mill#2".into()), "{roads:?}");
    assert!(!roads.contains(&"home#1--mill#1".into()), "{roads:?}");
}

#[tokio::test]
async fn gathers_removal_asks_and_lets_only_the_chief_close_restore_and_erase()
 {
    let mut script = Script::new();
    for place in ["dock", "farm", "fort"] {
        script = script.agent(
            &format!("{place}:0"),
            [
                call(
                    "ask_removal",
                    json!({"hamlet": "mill", "reason": "Idle"}),
                ),
                words("Asked"),
            ],
        );
    }
    let mut s = Session::start(script).await;
    for (id, mayor_name) in [
        ("mill", "miller"),
        ("dock", "docker"),
        ("farm", "farmer"),
        ("fort", "keeper"),
    ] {
        let reply = s
            .chief
            .say(mayor(), &format!("/found {id} {mayor_name} Work"))
            .await;
        assert_eq!(reply.kind, ReplyKind::Notice, "{reply:?}");
    }
    for place in ["dock", "farm", "fort"] {
        s.chief.say(to(place, "mayor"), "Ask for removal").await;
    }
    assert!(ids(&s.join().await.realm()).contains(&"mill".into()));
    let mill_chat = chat(to("mill", "mayor"));
    let page = s.chief.open(&mill_chat).await;
    let notices = page
        .items
        .iter()
        .filter(|i| i.kind == ItemKind::Notice)
        .map(|i| i.text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        notices,
        [
            "The hamlet `mill` was founded under the mayor \
             `miller`. /hamlet mill talks to it.",
            "The mayor of `dock` asks for the hamlet `mill` to be \
             removed. Not enough mayors have asked yet.",
            "The mayor of `farm` asks for the hamlet `mill` to be \
             removed. Not enough mayors have asked yet.",
            "The mayor of `fort` asks for the hamlet `mill` to be \
             removed. Enough have asked: /remove-hamlet mill \
             confirms it.",
        ]
    );
    let reply =
        s.chief.say(mayor(), "/remove-hamlet mill Finished").await;
    assert_eq!(reply.kind, ReplyKind::Notice);
    let closed = s.join().await.realm().closed;
    assert_eq!(
        closed.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(),
        ["mill"]
    );
    assert_eq!(
        (closed[0].reason.as_str(), closed[0].by.as_str()),
        ("Finished", "chief")
    );
    let reply = s.chief.say(mayor(), "/restore-hamlet mill").await;
    assert_eq!(reply.kind, ReplyKind::Notice);
    assert!(s.join().await.realm().closed.is_empty());
    s.chief
        .say(mayor(), "/remove-hamlet mill Finished again")
        .await;
    let reply = s.chief.say(mayor(), "/erase-hamlet mill").await;
    assert_eq!(reply.kind, ReplyKind::Notice);
    assert!(!s.home.file("houses/mill").exists());
    let reply =
        s.chief.say(mayor(), "/found mill another Work").await;
    assert_eq!(reply.kind, ReplyKind::Error);
    assert!(reply.text.contains("used before"), "{reply:?}");
}

#[tokio::test]
async fn founds_names_and_places_hamlets_by_command() {
    let mut s = Session::start(
        Script::new().agent("mill:0", [words("Fresh flour.")]),
    )
    .await;
    let reply = s.chief.say(mayor(), "/found mill miller").await;
    assert!(reply.text.contains("purpose"), "{reply:?}");
    for name in ["mayor", "founder"] {
        let reply = s
            .chief
            .say(mayor(), &format!("/found mill {name} Work"))
            .await;
        assert!(reply.text.contains("reserved"), "{reply:?}");
    }
    let reply = s
        .chief
        .say(mayor(), "/found mill miller Grinds flour.")
        .await;
    assert_eq!(
        (reply.kind, reply.text.as_str()),
        (
            ReplyKind::Notice,
            "The hamlet `mill` is founded under the mayor `miller`, \
             and it is yours. /hamlet mill talks to it."
        )
    );
    assert!(s.home.file("houses/mill/0/NOTES.md").is_file());
    let reply = s.chief.say(to("mill", "mayor"), "Hello").await;
    assert_eq!(reply.kind, ReplyKind::Notice);
    let prompt = s.chief.prompt(mayor()).await;
    assert!(prompt.contains("mill: mayor `miller`"), "{prompt}");
    let reply =
        s.chief.say(mayor(), "/found dock miller Ships").await;
    assert!(reply.text.contains("already exists"), "{reply:?}");
    let reply = s.chief.say(mayor(), "/hamlets").await;
    assert!(reply.text.contains("mill"), "{reply:?}");
    let reply = s
        .chief
        .say(
            mayor(),
            &format!("/name-hamlet mill {}", "x".repeat(33)),
        )
        .await;
    assert!(reply.text.contains("32 letters"), "{reply:?}");
    let reply =
        s.chief.say(mayor(), "/name-hamlet mill The   Mill").await;
    assert_eq!(reply.kind, ReplyKind::Notice);
    let reply =
        s.chief.say(mayor(), "/move-hamlet mill -12.5 40").await;
    assert_eq!(reply.kind, ReplyKind::Notice);
    s.chief
        .until("the mill named and placed", |c| {
            c.realm().hamlets.iter().any(|h| {
                h.id == "mill"
                    && h.name == "The Mill"
                    && h.at == Some(TownAt { x: -12.5, z: 40.0 })
            })
        })
        .await;
    let reply =
        s.chief.say(mayor(), "/move-hamlet mill Infinity 0").await;
    assert_eq!(reply.kind, ReplyKind::Error);
    let reply = s.chief.say(mayor(), "/remove-hamlet home").await;
    assert!(reply.text.contains("cannot be removed"), "{reply:?}");
    let reply = s.chief.say(mayor(), "/erase-hamlet mill").await;
    assert_eq!(reply.kind, ReplyKind::Error);
    s.chief.limit("home", "hamlets", "most", json!(2)).await;
    let reply =
        s.chief.say(mayor(), "/found dock docker Ships").await;
    assert!(reply.text.contains("no more hamlets"), "{reply:?}");
}

#[tokio::test]
async fn edits_and_reads_another_hamlets_papers_until_it_is_removed()
{
    let mut s = Session::start(
        Script::new().agent("mill:0", [words("Dry flour.")]),
    )
    .await;
    s.chief.say(mayor(), "/found mill miller Grind flour").await;
    let miller = to("mill", "mayor");
    let reply = s
        .chief
        .request(|id| {
            ClientMessage::Edit(Edit::Character {
                id,
                agent: miller.clone(),
                text: "Be curious.".into(),
            })
        })
        .await;
    assert_eq!(
        (reply.kind, reply.text.as_str()),
        (ReplyKind::Notice, "Saved.")
    );
    let reply =
        s.chief.limit("mill", "budget", "cost", json!(8)).await;
    assert_eq!(
        (reply.kind, reply.text.as_str()),
        (ReplyKind::Notice, "Saved.")
    );
    s.chief
        .until("the mill's own budget", |c| {
            let realm = c.realm();
            let cost = |id: &str| {
                realm
                    .hamlets
                    .iter()
                    .find(|h| h.id == id)
                    .and_then(|h| h.limits.budget.cost)
            };
            cost("mill") == Some(8.0) && cost("home") == Some(5.0)
        })
        .await;
    let reply = s
        .chief
        .request(|id| {
            ClientMessage::Edit(Edit::Notes {
                id,
                agent: to("mill", "mayor"),
                text: "# Notes\nKeep flour dry.".into(),
            })
        })
        .await;
    assert_eq!(
        (reply.kind, reply.text.as_str()),
        (ReplyKind::Notice, "Saved.")
    );
    let papers = s.chief.papers(to("mill", "mayor")).await;
    assert_eq!(
        (papers.character.as_str(), papers.notes.as_str()),
        ("Be curious.", "# Notes\nKeep flour dry."),
        "{}",
        s.server.log()
    );
    s.chief.say(to("mill", "mayor"), "What matters?").await;
    let history = s
        .chief
        .history(to("mill", "mayor"))
        .await
        .into_iter()
        .map(|p| (p.number, p.current, p.title, p.messages))
        .collect::<Vec<_>>();
    assert_eq!(history, [(1, true, "What matters?".to_string(), 2)]);
    assert!(
        s.chief
            .prompt(to("mill", "mayor"))
            .await
            .contains("Be curious.")
    );
    s.chief.say(mayor(), "/remove-hamlet mill Done").await;
    let reply =
        s.chief.limit("mill", "budget", "cost", json!(9)).await;
    assert_eq!(reply.kind, ReplyKind::Error);
    assert!(reply.text.contains("was removed"), "{reply:?}");
    let reply = s
        .chief
        .request(|id| {
            ClientMessage::Edit(Edit::Notes {
                id,
                agent: to("mill", "mayor"),
                text: "Changed".into(),
            })
        })
        .await;
    assert_eq!(reply.kind, ReplyKind::Error);
    assert!(reply.text.contains("was removed"), "{reply:?}");
    let read = s
        .chief
        .read(|id| Read::Prompt {
            agent: to("mill", "mayor"),
            id,
        })
        .await;
    assert!(format!("{read:?}").contains("was removed"), "{read:?}");
}

#[tokio::test]
async fn sends_a_whole_state_when_a_hamlet_closes_or_goes() {
    let mut s = Session::start(Script::new()).await;
    s.chief.say(mayor(), "/found mill miller Grind flour").await;
    s.chief
        .until("the mill founded", |c| {
            ids(&c.realm()).contains(&"mill".into())
        })
        .await;
    s.chief.say(mayor(), "/remove-hamlet mill Finished").await;
    s.chief
        .until("a whole state with the mill closed", |c| {
            c.messages().iter().any(|m| match m {
                ServerMessage::State(state) => {
                    ids(state) == ["home"]
                        && state.closed.iter().any(|h| h.id == "mill")
                }
                _ => false,
            })
        })
        .await;
    let ServerMessage::State(state) = s
        .chief
        .messages()
        .iter()
        .rfind(|m| matches!(m, ServerMessage::State(_)))
        .unwrap()
    else {
        unreachable!()
    };
    assert_eq!(ids(state), ["home"]);
    assert_eq!(
        state
            .closed
            .iter()
            .map(|h| h.id.as_str())
            .collect::<Vec<_>>(),
        ["mill"]
    );
    s.chief.say(mayor(), "/erase-hamlet mill").await;
    s.chief
        .until("the closed list empty", |c| {
            c.realm().closed.is_empty()
        })
        .await;
}

#[tokio::test]
async fn stops_another_hamlets_agent_by_name_and_office() {
    let held = || {
        call(
            "bash",
            json!({"command": "touch never", "network": true}),
        )
    };
    let mut s = Session::start(Script::new().agent("mill:0", [
        call("create_agent", json!({"name": "scout", "purpose": "Explore", "reason": "Needed"})),
        words("Ready."), held(),
    ]).agent("mill:1", [held(), held()])).await;
    s.chief.say(mayor(), "/found mill miller Grind flour").await;
    s.chief.say(to("mill", "mayor"), "Hire scout.").await;
    for (address, target) in [
        ("scout", "scout"),
        ("scout@mill", "scout"),
        ("mayor@mill", "mayor"),
    ] {
        s.chief.tell(to("mill", target), "Work.").await;
        let wait = s
            .chief
            .wait_for("the mill's approval", |w| {
                matches!(w.detail, WaitDetail::Approval { .. })
            })
            .await;
        let reply =
            s.chief.say(mayor(), &format!("/stop {address}")).await;
        assert_eq!(reply.text, format!("Stopping {address}."));
        s.chief
            .until("the wait lapsed", |c| c.waits().is_empty())
            .await;
        assert!(s.chief.changes().contains(&Update::WaitClosed {
            id: wait.id.clone(),
            how: "stopped".into()
        }));
        assert!(
            !s.home
                .file(&format!(
                    "houses/mill/{}/never",
                    if target == "mayor" { 0 } else { 1 }
                ))
                .exists()
        );
    }
}

#[tokio::test]
async fn agreements_survive_office_changes_but_former_mayors_cannot_add_them()
 {
    let road = |a: &str| {
        call(
            "agree_road",
            json!({"a":a,"b":"mason","reason":"Trade"}),
        )
    };
    let script = society().agent("home:0", [road("scout"), words("Proposed"), road("smith"), words("Former mayor tried")])
        .agent("mill:0", [
            call("create_agent", json!({"name":"mason","purpose":"Build","reason":"Work"})), words("Ready"),
            words("Consider later"), road("scout"), words("Agreed existing assent"),
            road("smith"), words("Only mill assented"),
        ]).agent("home:3", [words("Consider later")]);
    let mut s = Session::start(script).await;
    build(&mut s).await;
    s.chief.say(mayor(), "/found mill miller Work").await;
    s.chief.say(to("mill", "mayor"), "Create mason").await;
    s.chief.say(mayor(), "Propose scout's road").await;
    assert!(
        !roads(&s.join().await.realm())
            .contains(&"home#1--mill#1".into())
    );
    s.chief.say(mayor(), "/mayor poet New office").await;
    s.chief
        .say(to("mill", "mayor"), "Accept existing proposal")
        .await;
    assert!(
        roads(&s.join().await.realm())
            .contains(&"home#1--mill#1".into())
    );
    s.chief
        .say(to("home", "founder"), "Try proposing smith's road")
        .await;
    s.chief.say(to("mill", "mayor"), "Agree smith's road").await;
    assert!(
        !roads(&s.join().await.realm())
            .contains(&"home#2--mill#1".into())
    );
}

#[tokio::test]
async fn open_votes_prevent_recording_assent_in_either_hamlet() {
    let moving = || {
        call(
            "agree_move",
            json!({"agent":"scout","to":"mill","reason":"Work"}),
        )
    };
    let script = society()
        .errands([words("Held hearing").held_until("never")])
        .agent("home:0", [
            call("agree_road", json!({"a":"scout","b":"mason","reason":"Trade"})), words("Road attempted"),
            moving(), words("Move proposed"),
        ])
        .agent("mill:0", [
            call("create_agent", json!({"name":"mason","purpose":"Build","reason":"Work"})), words("Ready"),
            moving(), words("Move attempted"),
            words("Consider later"), moving(), words("Move agreed"),
        ]);
    let mut s = Session::start(script).await;
    build(&mut s).await;
    s.chief.say(mayor(), "/found mill miller Work").await;
    s.chief.say(to("mill", "mayor"), "Create mason").await;
    s.chief.say(mayor(), "/vote poet Review office").await;
    s.chief
        .until("open vote", |c| {
            c.realm().hamlets[0].votes.iter().any(|v| v.open)
        })
        .await;
    s.chief.say(mayor(), "Try road during own vote").await;
    assert!(
        s.join()
            .await
            .agent("mill", "miller")
            .unwrap()
            .conversations
            .is_empty(),
        "A held road agreement must not ask the other mayor"
    );
    s.chief
        .say(to("mill", "mayor"), "Try move while other hamlet votes")
        .await;
    assert!(
        s.join()
            .await
            .agent("home", "founder")
            .unwrap()
            .conversations
            .is_empty(),
        "A held move agreement must not ask the other mayor"
    );
    assert!(
        !roads(&s.join().await.realm())
            .contains(&"home#1--mill#1".into())
    );
    s.chief.say(mayor(), "/overturn 1 Continue").await;
    s.chief.say(mayor(), "Propose move after vote").await;
    assert!(
        s.join().await.agent("mill", "scout").is_none(),
        "A refused earlier agreement must not count"
    );
    s.chief.say(to("mill", "mayor"), "Now agree to move").await;
    s.chief
        .until("scout moved", |c| {
            c.agent("mill", "scout").is_some_and(|a| a.left.is_none())
        })
        .await;
}
