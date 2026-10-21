//! Agents together: creating them, roads, letters, questions, leaving
//! and coming back, merging, votes, objections and the library.

use hamlet_acceptance::*;
use hamlet_protocol::*;
use serde_json::json;

/// The mayor's first turn: it creates scout (1), smith (2, with a
/// road to scout) and poet (3).
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
    let reply = s.chief.say(mayor(), "Build our society").await;
    assert_eq!(reply.kind, ReplyKind::Notice);
    s.chief
        .until("the society", |c| {
            let names = c
                .realm()
                .agents
                .into_iter()
                .map(|a| (a.ref_.number, a.name))
                .collect::<Vec<_>>();
            names
                == [
                    (0, "founder".to_string()),
                    (1, "scout".to_string()),
                    (2, "smith".to_string()),
                    (3, "poet".to_string()),
                ]
        })
        .await;
    s.chief.idle("home", "founder").await;
}

/// Undirected roads as `hamlet#number--hamlet#number`.
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

fn agent(hamlet: &str, number: u64) -> AgentRef {
    AgentRef {
        hamlet: hamlet.into(),
        number,
    }
}

fn peer(with: &str, from: u64, kind: ChatRefKind) -> ChatRef {
    ChatRef {
        with: to("home", with),
        from: Some(agent("home", from)),
        kind: Some(kind),
        past: None,
    }
}

/// What each item shows: its author, or the failure of a tool, then
/// its text.
fn shown(items: &[Item]) -> String {
    items
        .iter()
        .map(|i| {
            i.tool
                .as_ref()
                .and_then(|t| t.failure.clone())
                .unwrap_or_else(|| i.text.clone())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn carries_a_letter_and_its_reply_then_asks_a_question() {
    let script = society()
        .agent(
            "home:0",
            [
                call("tell_agent", json!({"name": "smith", "message": "Make a hinge"})),
                words("Sent"),
                words("The letter returned"),
                call("ask_agent", json!({"name": "scout", "message": "Where is the iron?"})),
                words("Iron found"),
            ],
        )
        .agent("home:2", [words("Hinge ready")])
        .agent("home:1", [words("In the northern hill")]);
    let mut s = Session::start(script).await;
    build(&mut s).await;
    let mayor_chat = chat(mayor());
    s.chief.open(&mayor_chat).await;
    s.chief.say(mayor(), "Tell smith").await;
    s.chief.shows(&mayor_chat, "The letter returned").await;
    let reply = s
        .chief
        .items(&mayor_chat)
        .into_iter()
        .find(|i| i.text.starts_with("Reply from"))
        .unwrap();
    assert_eq!(reply.author.as_deref(), Some("smith@home"));
    assert_eq!(reply.kind, ItemKind::User);
    assert!(reply.text.contains("Hinge ready"), "{reply:?}");
    let letters =
        s.chief.open(&peer("smith", 0, ChatRefKind::Letters)).await;
    assert_eq!(
        letters
            .items
            .iter()
            .map(|i| (i.author.as_deref(), i.text.as_str()))
            .collect::<Vec<_>>(),
        [
            (
                Some("founder@home"),
                "Letter from `mayor`:\nMake a hinge"
            ),
            (None, "Hinge ready")
        ]
    );
    s.chief.say(mayor(), "Ask scout").await;
    s.chief.shows(&mayor_chat, "Iron found").await;
    let late = s.join().await;
    let scout = late.agent("home", "scout").unwrap();
    assert_eq!(
        scout
            .conversations
            .into_iter()
            .map(|c| (c.with, c.kind, c.count))
            .collect::<Vec<_>>(),
        [(agent("home", 0), ConversationViewKind::Questions, 1)]
    );
    let realm = late.realm();
    assert!(roads(&realm).contains(&"home#1--home#2".into()));
    let ends = [agent("home", 0), agent("home", 2)];
    let to_smith = realm.hamlets[0]
        .roads
        .iter()
        .find(|r| ends.contains(&r.a) && ends.contains(&r.b))
        .unwrap();
    assert_eq!(to_smith.messages, 2);
    let questions = s
        .chief
        .open(&peer("scout", 0, ChatRefKind::Questions))
        .await;
    assert_eq!(
        questions
            .items
            .iter()
            .map(|i| (i.author.as_deref(), i.text.as_str()))
            .collect::<Vec<_>>(),
        [
            (
                Some("founder@home"),
                "Question from `mayor` (waiting for your \
                 answer):\nWhere is the iron?"
            ),
            (None, "In the northern hill")
        ]
    );
    let questions_chat = peer("scout", 0, ChatRefKind::Questions);
    let item = questions.items[0].id.clone();
    let reply = s
        .chief
        .request(|id| {
            ClientMessage::Rewind(Rewind {
                id,
                chat: questions_chat,
                item,
            })
        })
        .await;
    assert_eq!(
        (reply.kind, reply.text.as_str()),
        (
            ReplyKind::Error,
            "Conversations between agents are read-only."
        )
    );
}

#[tokio::test]
async fn asks_the_mayor_for_a_road_and_the_mayor_builds_it() {
    let script = society()
        .agent(
            "home:1",
            [
                call("request_road", json!({"to": "poet", "reason": "Map legends"})),
                words("Road requested"),
            ],
        )
        .agent(
            "home:0",
            [
                call(
                    "build_road",
                    json!({"a": "scout", "b": "poet", "reason": "Map legends"}),
                ),
                words("Road laid"),
            ],
        );
    let mut s = Session::start(script).await;
    build(&mut s).await;
    s.chief.say(to("home", "scout"), "Request a road").await;
    s.chief
        .until("the road from scout to poet", |c| {
            roads(&c.realm()).contains(&"home#1--home#3".into())
        })
        .await;
}

#[tokio::test]
async fn lays_and_takes_up_roads_by_command() {
    let mut s = Session::start(society()).await;
    build(&mut s).await;
    let reply = s.chief.say(mayor(), "/road scout smith").await;
    assert!(reply.text.contains("already have a road"), "{reply:?}");
    let reply =
        s.chief.say(mayor(), "/road nobody nobody Needed").await;
    assert_eq!(
        (reply.kind, reply.text.as_str()),
        (ReplyKind::Error, "no agent named `nobody` in this hamlet")
    );
    let reply = s.chief.say(mayor(), "/unroad mayor scout").await;
    assert!(reply.text.contains("stays for as long"), "{reply:?}");
    let spent = s.join().await.realm().hamlets[0].budget.spent.micros;
    let reply = s.chief.say(mayor(), "/road scout poet share").await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{reply:?}");
    s.chief
        .until("the new road", |c| {
            roads(&c.realm()).contains(&"home#1--home#3".into())
        })
        .await;
    let reply = s
        .chief
        .say(mayor(), "/unroad scout smith no longer needed")
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{reply:?}");
    s.chief
        .until("the road gone", |c| {
            !roads(&c.realm()).contains(&"home#1--home#2".into())
        })
        .await;
    // A road costs a cent when laid, and is not given back when taken
    // up.
    s.chief
        .until("the road's charge", |c| {
            c.realm().hamlets[0].budget.spent.micros == spent + 10_000
        })
        .await;
    let reply = s.chief.say(mayor(), "/road scout poet again").await;
    assert!(reply.text.contains("already have a road"), "{reply:?}");
    assert_eq!(
        s.join().await.realm().hamlets[0].budget.spent.micros,
        spent + 10_000
    );
}

#[tokio::test]
async fn protects_an_instructed_agent_then_removes_and_undoes() {
    let script = society()
        .agent("home:1", [words("Chief task answered").held_until("release")])
        .agent(
            "home:0",
            [
                call("remove_agent", json!({"name": "scout", "reason": "Finished"})),
                words("Removal checked"),
                call("remove_agent", json!({"name": "scout", "reason": "Finished"})),
                call("tell_agent", json!({"name": "scout", "message": "Still there?"})),
                words("Archived"),
            ],
        );
    let mut s = Session::start(script).await;
    build(&mut s).await;
    let scout_chat = chat(to("home", "scout"));
    s.chief.open(&scout_chat).await;
    let working =
        s.chief.tell(to("home", "scout"), "Keep working").await;
    s.chief
        .until("scout to work", |c| c.working(&scout_chat))
        .await;
    let mayor_chat = chat(mayor());
    s.chief.open(&mayor_chat).await;
    s.chief.say(mayor(), "Remove scout").await;
    let failure = s
        .chief
        .items(&mayor_chat)
        .into_iter()
        .find_map(|i| i.tool.and_then(|t| t.failure))
        .unwrap();
    assert!(
        failure.starts_with(
            "`scout` has a task from the chief chief (\"Keep \
             working\") that is not finished."
        ),
        "{failure}"
    );
    std::fs::write(s.home.side().join("release"), "").unwrap();
    s.chief.reply(&working).await;
    s.chief.say(mayor(), "Remove now").await;
    assert!(
        shown(&s.chief.items(&mayor_chat)).contains("removed"),
        "{}",
        shown(&s.chief.items(&mayor_chat))
    );
    s.chief
        .until("scout removed", |c| {
            c.agent("home", "scout").is_some_and(|a| a.left.is_some())
        })
        .await;
    let left = s.chief.agent("home", "scout").unwrap().left.unwrap();
    assert_eq!(
        (&left.how, left.by.as_str(), left.reason.as_str()),
        (&LeftHow::Removed, "the mayor", "Finished")
    );
    assert!(
        !roads(&s.chief.realm()).contains(&"home#1--home#2".into())
    );
    let reply = s.chief.say(mayor(), "/undo").await;
    assert_eq!(reply.kind, ReplyKind::Notice);
    assert!(reply.text.contains("Restored `scout`"), "{reply:?}");
    let late = s.join().await;
    assert!(late.agent("home", "scout").unwrap().left.is_none());
    assert!(roads(&late.realm()).contains(&"home#1--home#2".into()));
}

#[tokio::test]
async fn removes_restores_and_erases_an_agent_by_command() {
    let mut s = Session::start(society()).await;
    build(&mut s).await;
    let reply = s.chief.say(mayor(), "/remove mayor x").await;
    assert!(
        reply.text.contains(
            "the mayor cannot be removed or merged; replacing a \
             mayor takes a chief or a vote"
        ),
        "{reply:?}"
    );
    let reply = s.chief.say(mayor(), "/erase scout").await;
    assert_eq!(
        reply.text,
        "nothing is archived under `scout`. Only an agent that was \
         removed or merged can be deleted for good; /remove it first"
    );
    std::fs::write(s.home.file("houses/home/1/found.txt"), "Dune")
        .unwrap();
    let reply =
        s.chief.say(mayor(), "/remove scout Work ended").await;
    assert_eq!(reply.kind, ReplyKind::Notice);
    assert!(
        reply.text.starts_with(
            "Removed `scout`. Its house is archived as scout-"
        ) && reply.text.ends_with(
            " and its roads are gone. A chief can restore it."
        ),
        "{reply:?}"
    );
    let farewell = "`scout` was removed from the hamlet by the \
                    chief chief (\"Work ended\"). Its house is \
                    archived and its roads went with it; only a \
                    chief can restore it.";
    s.chief
        .until("scout removed", |c| {
            c.agent("home", "scout").is_some_and(|a| a.left.is_some())
        })
        .await;
    let left = s.chief.agent("home", "scout").unwrap().left.unwrap();
    assert_eq!(left.words, farewell);
    let reply = s.chief.say(to("home", "scout"), "Work").await;
    assert_eq!(
        (reply.kind, reply.text.as_str()),
        (ReplyKind::Error, farewell)
    );
    let scout_chat = chat(to("home", "scout"));
    let opened = s
        .chief
        .ask(|id| {
            ClientMessage::Open(Open {
                id,
                chat: scout_chat,
                before: None,
            })
        })
        .await;
    assert!(
        matches!(&opened, ServerMessage::Fault(f) if f.text == farewell),
        "{opened:?}"
    );
    let reply = s.chief.say(mayor(), "/stop scout").await;
    assert_eq!(
        (reply.kind, reply.text.as_str()),
        (ReplyKind::Error, farewell)
    );
    let reply = s.chief.say(mayor(), "/record").await;
    assert!(reply.text.contains("/restore scout-"), "{reply:?}");
    assert!(
        reply
            .text
            .contains("the chief chief removed `scout`: Work ended"),
        "{reply:?}"
    );
    let reply = s.chief.say(mayor(), "/restore scout").await;
    assert!(reply.text.starts_with("Restored `scout`"), "{reply:?}");
    assert_eq!(
        std::fs::read_to_string(
            s.home.file("houses/home/1/found.txt")
        )
        .unwrap(),
        "Dune"
    );
    s.chief.say(mayor(), "/remove scout Work ended").await;
    let reply = s.chief.say(mayor(), "/erase scout").await;
    assert!(
        reply.text.starts_with("Deleted `scout` for good.")
            && reply.text.ends_with("It cannot be restored."),
        "{reply:?}"
    );
    assert!(!s.home.file("houses/home/1").exists());
    let reply = s.chief.say(mayor(), "/restore scout").await;
    assert_eq!(
        reply.text,
        "nothing is archived under `scout`; /record lists what is"
    );
}

#[tokio::test]
async fn merges_an_agent_redirects_its_letters_and_splits_it_back_out()
 {
    let script = society()
        .agent(
            "home:0",
            [
                call(
                    "merge_agents",
                    json!({"keep": "smith", "absorb": "scout", "purpose": "Find and forge", "reason": "Together"}),
                ),
                call("tell_agent", json!({"name": "scout", "message": "Hello"})),
                words("Merged"),
            ],
        )
        .errands([words("# Notes\nFind and forge.")]);
    let mut s = Session::start(script).await;
    build(&mut s).await;
    s.chief.say(mayor(), "Merge the work").await;
    let late = s.join().await;
    let left = late.agent("home", "scout").unwrap().left.unwrap();
    assert_eq!(
        (&left.how, &left.into),
        (&LeftHow::Merged, &Some(agent("home", 2)))
    );
    assert!(left.words.contains("talk to `smith`"), "{left:?}");
    let smith = late.agent("home", "smith").unwrap();
    assert_eq!(smith.purpose, "Find and forge");
    let reply = s.chief.say(mayor(), "/restore scout").await;
    assert_eq!(reply.kind, ReplyKind::Notice);
    assert!(
        reply.text.contains("Split `scout` from `smith`"),
        "{reply:?}"
    );
    let restored = s.join().await;
    assert!(restored.agent("home", "scout").unwrap().left.is_none());
    assert_eq!(
        restored.agent("home", "smith").unwrap().purpose,
        "Smith"
    );
}

#[tokio::test]
async fn merges_houses_by_command_keeping_both_sides() {
    let mut s = Session::start(society()).await;
    build(&mut s).await;
    for (number, notes) in [
        (2, "# Notes\nAlice likes tea.\n"),
        (1, "# Notes\nThe wiki is slow.\n"),
    ] {
        std::fs::write(
            s.home.file(&format!("houses/home/{number}/NOTES.md")),
            notes,
        )
        .unwrap();
    }
    std::fs::write(s.home.file("houses/home/2/list.txt"), "mine")
        .unwrap();
    std::fs::write(s.home.file("houses/home/1/list.txt"), "theirs")
        .unwrap();
    std::fs::write(s.home.file("houses/home/1/found.txt"), "Dune")
        .unwrap();
    let reply = s
        .chief
        .say(mayor(), "/merge scout into smith They overlap.")
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{reply:?}");
    for part in [
        "Merged `scout` into `smith`",
        "joined, not rewritten",
        "Kept apart because they differed:",
        "list.txt",
    ] {
        assert!(reply.text.contains(part), "{part:?} in {reply:?}");
    }
    let house = |file: &str| {
        std::fs::read_to_string(
            s.home.file(&format!("houses/home/2/{file}")),
        )
        .unwrap()
    };
    assert!(
        house("NOTES.md")
            .contains("## From scout\nThe wiki is slow.")
    );
    assert_eq!(
        house("merged/scout/NOTES.before.md"),
        "# Notes\nAlice likes tea.\n"
    );
    assert_eq!(house("list.txt"), "mine");
    assert_eq!(house("merged/scout/list.txt"), "theirs");
    assert_eq!(house("found.txt"), "Dune");
    let brief = s.chief.prompt(to("home", "smith")).await;
    assert!(
        brief.contains(
            "scout: merged into you; its notes, skills and roads \
             are yours"
        ),
        "{brief}"
    );
    let brief = s.chief.prompt(mayor()).await;
    assert!(
        brief.contains("scout: merged into smith; talk to smith"),
        "{brief}"
    );
    let reply = s
        .chief
        .say(mayor(), "/merge founder into founder Needed")
        .await;
    assert_eq!(
        (reply.kind, reply.text.as_str()),
        (ReplyKind::Error, "an agent cannot be merged with itself")
    );
}

#[tokio::test]
async fn holds_decisions_during_a_vote_follows_the_new_office_and_overturns()
 {
    let script = society()
        .errands([
            words("Let the agents decide").held_until("release"),
            call("cast_ballot", json!({"choice": "yes", "reason": "A new direction", "evidence": ""})),
            call("cast_ballot", json!({"choice": "yes", "reason": "A new direction", "evidence": ""})),
            call("cast_ballot", json!({"choice": "yes", "reason": "A new direction", "evidence": ""})),
        ])
        .agent(
            "home:0",
            [
                call("remove_agent", json!({"name": "scout", "reason": "Too soon"})),
                words("Checked"),
            ],
        )
        .agent("home:2", [words("Smith holds the office")]);
    let mut s = Session::start(script).await;
    build(&mut s).await;
    let mut voter = s.join().await;
    let vote =
        voter.tell(mayor(), "/vote smith A new direction").await;
    s.chief
        .until("an open vote", |c| {
            c.realm().hamlets[0].votes.iter().any(|v| v.open)
        })
        .await;
    let mayor_chat = chat(mayor());
    s.chief.open(&mayor_chat).await;
    s.chief.say(mayor(), "Remove scout").await;
    // Nothing else is decided while the vote is open.
    assert!(
        s.join()
            .await
            .agent("home", "scout")
            .unwrap()
            .left
            .is_none()
    );
    let called = s
        .chief
        .items(&mayor_chat)
        .into_iter()
        .find(|i| i.text.starts_with("Vote 1 called"))
        .unwrap();
    assert_eq!(
        (called.kind, called.text.as_str()),
        (
            ItemKind::Notice,
            "Vote 1 called by chief:chief: replace the mayor: \
             `smith` would become mayor, and `founder`, the mayor \
             now, would stay as an ordinary agent. A new direction"
        )
    );
    std::fs::write(s.home.side().join("release"), "").unwrap();
    let reply = voter.reply(&vote).await;
    assert_eq!(reply.kind, ReplyKind::Notice);
    assert!(reply.text.contains("Vote 1 is called"), "{reply:?}");
    s.chief
        .until("smith in office", |c| {
            c.agent("home", "smith").is_some_and(|a| a.mayor)
        })
        .await;
    let realm = s.chief.realm();
    let report = &realm.hamlets[0].votes[0];
    assert!(!report.open);
    assert!(report.report.contains("Passed:"), "{report:?}");
    // The chat with the office now reaches smith.
    s.chief.say(mayor(), "Who is mayor?").await;
    s.chief.shows(&mayor_chat, "Smith holds the office").await;
    let reply = s.chief.say(mayor(), "/votes").await;
    assert!(reply.text.contains("Passed:"), "{reply:?}");
    let reply = s.chief.say(mayor(), "/overturn 1 Back").await;
    assert!(reply.text.contains("founder"), "{reply:?}");
    s.chief
        .until("founder back in office", |c| {
            c.agent("home", "founder").is_some_and(|a| a.mayor)
        })
        .await;
    let reply = s.chief.say(mayor(), "/overturn 1").await;
    assert!(reply.text.contains("already overturned"), "{reply:?}");
}

#[tokio::test]
async fn refuses_votes_that_cannot_be_held() {
    let mut s = Session::start(society()).await;
    let reply = s.chief.say(mayor(), "/vote nobody x").await;
    assert!(reply.text.contains("at least 3 agents"), "{reply:?}");
    build(&mut s).await;
    let reply = s.chief.say(mayor(), "/vote mayor x").await;
    assert!(reply.text.contains("mayor already"), "{reply:?}");
    let reply = s.chief.say(mayor(), "/vote nobody x").await;
    assert!(
        reply.text.contains("candidate must be an agent"),
        "{reply:?}"
    );
}

#[tokio::test]
async fn hands_the_office_over_by_command() {
    let mut s = Session::start(society()).await;
    build(&mut s).await;
    let reply =
        s.chief.say(mayor(), "/mayor smith Steady hands").await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{reply:?}");
    s.chief
        .until("smith in office", |c| {
            c.agent("home", "smith").is_some_and(|a| a.mayor)
                && c.agent("home", "founder")
                    .is_some_and(|a| !a.mayor)
        })
        .await;
    let reply = s.chief.say(mayor(), "/mayor nobody Needed").await;
    assert_eq!(
        (reply.kind, reply.text.as_str()),
        (ReplyKind::Error, "no agent named `nobody`")
    );
    let reply =
        s.chief.say(mayor(), "/mayor founder Back again").await;
    assert!(reply.text.contains("mayor"), "{reply:?}");
}

#[tokio::test]
async fn holds_an_instruction_for_an_objection_then_goes_or_withdraws()
 {
    let script = society().agent(
        "home:1",
        [
            call(
                "object",
                json!({"reason": "This is the only copy"}),
            ),
            call(
                "write",
                json!({"path": "go.txt", "content": "done"}),
            ),
            words("Done"),
            call(
                "object",
                json!({"reason": "This is the only copy"}),
            ),
            call(
                "write",
                json!({"path": "withdraw.txt", "content": "done"}),
            ),
            words("Done"),
        ],
    );
    let mut s = Session::start(script).await;
    build(&mut s).await;
    s.chief
        .limit("home", "asking", "mayorReview", json!("never"))
        .await;
    let scout_chat = chat(to("home", "scout"));
    s.chief.open(&scout_chat).await;
    for choice in [Choice::Go, Choice::Withdraw] {
        let word = if choice == Choice::Go {
            "go"
        } else {
            "withdraw"
        };
        let line = format!("Delete the {word} copy");
        let pending = s.chief.tell(to("home", "scout"), &line).await;
        let wait = s
            .chief
            .wait_for("an objection", |w| {
                matches!(w.detail, WaitDetail::Objection { .. })
            })
            .await;
        let Wait {
            about, answers, by, ..
        } = &wait;
        assert_eq!(
            about,
            &format!(
                "chief told scout: {line}\nscout objects: This is \
                 the only copy"
            )
        );
        assert_eq!(answers, &[Choice::Go, Choice::Withdraw]);
        assert_eq!(by, "scout");
        let reply =
            s.chief.choose(&wait.id, choice, Some("I checked")).await;
        assert_eq!(reply.kind, ReplyKind::Notice);
        s.chief.reply(&pending).await;
        let file = s.home.file(&format!("houses/home/1/{word}.txt"));
        assert_eq!(file.exists(), word == "go", "{word}");
    }
    s.chief
        .shows(
            &scout_chat,
            "The chief withdrew the instruction: I checked",
        )
        .await;
}

#[tokio::test]
async fn shelves_an_offered_skill_for_every_agent_and_unshelves_it() {
    let script = society()
        .agent(
            "home:1",
            [
                call("submit_skill", json!({"skill": "map-iron", "reason": "Useful maps"})),
                words("Shared"),
            ],
        )
        .errands([words("ACCEPT Useful maps")]);
    let mut s = Session::start(script).await;
    build(&mut s).await;
    let skill = s.home.file("houses/home/1/skills/map-iron");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: map-iron\ndescription: Maps iron \
         deposits.\n---\nRead the map.\n",
    )
    .unwrap();
    s.chief.say(to("home", "scout"), "Share the map").await;
    s.chief
        .until("the shelved skill", |c| {
            c.messages().iter().any(|m| {
                matches!(m, ServerMessage::State(s)
                if s.library.iter().any(|k| k.name == "map-iron"))
            })
        })
        .await;
    let shelved = s
        .chief
        .realm()
        .library
        .into_iter()
        .find(|k| k.name == "map-iron")
        .unwrap();
    assert_eq!(
        (shelved.description.as_str(), shelved.by.as_str()),
        ("Maps iron deposits.", "scout@home")
    );
    assert!(!skill.exists());
    assert!(s.home.file("library/map-iron/SKILL.md").is_file());
    assert!(
        s.chief
            .changes()
            .iter()
            .any(|u| matches!(u, Update::Agent { agent }
            if agent.name == "scout" && agent.consulting)),
        "scout consults the librarian"
    );
    let prompt = s.chief.prompt(to("home", "smith")).await;
    assert!(
        prompt.contains("map-iron: Maps iron deposits."),
        "{prompt}"
    );
    let reply =
        s.chief.say(mayor(), "/unshelve map-iron Retire").await;
    assert_eq!(reply.kind, ReplyKind::Notice);
    s.chief
        .until("the skill off the shelf", |c| {
            !c.realm().library.iter().any(|k| k.name == "map-iron")
        })
        .await;
    let reply = s.chief.say(mayor(), "/unshelve absent").await;
    assert_eq!(reply.text, "the library has no skill named `absent`");
    let reply = s.chief.say(mayor(), "/unshelve ../invalid").await;
    assert_eq!(reply.text, "usage: /unshelve <skill>");
}

#[tokio::test]
async fn refuses_agents_the_mayor_may_not_create() {
    let script = Script::new().agent(
        "home:0",
        [
            call("create_agent", json!({"name": "mayor", "purpose": "Looks", "reason": "Needed"})),
            call("create_agent", json!({"name": "scout", "purpose": " ", "reason": "Needed"})),
            call("create_agent", json!({"name": "FelineBiologist", "purpose": "Looks", "reason": "Needed"})),
            call("create_agent", json!({"name": "feline-biologist", "purpose": "Again", "reason": "Needed"})),
            call("set_model", json!({"name": "feline-biologist", "model": "no::such", "reason": "Try"})),
            words("Done"),
        ],
    );
    let mut s = Session::start(script).await;
    let mayor_chat = chat(mayor());
    s.chief.open(&mayor_chat).await;
    s.chief.say(mayor(), "Hire").await;
    s.chief
        .until("the tidied name", |c| {
            c.agent("home", "feline-biologist").is_some()
        })
        .await;
    let names = s
        .chief
        .realm()
        .agents
        .into_iter()
        .map(|a| a.name)
        .collect::<Vec<_>>();
    // Names are tidied, and the refused calls create nobody.
    assert_eq!(names, ["founder", "feline-biologist"]);
    let tools = s
        .chief
        .items(&mayor_chat)
        .into_iter()
        .filter(|i| i.tool.is_some())
        .count();
    assert_eq!(tools, 5);
    assert_eq!(
        std::fs::read_to_string(
            s.home.file("houses/home/1/NOTES.md")
        )
        .unwrap(),
        "# Notes\n"
    );
}

#[tokio::test]
async fn asks_before_a_reserved_creation_and_caps_the_hamlet() {
    let script = Script::new().agent(
        "home:0",
        [
            call("create_agent", json!({"name": "scout", "purpose": "Looks", "reason": "Needed"})),
            words("Asked"),
            call("create_agent", json!({"name": "smith", "purpose": "Builds", "reason": "Needed"})),
            words("Full"),
        ],
    );
    let mut s = Session::start(script).await;
    s.chief
        .limit("home", "mayor", "createAgents", json!(false))
        .await;
    s.chief.tell(mayor(), "Hire scout").await;
    let wait = s
        .chief
        .wait_for("a reserved deed", |w| {
            matches!(
                w.detail,
                WaitDetail::Approval {
                    danger: WaitDanger::Reserved,
                    ..
                }
            )
        })
        .await;
    let WaitDetail::Approval { says, .. } = &wait.detail else {
        unreachable!()
    };
    let answers = &wait.answers;
    assert_eq!(answers, &[Choice::Allow, Choice::Deny]);
    assert_eq!(says, "wants to do what you reserved:");
    s.chief.choose(&wait.id, Choice::Allow, None).await;
    s.chief
        .until("scout created", |c| {
            c.agent("home", "scout").is_some()
        })
        .await;
    s.chief
        .limit("home", "mayor", "createAgents", json!(true))
        .await;
    let reply = s
        .chief
        .request(|id| {
            ClientMessage::Edit(Edit::Limits(EditLimits::Whole {
                hamlet: "home".into(),
                id,
                reason: None,
                whole: [("mostAgents".to_string(), json!(1))].into(),
            }))
        })
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice);
    let mayor_chat = chat(mayor());
    s.chief.open(&mayor_chat).await;
    s.chief.say(mayor(), "Hire smith").await;
    assert!(s.join().await.agent("home", "smith").is_none());
    s.chief.say(mayor(), "/remove scout Done").await;
    let reply = s.chief.say(mayor(), "/restore scout").await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{reply:?}");
}

#[tokio::test]
async fn briefs_each_agent_on_its_hamlet() {
    let mut s = Session::start(society()).await;
    build(&mut s).await;
    s.chief.say(mayor(), "/to scout Explore carefully").await;
    let mayor_prompt = s.chief.prompt(mayor()).await;
    let scout_prompt = s.chief.prompt(to("home", "scout")).await;
    for words in [
        "Agents:\n- scout: Scout\n- smith: Smith\n- poet: Poet\n",
        "The latest:\n- chief told scout: Explore carefully\n",
        "What was decided, latest first:\n- The mayor created \
         `poet`: Write\n- The mayor built a road between `smith` \
         and `scout`: Build\n- The mayor created `smith`: Build\n- \
         The mayor created `scout`: Explore\n",
        "it passes with 2 of 3 of the voters",
    ] {
        assert!(
            mayor_prompt.contains(words),
            "{words}\n{mayor_prompt}"
        );
    }
    assert!(
        scout_prompt.contains("# Your place\n")
            && scout_prompt
                .contains("Your roads lead to: smith, mayor."),
        "{scout_prompt}"
    );
    assert!(!scout_prompt.contains("Agents:\n"), "{scout_prompt}");
}

#[tokio::test]
async fn carries_a_question_across_a_crash_without_asking_twice() {
    let mut s = Session::start(society().agent("home:0", [
        call("ask_agent", json!({"name": "scout", "message": "Do the tests pass?"})),
        words("The scout answered."),
    ]).agent("home:1", [words("Never.").held_until("release")])).await;
    build(&mut s).await;
    s.chief.open(&chat(mayor())).await;
    s.chief.tell(mayor(), "Ask scout.").await;
    s.chief
        .until("scout working on the question", |c| {
            c.agent("home", "scout").is_some_and(|a| a.busy)
        })
        .await;
    let question = peer("scout", 0, ChatRefKind::Questions);
    s.chief.open(&question).await;
    s.chief
        .shows(
            &question,
            "Question from `mayor` (waiting for your answer):\nDo \
             the tests pass?",
        )
        .await;
    s.server.kill().await;
    let server = s
        .home
        .serve(
            &Script::new()
                .agent("home:1", [words("Yes, all green.")])
                .agent("home:0", [words("The scout answered.")]),
            QUIET,
        )
        .await;
    let mut chief = Client::join(&server).await;
    chief.open(&chat(mayor())).await;
    chief.shows(&chat(mayor()), "The scout answered.").await;
    chief.idle("home", "founder").await;
    let page = chief.open(&question).await;
    assert_eq!(
        page.items
            .iter()
            .map(|i| i.text.as_str())
            .collect::<Vec<_>>(),
        [
            "Question from `mayor` (waiting for your answer):\nDo \
             the tests pass?",
            "Yes, all green."
        ]
    );
    assert_eq!(
        chief
            .agent("home", "scout")
            .unwrap()
            .conversations
            .iter()
            .find(|c| c.kind == ConversationViewKind::Questions)
            .unwrap()
            .count,
        1
    );
    let page = chief.open(&chat(mayor())).await;
    assert_eq!(
        page.items
            .iter()
            .filter_map(|i| i.tool.as_ref())
            .filter(|t| t.name == "ask_agent" && t.exit == Some(0))
            .count(),
        1
    );
}

#[tokio::test]
async fn a_failed_peer_turn_does_not_fault_the_agent() {
    let mut s = Session::start(society().agent("home:0", [
        call("tell_agent", json!({"name": "scout", "message": "Explore."})), words("Sent."), words("Noted the failure."),
    ]).agent("home:1", [failure("Peer model unavailable")])).await;
    build(&mut s).await;
    s.chief.say(mayor(), "Send scout.").await;
    let peer_chat = peer("scout", 0, ChatRefKind::Letters);
    s.chief.open(&peer_chat).await;
    s.chief
        .until("the peer failure", |c| {
            c.items(&peer_chat).iter().any(|i| {
                i.kind == ItemKind::Error
                    && i.text.contains("Peer model unavailable")
            })
        })
        .await;
    s.chief.idle("home", "scout").await;
    assert!(!s.chief.agent("home", "scout").unwrap().faulted);
    assert!(s.chief.waits().is_empty(), "{:?}", s.chief.waits());
}

#[tokio::test]
async fn keeps_record_names_and_brief_numbers_after_erasure() {
    let mut s = Session::start(society().agent(
        "home:0",
        [
            call(
                "remove_agent",
                json!({"name": "nobody", "reason": "No work"}),
            ),
            words("Refused."),
        ],
    ))
    .await;
    build(&mut s).await;
    s.chief.say(mayor(), "/remove scout Finished").await;
    s.chief.say(mayor(), "/erase scout").await;
    assert!(s.join().await.agent("home", "scout").is_none());
    let record = s.chief.say(mayor(), "/record").await.text;
    assert!(
        record.contains("between `smith` and `scout`: Build"),
        "{record}"
    );
    let prompt = s.chief.prompt(mayor()).await;
    assert!(
        prompt.contains("between `smith` and `agent #1`: Build"),
        "{prompt}"
    );
    s.chief.open(&chat(mayor())).await;
    s.chief.say(mayor(), "Remove nobody.").await;
    let page = s.chief.open(&chat(mayor())).await;
    assert!(
        page.items
            .iter()
            .filter_map(|i| i.tool.as_ref()?.failure.as_ref())
            .any(|f| f.contains("nobody")),
        "{page:?}"
    );
    let prompt = s.chief.prompt(mayor()).await;
    assert!(prompt.contains("nobody"), "{prompt}");
}

#[tokio::test]
async fn keeps_a_refused_road_beside_six_recent_decisions() {
    let mut s = Session::start(
        society()
            .agent(
                "home:1",
                [
                    call(
                        "request_road",
                        json!({"to":"poet", "reason":"Need verses"}),
                    ),
                    words("Understood."),
                ],
            )
            .agent("home:0", [words("Not yet.")]),
    )
    .await;
    build(&mut s).await;
    s.chief.say(to("home", "scout"), "Ask for a road.").await;
    let brief = s.chief.prompt(to("home", "scout")).await;
    assert!(
        brief.contains(
            "The mayor did not build the road you asked for to \
             `poet`: Not yet."
        ),
        "{brief}"
    );
    for n in 0..4 {
        s.chief
            .say(mayor(), &format!("/road scout poet built-{n}"))
            .await;
        s.chief
            .say(mayor(), &format!("/unroad scout poet removed-{n}"))
            .await;
    }
    let brief = s.chief.prompt(to("home", "scout")).await;
    assert!(
        brief.contains("What touched you:")
            && brief.contains("Not yet."),
        "{brief}"
    );
    assert!(
        !brief.contains(": built-0")
            && !brief.contains(": removed-0"),
        "{brief}"
    );
    for n in 1..4 {
        assert!(
            brief.contains(&format!(": built-{n}"))
                && brief.contains(&format!(": removed-{n}")),
            "{brief}"
        );
    }
    s.chief.limit("home", "mayor", "toldDays", json!(0)).await;
    let brief = s.chief.prompt(to("home", "scout")).await;
    assert!(
        !brief.contains(": built-3")
            && !brief.contains("What touched you:"),
        "{brief}"
    );
}

#[tokio::test]
async fn stops_at_a_model_time_cap_charged_by_a_road() {
    let mut s = Session::start(
        society().agent("home:0", [words("After reset.")]),
    )
    .await;
    build(&mut s).await;
    s.chief.limit("home", "road", "minutes", json!(1)).await;
    let road = s.chief.say(mayor(), "/road scout poet Work").await;
    assert_eq!(road.kind, ReplyKind::Notice, "{road:?}");
    s.chief.limit("home", "budget", "minutes", json!(1)).await;
    s.chief.open(&chat(mayor())).await;
    s.chief.say(mayor(), "No time left.").await;
    let page = s.chief.open(&chat(mayor())).await;
    assert!(
        page.items.iter().any(|i| i.kind == ItemKind::Notice
            && i.text.contains("1m model time of 1m model time")),
        "{page:?}"
    );
    s.chief.say(mayor(), "/reset-budget").await;
    s.chief.say(mayor(), "Answer now.").await;
    s.chief.shows(&chat(mayor()), "After reset.").await;
}

#[tokio::test]
async fn a_mayor_review_holds_the_next_step_across_restart() {
    for choice in [Choice::Go, Choice::Withdraw] {
        let script = society()
            .agent("home:1", [
                call("write", json!({"path":"first.txt","content":"first"})),
                call("write", json!({"path":"second.txt","content":"second"})).held_until("step"),
                words("Finished reviewed work"),
            ])
            .errands([words("OBJECT: Keep the only copy.").held_until("review")]);
        let mut s = Session::start(script).await;
        build(&mut s).await;
        s.chief
            .limit("home", "asking", "mayorReview", json!("always"))
            .await;
        let house = s.home.path().join("houses/home/1");
        s.chief.open(&chat(to("home", "scout"))).await;
        s.chief.tell(to("home", "scout"), "Review this work").await;
        s.chief
            .until("work beside the review", |_| {
                house.join("first.txt").exists()
            })
            .await;
        std::fs::write(s.home.side().join("review"), "").unwrap();
        let wait = s
            .chief
            .wait_for("mayor's objection", |wait| {
                wait.by == "mayor"
                    && matches!(
                        wait.detail,
                        WaitDetail::Objection { .. }
                    )
            })
            .await;
        let Wait { answers, about, .. } = &wait;
        assert_eq!(answers, &[Choice::Go, Choice::Withdraw]);
        assert!(about.contains("Keep the only copy."));
        std::fs::write(s.home.side().join("step"), "").unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(150))
            .await;
        assert!(!house.join("second.txt").exists());
        s.server.kill().await;
        let script = Script::new().agent("home:1", [
            call("write", json!({"path":"second.txt","content":"second"})),
            words("Finished reviewed work"),
        ]).errands([failure("The completed review must not run twice")]);
        let server = s.home.serve(&script, QUIET).await;
        let mut chief = Client::join(&server).await;
        let recovered = chief
            .wait_for("recovered review", |w| w.id == wait.id)
            .await;
        assert_eq!(recovered, wait);
        assert!(!house.join("second.txt").exists());
        chief.open(&chat(to("home", "scout"))).await;
        assert_eq!(
            chief
                .choose(&wait.id, choice.clone(), Some("Checked"))
                .await
                .kind,
            ReplyKind::Notice
        );
        if choice == Choice::Go {
            chief
                .shows(
                    &chat(to("home", "scout")),
                    "Finished reviewed work",
                )
                .await;
            assert_eq!(
                std::fs::read_to_string(house.join("second.txt"))
                    .unwrap(),
                "second"
            );
        } else {
            chief
                .until("withdrawal notice", |c| {
                    c.items(&chat(to("home", "scout"))).iter().any(
                        |item| {
                            item.kind == ItemKind::Notice
                                && item.text
                                    == "The chief withdrew the \
                                        instruction: Checked"
                        },
                    )
                })
                .await;
            assert!(!house.join("second.txt").exists());
        }
        chief.idle("home", "scout").await;
        assert!(chief.realm().waits.is_empty());
    }
}

#[tokio::test]
async fn a_late_mayor_review_only_asks_for_dismissal() {
    let mut s = Session::start(
        society()
            .agent("home:1", [words("Already done")])
            .errands([words("OBJECT: Too late to stop.")
                .held_until("late-review")]),
    )
    .await;
    build(&mut s).await;
    s.chief
        .limit("home", "asking", "mayorReview", json!("always"))
        .await;
    s.chief.say(to("home", "scout"), "Do the work").await;
    s.chief.idle("home", "scout").await;
    std::fs::write(s.home.side().join("late-review"), "").unwrap();
    let wait = s
        .chief
        .wait_for("late review", |w| {
            matches!(w.detail, WaitDetail::Objection { .. })
        })
        .await;
    let Wait { answers, about, .. } = &wait;
    assert_eq!(answers, &[Choice::Dismiss]);
    assert!(about.contains("work had already ended"), "{about}");
    assert_eq!(
        s.chief.choose(&wait.id, Choice::Dismiss, None).await.kind,
        ReplyKind::Notice
    );
}

#[tokio::test]
async fn merges_whole_skills_under_usable_names_and_keeps_original_note_bytes()
 {
    let mut s = Session::start(society()).await;
    build(&mut s).await;
    let kept = s.home.file("houses/home/2");
    let gone = s.home.file("houses/home/1");
    for (root, description) in
        [(&kept, "Kept skill"), (&gone, "Other skill")]
    {
        std::fs::create_dir_all(root.join("skills/map/scripts"))
            .unwrap();
        std::fs::write(
            root.join("skills/map/SKILL.md"),
            format!(
                "---\nname: map\ndescription: \
                 {description}\n---\nRun skills/map/scripts/run.py"
            ),
        )
        .unwrap();
        std::fs::write(
            root.join("skills/map/scripts/run.py"),
            "print('skills/map/data')",
        )
        .unwrap();
        std::fs::write(
            root.join("skills/map/image.bin"),
            [0xff, 0x00, 0x80],
        )
        .unwrap();
    }
    let bad = b"# Notes\nCaf\xe9";
    std::fs::write(kept.join("NOTES.md"), bad).unwrap();
    let refused =
        s.chief.say(mayor(), "/merge scout into smith").await;
    assert!(refused.text.contains("UTF-8"), "{refused:?}");
    assert_eq!(std::fs::read(kept.join("NOTES.md")).unwrap(), bad);
    assert!(
        s.join()
            .await
            .agent("home", "scout")
            .unwrap()
            .left
            .is_none()
    );
    std::fs::write(kept.join("NOTES.md"), "# Notes\nKeep me\n")
        .unwrap();
    std::fs::write(gone.join("NOTES.md"), bad).unwrap();
    let merged =
        s.chief.say(mayor(), "/merge scout into smith").await;
    assert_eq!(merged.kind, ReplyKind::Notice, "{merged:?}");
    assert_eq!(
        std::fs::read(kept.join("merged/scout/NOTES.md")).unwrap(),
        bad
    );
    assert_eq!(
        std::fs::read(kept.join("merged/scout/NOTES.before.md"))
            .unwrap(),
        b"# Notes\nKeep me\n"
    );
    let skill = kept.join("skills/map-from-scout");
    let paper =
        std::fs::read_to_string(skill.join("SKILL.md")).unwrap();
    assert!(paper.contains("name: map-from-scout"), "{paper}");
    assert!(paper.contains("skills/map-from-scout/scripts/run.py"));
    assert_eq!(
        std::fs::read_to_string(skill.join("scripts/run.py"))
            .unwrap(),
        "print('skills/map-from-scout/data')"
    );
    assert_eq!(
        std::fs::read(skill.join("image.bin")).unwrap(),
        [0xff, 0x00, 0x80]
    );
    assert!(
        s.chief
            .prompt(to("home", "smith"))
            .await
            .contains("map-from-scout")
    );
    std::fs::write(
        skill.join("scripts/run.py"),
        "changed after merge",
    )
    .unwrap();
    assert_eq!(
        s.chief.say(mayor(), "/restore scout").await.kind,
        ReplyKind::Notice
    );
    assert_eq!(
        std::fs::read_to_string(kept.join("NOTES.md")).unwrap(),
        "# Notes\nKeep me\n"
    );
    assert_eq!(
        std::fs::read_to_string(skill.join("scripts/run.py"))
            .unwrap(),
        "changed after merge"
    );
    assert!(!skill.join("SKILL.md").exists());
    assert_eq!(std::fs::read(gone.join("NOTES.md")).unwrap(), bad);
}

#[tokio::test]
async fn refuses_a_taken_skill_rename_before_archiving_either_agent()
{
    let mut s = Session::start(society()).await;
    build(&mut s).await;
    for (number, name, description) in [
        (1, "map", "other"),
        (2, "map", "mine"),
        (2, "map-from-scout", "already here"),
    ] {
        let skill = s
            .home
            .file(&format!("houses/home/{number}/skills/{name}"));
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            format!(
                "---\nname: {name}\ndescription: \
                 {description}\n---\nOriginal"
            ),
        )
        .unwrap();
    }
    let original = std::fs::read(
        s.home.file("houses/home/2/skills/map-from-scout/SKILL.md"),
    )
    .unwrap();
    s.chief.say(mayor(), "/merge scout into smith").await;
    let realm = s.join().await;
    assert!(realm.agent("home", "scout").unwrap().left.is_none());
    assert_eq!(
        std::fs::read(
            s.home
                .file("houses/home/2/skills/map-from-scout/SKILL.md")
        )
        .unwrap(),
        original
    );
}

#[tokio::test]
async fn a_stopped_library_offer_releases_its_claim_and_uses_librarian_notes()
 {
    for restart in [false, true] {
        let mut endpoint = openai::Endpoint::start().await;
        let mut s = Session::with_endpoint(&endpoint).await;
        let tool = || openai::Response {
            text: String::new(),
            finish: "tool_calls".into(),
            calls: vec![
                json!({"index":0,"id":"offer","type":"function","function":{"name":"submit_skill","arguments":json!({"skill":"artifact","reason":"Useful"}).to_string()}}),
            ],
            ..Default::default()
        };
        let source = s.home.file("houses/home/0/skills/artifact");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(
            source.join("SKILL.md"),
            "---\nname: artifact\ndescription: Useful \
             artifacts\n---\nMake an artifact",
        )
        .unwrap();
        std::fs::write(
            s.home.file("librarian/NOTES.md"),
            "# Notes\nPrefer reproducible examples over promises.",
        )
        .unwrap();
        endpoint.enqueue([
            tool(),
            openai::Response {
                text: "ACCEPT Useful".into(),
                hold_until: Some(s.home.side().join("never")),
                ..Default::default()
            },
        ]);
        s.chief.tell(mayor(), "Offer artifact").await;
        let requests = endpoint.completions(2).await;
        assert!(
            requests[1].body.as_ref().unwrap()["messages"]
                .to_string()
                .contains(
                    "Prefer reproducible examples over promises."
                )
        );
        s.chief.say(mayor(), "/stop").await;
        s.chief.idle("home", "founder").await;
        assert!(source.join("SKILL.md").exists());
        assert!(!s.home.file("library/artifact").exists());
        if restart {
            s.server.kill().await;
            s.server = s
                .home
                .serve_providers(&[
                    ("HAMLET_REFLECT_EVERY", "0"),
                    ("HAMLET_MODEL", "local::org/large"),
                    ("HAMLET_BASE_URL", &endpoint.url),
                ])
                .await;
            s.chief = Client::join(&s.server).await;
        }
        endpoint.enqueue([
            tool(),
            openai::Response {
                text: "ACCEPT Useful".into(),
                ..Default::default()
            },
            openai::Response {
                text: "Offered again".into(),
                ..Default::default()
            },
        ]);
        s.chief.say(mayor(), "Offer the same artifact again").await;
        s.chief.idle("home", "founder").await;
        assert!(s.home.file("library/artifact/SKILL.md").exists());
        assert!(!source.exists());
        assert_eq!(endpoint.completions(5).await.len(), 5);
    }
}
