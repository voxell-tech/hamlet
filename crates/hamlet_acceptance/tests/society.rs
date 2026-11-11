//! What agents do in the society with their tools: the Hand's and a
//! lead's, letters and questions, workers, and the acts a chief
//! keeps for itself.

use hamlet_acceptance::*;
use hamlet_protocol::client::*;
use serde_json::json;

/// The failures of the tool items of a chat, by tool.
fn failures(
    client: &Client,
    chat: &ChatRef,
) -> Vec<(String, Option<String>)> {
    client
        .items(chat)
        .into_iter()
        .filter_map(|item| item.tool)
        .map(|tool| (tool.name, tool.failure))
        .collect()
}

fn lane(from: AgentId, kind: ChatRefKind, with: AgentId) -> ChatRef {
    ChatRef {
        from: Some(from),
        kind: Some(kind),
        ..chat(with)
    }
}

#[tokio::test]
async fn the_hand_makes_a_scholar_founds_a_hamlet_and_hires_into_it()
{
    let mut s = Session::start(Faux::new().agent(
        1,
        [
            calls([
                (
                    "create_scholar",
                    json!({"name": "bo", "character": "Careful."}),
                ),
                (
                    "found_hamlet",
                    json!({"hamlet": "bog", "scholar": "scholar#2", "purpose": "drain"}),
                ),
            ]),
            words("Founded."),
            calls([
                (
                    "create_scholar",
                    json!({"name": "cy", "character": "Digs."}),
                ),
                (
                    "hire",
                    json!({"scholar": "scholar#3", "purpose": "dig", "hamlet": "bog"}),
                ),
            ]),
            words("Hired."),
            call(
                "seat_mayor",
                json!({"hire": "bog#1", "reason": "cy leads now"}),
            ),
            words("Seated."),
        ],
    ))
    .await;
    let hand = s.hand();
    let now = chat(hand);
    s.chief.open(&now).await;
    s.chief.tell(hand, "Found a hamlet.").await;
    s.chief.shows(&now, "Founded.").await;
    let bo = s.chief.id("bog#0");
    assert_eq!(s.chief.hamlet("bog").unwrap().mayor, bo);
    assert_eq!(s.chief.agent("bog#0").unwrap().parent, Some(hand));
    let scholar = s.chief.scholar("bo").unwrap();
    assert_eq!(scholar.chief, "chief");
    let papers = s.chief.papers(Keeper::Scholar(scholar.id)).await;
    assert_eq!(papers.character, "Careful.");

    s.chief.tell(hand, "Hire cy.").await;
    s.chief.shows(&now, "Hired.").await;
    let cy = s.chief.id("bog#1");
    assert_eq!(s.chief.agent("bog#1").unwrap().parent, Some(bo));

    s.chief.tell(hand, "Seat cy.").await;
    s.chief.shows(&now, "Seated.").await;
    assert_eq!(s.chief.hamlet("bog").unwrap().mayor, cy);
    assert_eq!(s.chief.agent("bog#0").unwrap().parent, Some(cy));
    assert!(
        failures(&s.chief, &now).iter().all(|(_, f)| f.is_none())
    );
}

#[tokio::test]
async fn a_lead_runs_the_hires_under_it_and_nothing_beyond_them() {
    let mut s = Session::start(Faux::new().agent(
        2,
        [
            calls([
                (
                    "create_scholar",
                    json!({"name": "cy", "character": "Digs."}),
                ),
                (
                    "create_scholar",
                    json!({"name": "dee", "character": "Hauls."}),
                ),
            ]),
            calls([
                ("hire", json!({"scholar": "scholar#3", "purpose": "dig"})),
                ("hire", json!({"scholar": "scholar#4", "purpose": "haul"})),
            ]),
            calls([
                (
                    "set_tools",
                    json!({"hire": "docs#1", "off": ["bash"], "reason": "no shell"}),
                ),
                (
                    "set_allowance",
                    json!({"hire": "docs#1", "dollars": 2.5, "reason": "small"}),
                ),
                (
                    "set_model",
                    json!({"hire": "docs#1", "model": "faux::faux-1", "reason": "same"}),
                ),
                (
                    "nickname",
                    json!({"hire": "docs#1", "nickname": "Spade", "reason": "short"}),
                ),
                (
                    "lay_road",
                    json!({"a": "docs#1", "b": "docs#2", "reason": "share"}),
                ),
            ]),
            calls([
                (
                    "take_up_road",
                    json!({"a": "docs#1", "b": "docs#2", "reason": "done"}),
                ),
                (
                    "end_hire",
                    json!({"hire": "docs#2", "reason": "finished"}),
                ),
                ("read_record", json!({"hamlet": "docs"})),
                (
                    "end_hire",
                    json!({"hire": "hand", "reason": "not mine"}),
                ),
            ]),
            words("Led."),
        ],
    ))
    .await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    let now = chat(ada);
    s.chief.open(&now).await;
    s.chief.tell(ada, "Lead.").await;
    s.chief.shows(&now, "Led.").await;

    let cy = s.chief.agent("docs#1").unwrap();
    assert_eq!((cy.kind, cy.parent), (AgentKind::Hire, Some(ada)));
    assert_eq!(cy.nickname.as_deref(), Some("Spade"));
    // A chief above it names it too, and an empty name takes the
    // nickname away.
    assert_eq!(
        s.chief.command(ada, "/nickname docs#1 Pick").await,
        "`docs#1` is called Pick now."
    );
    assert_eq!(
        s.chief.command(ada, "/nickname docs#1").await,
        "`docs#1` goes by its name again."
    );
    s.chief
        .until("the nickname gone", |c| {
            c.agent("docs#1").is_some_and(|cy| cy.nickname.is_none())
        })
        .await;
    assert_eq!(
        s.chief.command(ada, "/tools docs#1").await,
        "`docs#1` has these off: bash"
    );
    s.chief
        .until("its tools off shown", |c| {
            c.agent("docs#1")
                .is_some_and(|cy| cy.tools_off == ["bash"])
        })
        .await;
    // A chief chooses how any agent thinks, its tier and its model.
    assert_eq!(
        s.chief.command(ada, "/effort docs#1 xhigh").await,
        "`docs#1` thinks at xhigh."
    );
    assert_eq!(
        s.chief.command(ada, "/fast docs#1 on").await,
        "`docs#1` asks for the fast tier: on."
    );
    assert_eq!(
        s.chief.command(ada, "/model hand faux::faux-1").await,
        "`hand` runs faux::faux-1."
    );
    s.chief
        .until("the choices shown", |c| {
            c.agent("docs#1").is_some_and(|cy| {
                cy.effort == Some(hamlet_protocol::Effort::XHigh)
                    && cy.fast
            }) && c.agent("hand").is_some_and(|hand| {
                hand.own_model.as_deref() == Some("faux::faux-1")
            })
        })
        .await;
    let dee = s.chief.agent("docs#2").unwrap();
    assert_eq!(dee.left.unwrap().how, LeftHow::Ended);
    let record = s.chief.command(ada, "/history").await;
    for line in [
        "docs#0 (ada) hired docs#1 (cy)",
        "docs#0 (ada) set the allowance of docs#1 (cy): small",
        "docs#0 (ada) chose the model of docs#1 (cy): same",
        "docs#0 (ada) nicknamed docs#1 (cy): short",
        "docs#0 (ada) laid a road from docs#1 (cy): share",
        "docs#0 (ada) took up a road from docs#1 (cy): done",
        "docs#0 (ada) ended docs#2 (dee): finished",
    ] {
        assert!(
            record.contains(line),
            "{line} missing from:\n{record}"
        );
    }

    // The Hand's tools and other hamlets' agents are not a lead's.
    let outcome = failures(&s.chief, &now)
        .into_iter()
        .filter(|(_, failure)| failure.is_some())
        .map(|(tool, _)| tool)
        .collect::<Vec<_>>();
    assert_eq!(outcome, ["read_record", "end_hire"]);
}

#[tokio::test]
async fn letters_and_questions_go_along_roads_and_a_question_back_is_refused()
 {
    let mut s = Session::start(
        Faux::new()
            .agent(
                2,
                [
                    call(
                        "tell",
                        json!({"to": "docs#1", "text": "Hello cy"}),
                    ),
                    words("Told."),
                    call(
                        "ask",
                        json!({"to": "docs#1", "question": "Which colour?"}),
                    ),
                    words("cy said blue."),
                ],
            )
            .agent(
                3,
                [
                    words("Noted."),
                    call(
                        "ask",
                        json!({"to": "docs#0", "question": "And you?"}),
                    ),
                    words("Blue."),
                ],
            ),
    )
    .await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    let cy = s.hire(ada, "cy", "dig").await;
    let now = chat(ada);
    s.chief.open(&now).await;

    s.chief.tell(ada, "Tell cy hello.").await;
    s.chief.shows(&now, "Told.").await;
    let letters = lane(ada, ChatRefKind::Letters, cy);
    s.chief
        .until("cy answered the letter", |c| {
            c.agent("docs#1").is_some_and(|a| {
                !a.busy && !a.conversations.is_empty()
            })
        })
        .await;
    let page = s.chief.open(&letters).await;
    let texts = page
        .items
        .iter()
        .map(|i| {
            (i.kind.clone(), i.text.as_str(), i.author.as_deref())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        texts,
        [
            (ItemKind::User, "Hello cy", Some("docs#0 (ada)")),
            (ItemKind::Assistant, "Noted.", None)
        ]
    );

    // The question waits for its answer; cy cannot ask ada back while
    // ada waits on it.
    s.chief.tell(ada, "Ask cy a question.").await;
    s.chief.shows(&now, "cy said blue.").await;
    let questions = lane(ada, ChatRefKind::Questions, cy);
    let page = s.chief.open(&questions).await;
    assert_eq!(page.items.first().unwrap().text, "Which colour?");
    assert_eq!(page.items.last().unwrap().text, "Blue.");
    let back = failures(&s.chief, &questions);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].0, "ask");
    assert!(back[0].1.is_some(), "asking back is refused: {back:?}");
    let cy = s.chief.agent("docs#1").unwrap();
    let kinds = cy
        .conversations
        .iter()
        .map(|c| c.kind.clone())
        .collect::<Vec<_>>();
    assert!(kinds.contains(&ConversationViewKind::Letters));
    assert!(kinds.contains(&ConversationViewKind::Questions));
}

#[tokio::test]
async fn a_worker_answers_its_starter_once_and_ends() {
    let mut s = Session::start(
        Faux::new()
            .agent(
                2,
                [
                    call(
                        "start_worker",
                        json!({"task": "count the files", "wait": true}),
                    ),
                    words("The worker said 42."),
                    call(
                        "start_worker",
                        json!({"task": "count again"}),
                    ),
                    words("Started another."),
                    words("The second answer came."),
                ],
            )
            .kind("worker", [words("42"), words("43")]),
    )
    .await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    let now = chat(ada);
    s.chief.open(&now).await;
    s.chief.tell(ada, "Count.").await;
    s.chief.shows(&now, "The worker said 42.").await;
    let worker = s.chief.agent("worker#3").unwrap();
    assert_eq!(worker.kind, AgentKind::Worker);
    assert_eq!(worker.parent, Some(ada));
    assert_eq!(worker.left.unwrap().how, LeftHow::Answered);

    // Without waiting, the answer comes later as a letter.
    s.chief.tell(ada, "Count again.").await;
    s.chief.shows(&now, "Started another.").await;
    s.chief.shows(&now, "worker#4 answers: 43").await;
    s.chief.shows(&now, "The second answer came.").await;
    assert_eq!(
        s.chief.agent("worker#4").unwrap().left.unwrap().how,
        LeftHow::Answered
    );
}

#[tokio::test]
async fn what_the_hand_reserves_for_a_chief_waits_for_one() {
    let mut s = Session::start(Faux::new().agent(
        1,
        [
            call(
                "edit_scholar",
                json!({"scholar": "scholar#2", "path": "memory/farm.md",
                       "content": "---\ndescription: The farm\n---\nWheat.\n",
                       "message": "Remember the farm."}),
            ),
            words("Edited."),
            call(
                "remove_hamlet",
                json!({"hamlet": "docs", "reason": "finished"}),
            ),
            words("Removal over."),
            call(
                "remove_hamlet",
                json!({"hamlet": "docs", "reason": "again"}),
            ),
            words("Removed."),
        ],
    ))
    .await;
    let hand = s.hand();
    let ada = s.found("docs", "ada", "keep the docs").await;
    let _ = ada;
    let now = chat(hand);
    s.chief.open(&now).await;
    s.chief.tell(hand, "Remember the farm.").await;
    let edit = s
        .chief
        .wait_for("the edit", |w| w.agent == Some(hand))
        .await;
    let WaitDetail::Approval { danger, .. } = &edit.detail else {
        panic!("an approval: {edit:?}");
    };
    assert_eq!(danger, &WaitDanger::Reserved);
    let scholar = s.chief.scholar("ada").unwrap();
    assert!(
        s.chief
            .papers(Keeper::Scholar(scholar.id))
            .await
            .memory
            .is_empty()
    );
    s.chief.command(hand, "/allow").await;
    s.chief.shows(&now, "Edited.").await;
    let papers = s.chief.papers(Keeper::Scholar(scholar.id)).await;
    assert_eq!(papers.memory.len(), 1);
    assert_eq!(papers.memory[0].path, "farm.md");
    assert_eq!(papers.memory[0].description, "The farm");

    // A refusal leaves the hamlet standing; the next ask may succeed.
    s.chief.tell(hand, "Remove the docs.").await;
    s.chief
        .wait_for("the removal", |w| w.agent == Some(hand))
        .await;
    s.chief.command(hand, "/deny keep it").await;
    s.chief.shows(&now, "Removal over.").await;
    assert!(s.chief.hamlet("docs").is_some());
    s.chief.tell(hand, "Remove the docs after all.").await;
    s.chief
        .wait_for("the removal again", |w| w.agent == Some(hand))
        .await;
    s.chief.command(hand, "/allow").await;
    s.chief.shows(&now, "Removed.").await;
    assert!(s.chief.hamlet("docs").is_none());
    assert_eq!(
        s.chief.agent("docs#0").unwrap().left.unwrap().how,
        LeftHow::Hamlet
    );
}

#[tokio::test]
async fn two_mayors_agree_a_road_between_their_hamlets() {
    let mut s = Session::start(
        Faux::new()
            .agent(
                2,
                [
                    call(
                        "agree_road",
                        json!({"mine": "docs#1", "theirs": "mill#1", "reason": "share files"}),
                    ),
                    words("Agreed."),
                ],
            )
            .agent(
                3,
                [
                    call(
                        "agree_road",
                        json!({"mine": "mill#1", "theirs": "docs#1", "reason": "share files"}),
                    ),
                    words("Agreed too."),
                ],
            ),
    )
    .await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    let bo = s.found("mill", "bo", "grind").await;
    s.hire(ada, "cy", "dig").await;
    s.hire(bo, "dee", "haul").await;
    let agreed = |c: &Client| {
        c.realm().hamlets.iter().any(|h| {
            h.roads.iter().any(|r| r.kind == RoadKind::Agreed)
        })
    };
    s.chief.open(&chat(ada)).await;
    s.chief.open(&chat(bo)).await;
    s.chief.tell(ada, "Agree the road.").await;
    s.chief.shows(&chat(ada), "Agreed.").await;
    assert!(!agreed(&s.chief), "one mayor alone opens no road");
    s.chief.tell(bo, "Agree the road.").await;
    s.chief.shows(&chat(bo), "Agreed too.").await;
    s.chief.until("the road opens", agreed).await;
}

#[tokio::test]
async fn a_mayors_blank_hire_stays_out_of_the_city_and_the_harbor() {
    // The mayor, agent 2, hires a blank agent.
    let mut s = Session::start(Faux::new().agent(
        2,
        [
            call(
                "hire",
                json!({"name": "sam", "purpose": "read the docs"}),
            ),
            words("Hired sam."),
        ],
    ))
    .await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    s.chief.open(&chat(ada)).await;
    s.chief.tell(ada, "Hire a reader.").await;
    s.chief.shows(&chat(ada), "Hired sam.").await;
    assert!(s.chief.agent("docs#1").is_some());
    let sam = s.chief.scholar("sam").unwrap();
    assert_eq!((sam.arrival, sam.house), (Arrival::Apart, None));
    assert!(s.chief.waits().is_empty(), "{:?}", s.chief.waits());
}
