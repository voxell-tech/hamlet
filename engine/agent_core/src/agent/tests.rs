use std::path::PathBuf;

use hamlet_home::Home;
use hamlet_model::provider::{Completion, Replay};
use hamlet_rules::civic::Civic;
use hamlet_rules::hamlet::{FOUNDER, HOME};
use hamlet_rules::realm::Origin;
use hamlet_rules::vote::{Choice, Motion, Standing};
use hamlet_sandbox::Unsandboxed;

use super::*;
use crate::testing::{
    ALICE, bash, call, hamlet_of_two, limit, place, says,
};

/// An agent in `home` whose model answers with `replies`.
fn open(home: &Home, replies: Vec<Value>) -> Agent {
    let tools = Tools::new(home.clone(), &Unsandboxed);
    let path = home.sessions().join("test.jsonl");
    Agent::open(
        Arc::new(Replay::new(replies)),
        tools,
        "test".into(),
        &path,
        vec![],
    )
    .unwrap()
}

/// The mayor of `hamlet`, in `home`.
fn mayor(home: &Home, hamlet: &Hamlet, replies: Vec<Value>) -> Agent {
    let mut mayor = open(home, replies);
    mayor.hamlet = Some(hamlet.again());
    mayor
}

/// The answer of `agent` to `text`, and the events of its turn.
async fn heard(
    agent: &mut Agent,
    text: &str,
) -> (String, Vec<Event>) {
    let mut events = vec![];
    let reply = agent
        .turn(None, ALICE, text, &mut |e| events.push(e))
        .await
        .unwrap();
    (reply, events)
}

/// The answer of `agent` to `text`.
async fn ask(agent: &mut Agent, text: &str) -> String {
    heard(agent, text).await.0
}

/// The notes of the agent `name` of the first hamlet.
fn notes(home: &Home, name: &str) -> PathBuf {
    home.root
        .join("agents")
        .join(name)
        .join("workspace/NOTES.md")
}

/// Gives the librarian and the scout a note each.
fn with_notes(home: &Home) {
    std::fs::write(
        notes(home, "librarian"),
        "# Notes\nAlice likes tea.\n",
    )
    .unwrap();
    std::fs::write(
        notes(home, "scout"),
        "# Notes\nThe wiki is slow.\n",
    )
    .unwrap();
}

/// The event at the bottom of `event`, however deep among the agents
/// it happened.
fn innermost(event: &Event) -> &Event {
    match event {
        Event::Agent { event, .. } => innermost(event),
        other => other,
    }
}

/// A model whose reply of no words was cut off for want of room.
struct Cramped(Replay);

impl Provider for Cramped {
    fn complete<'a>(
        &'a self,
        messages: &'a [Value],
        tools: &'a [Value],
        model: Option<&'a str>,
        on_delta: &'a mut (dyn FnMut(Delta<'_>) + Send),
    ) -> futures::future::BoxFuture<'a, Result<Completion>> {
        Box::pin(async move {
            let mut done = self
                .0
                .complete(messages, tools, model, on_delta)
                .await?;
            if done.message["content"] == "" {
                done.call.finish_reason = Some("length".into());
            }
            Ok(done)
        })
    }
}

#[tokio::test]
async fn a_conversation_with_no_room_left_is_summarised() {
    let home = place("agent-compact");
    let replies = vec![
        says("hi"),
        says(""),
        says("They greeted."),
        says("done"),
    ];
    let mut agent = open(&home, vec![]);
    agent.provider = Arc::new(Cramped(Replay::new(replies)));
    ask(&mut agent, "hello").await;
    let (reply, events) = heard(&mut agent, "more").await;
    assert_eq!(reply, "done");
    assert!(events.iter().any(|e| matches!(e, Event::Notice(_))));
    // The message that had no answer yet is kept in its own words.
    let stands = agent.messages[0]["content"].as_str().unwrap();
    assert!(stands.contains("They greeted."));
    assert!(stands.ends_with("more"));
    assert_eq!(agent.messages.len(), 2);
    assert_eq!(open(&home, vec![]).messages, agent.messages);
}

#[tokio::test]
async fn a_turn_that_failed_is_carried_on_from_where_it_ended() {
    let home = place("agent-again");
    // The model has no second reply, so the turn fails after its
    // tool.
    let mut agent = open(&home, vec![bash("echo hi")]);
    assert!(
        agent.turn(None, ALICE, "work", &mut |_| {}).await.is_err()
    );
    agent.provider = Arc::new(Replay::new(vec![says("done")]));
    assert_eq!(agent.again(&mut |_| {}).await.unwrap(), "done");
    let asked = agent
        .messages
        .iter()
        .filter(|m| m["role"] == "user")
        .count();
    assert_eq!(asked, 1);
    assert!(agent.again(&mut |_| {}).await.is_err());
}

#[tokio::test]
async fn tool_round_trip_is_logged_and_resumable() {
    let home = place("agent-loop");
    let responses =
        vec![bash("echo hi > f.txt; cat f.txt"), says("done")];
    let mut agent = open(&home, responses);
    let (reply, events) = heard(&mut agent, "make a file").await;
    assert_eq!(reply, "done");
    assert!(matches!(events[0], Event::ModelCall(_)));
    assert!(matches!(events[1], Event::Tool { .. }));
    assert!(matches!(
        events[2],
        Event::ToolDone { exit_code: 0, .. }
    ));
    let logged = log::read_events(agent.log_path()).unwrap();
    assert_eq!(
        logged.iter().filter(|e| e["type"] == "model_call").count(),
        2
    );
    assert!(home.workspace.join("f.txt").exists());

    // Context is rebuilt from the log alone.
    let resumed = open(&home, vec![]);
    assert_eq!(resumed.messages.len(), 4);
    assert_eq!(resumed.messages[2]["role"], "tool");
    assert!(
        resumed.messages[2]["content"]
            .as_str()
            .unwrap()
            .contains("hi")
    );
}

#[tokio::test]
async fn gate_blocks_a_tool_call() {
    let home = place("agent-gate");
    let responses = vec![bash("touch blocked.txt"), says("ok")];
    let mut agent = open(&home, responses);
    agent.gate = Some(Box::new(|request| {
        let ruling = if request.call.name == "bash" {
            Ruling::Deny("not allowed".into())
        } else {
            Ruling::Go(Leave::default())
        };
        Box::pin(async move { ruling })
    }));
    ask(&mut agent, "go").await;
    assert!(!home.workspace.join("blocked.txt").exists());
    assert!(
        agent.messages[2]["content"]
            .as_str()
            .unwrap()
            .contains("blocked: not allowed")
    );
}

#[tokio::test]
async fn provider_failure_aborts_turn_and_keeps_history_valid() {
    let home = place("agent-abort");
    let mut agent = open(&home, vec![bash("true")]);
    let failed = agent.turn(None, ALICE, "go", &mut |_| {}).await;
    assert!(failed.is_err());
    let events = log::read_events(agent.log_path()).unwrap();
    assert_eq!(events.last().unwrap()["type"], "turn_aborted");
    // Model call records are not part of the conversation.
    assert_eq!(open(&home, vec![]).messages.len(), 3);
}
#[tokio::test]
async fn the_mayor_creates_an_agent_and_gives_it_work() {
    let home = place("agent-mayor");
    let responses = vec![
        call(
            "create_agent",
            json!({"name": "librarian", "purpose": "Keeps the reading list.", "reason": "asked", "character": "Terse."}),
        ),
        call(
            "ask_agent",
            json!({"name": "librarian", "message": "Add Dune."}),
        ),
        // The librarian's own turn, in its own house.
        bash("echo Dune >> list.txt"),
        says("Added Dune."),
        says("The librarian has it."),
    ];
    let hamlet = Hamlet::new(home.root.clone());
    let mut mayor = mayor(&home, &hamlet, responses);
    let (reply, events) = heard(&mut mayor, "track my reading").await;
    assert_eq!(reply, "The librarian has it.");

    let house = home.root.join("agents/librarian");
    assert!(house.join("workspace/list.txt").exists());
    assert!(!home.workspace.join("list.txt").exists());
    assert!(house.join("sessions/mayor.jsonl").exists());
    assert_eq!(
        std::fs::read_to_string(house.join("CHARACTER.md")).unwrap(),
        "Terse.\n"
    );
    assert!(events.iter().any(|e| matches!(
        e,
        Event::AgentCreated { name, .. } if name == "librarian"
    )));
    assert!(events.iter().any(|e| matches!(
        e,
        Event::Agent { name, event }
            if name == "librarian" && matches!(**event, Event::Tool { .. })
    )));
    // The mayor got the agent's reply as the tool's result.
    let result = mayor
        .messages
        .iter()
        .rfind(|m| m["role"] == "tool")
        .unwrap();
    assert_eq!(result["content"], "Added Dune.");
}

#[tokio::test]
async fn a_skill_goes_to_the_library_when_the_librarian_takes_it() {
    use hamlet_rules::library::Library;

    let home = place("agent-submit");
    let hamlet = Hamlet::new(home.root.clone());
    let library = Library::open(&home.root).unwrap();
    for name in ["tides", "mine"] {
        let at = home.skills().join(name);
        std::fs::create_dir_all(&at).unwrap();
        std::fs::write(
            at.join("SKILL.md"),
            format!(
                "---\nname: {name}\ndescription: Tells the \
                 tide.\n---\nRun it.\n"
            ),
        )
        .unwrap();
    }
    let offer = |skill: &str| {
        call(
            "submit_skill",
            json!({"skill": skill, "reason": "Ports need it."}),
        )
    };
    let responses = vec![
        offer("tides"),
        says("ACCEPT. Clear, and any agent could run it."),
        offer("mine"),
        says("REFUSE: the shelves have `tides` for that."),
        offer("nothing"),
        call(
            "ask_librarian",
            json!({"question": "How high is the tide?"}),
        ),
        says("Use `tides`: it tells the tide."),
        says("Done."),
    ];
    let mut mayor = open(&home, responses);
    mayor.tools = Tools::new(home.clone(), &Unsandboxed)
        .with_library(library.clone());
    mayor.hamlet = Some(hamlet.again());
    ask(&mut mayor, "share them").await;

    // Taken, it is the library's and gone from the agent's own.
    assert!(library.has("tides"));
    assert!(!home.skills().join("tides").exists());
    // Refused, it stays, and the agent is told why.
    assert!(!library.has("mine"));
    assert!(home.skills().join("mine/SKILL.md").is_file());
    let told = tool_outputs(&mayor);
    assert!(told[0].contains("took `tides`"), "{told:?}");
    assert!(told[1].contains("the shelves have"), "{told:?}");
    assert!(told[2].contains("no skill in skills/nothing"));
    // Asked, the librarian says what the shelves hold.
    assert!(told[3].contains("Use `tides`"), "{told:?}");
    let kinds = library
        .events()
        .iter()
        .map(|e| (e["type"].to_string(), e["by"].to_string()))
        .collect::<Vec<_>>();
    assert_eq!(kinds[0].0, "\"skill_shelved\"");
    assert_eq!(kinds[1].0, "\"skill_refused\"");
    assert!(kinds[0].1.contains("@home"), "{kinds:?}");
    // The librarian's three answers were paid by who asked.
    let calls = budget::Ledger::new(&home.root)
        .since(chrono::NaiveDate::MIN)
        .len();
    assert_eq!(calls, 8);
}

#[tokio::test]
async fn agents_talk_only_along_roads_and_the_mayor_builds_them() {
    let home = place("agent-roads");
    let hamlet = hamlet_of_two(&home);
    let responses = vec![
        call(
            "ask_agent",
            json!({"name": "librarian", "message": "Find out who wrote Dune."}),
        ),
        // The librarian has no road to the scout yet.
        call(
            "ask_agent",
            json!({"name": "scout", "message": "Who wrote Dune?"}),
        ),
        call(
            "request_road",
            json!({"to": "scout", "reason": "I will ask it often."}),
        ),
        // The mayor, asked by the librarian, agrees.
        call(
            "build_road",
            json!({"a": "librarian", "b": "scout", "reason": "They work together."}),
        ),
        says("Built."),
        call(
            "ask_agent",
            json!({"name": "scout", "message": "Who wrote Dune?"}),
        ),
        says("Frank Herbert."),
        says("The scout says Frank Herbert."),
        says("Frank Herbert wrote it."),
    ];
    let mut mayor = mayor(&home, &hamlet, responses);
    let (reply, events) = heard(&mut mayor, "who wrote Dune?").await;
    assert_eq!(reply, "Frank Herbert wrote it.");

    assert!(hamlet.has_road("librarian", "scout"));
    let carried = |a: &str, b: &str| {
        hamlet
            .roads()
            .into_iter()
            .find(|r| r.joins(a, b))
            .map(|r| r.messages)
    };
    // To the librarian, and its request to the mayor.
    assert_eq!(carried(MAYOR, "librarian"), Some(2));
    assert_eq!(carried("librarian", "scout"), Some(1));
    assert_eq!(carried(MAYOR, "scout"), Some(0));

    assert!(events.iter().any(|e| matches!(
        innermost(e),
        Event::RoadBuilt { a, b } if a == "librarian" && b == "scout"
    )));
    // The refusal reached the librarian as its tool's result.
    let log = log::read_events(
        &home.root.join("agents/librarian/sessions/mayor.jsonl"),
    )
    .unwrap();
    assert!(log.iter().any(|e| {
        e["type"] == "tool"
            && e["output"]
                .as_str()
                .is_some_and(|o| o.contains("no road to `scout`"))
    }));
}

#[tokio::test]
async fn a_task_runs_out_of_messages() {
    let home = place("agent-cap");
    let hamlet = hamlet_of_two(&home);
    limit(&home, "[task]\nmessages = 1\n");
    let again = || {
        call(
            "ask_agent",
            json!({"name": "scout", "message": "Anything."}),
        )
    };
    let responses = vec![
        again(),
        says("Nothing."),
        again(),
        says("I could not ask again."),
    ];
    let mut mayor = mayor(&home, &hamlet, responses);
    ask(&mut mayor, "go").await;
    assert_eq!(mayor.messages[2]["content"], "Nothing.");
    let refused = mayor.messages[4]["content"].as_str().unwrap();
    assert!(
        refused.contains("used all 1 of its messages"),
        "{refused}"
    );
}

/// The text the latest tool call returned.
fn last_result(agent: &Agent) -> String {
    agent
        .messages
        .iter()
        .rfind(|m| m["role"] == "tool")
        .and_then(|m| m["content"].as_str())
        .unwrap_or_default()
        .to_string()
}

#[tokio::test]
async fn a_hamlet_out_of_budget_stops_the_turn_and_says_who_can_change_it()
 {
    let home = place("agent-spent");
    let hamlet = hamlet_of_two(&home);
    limit(&home, "[budget]\ncost = 0.000001\n");
    let mut mayor =
        mayor(&home, &hamlet, vec![says("One."), says("Two.")]);
    // Nothing is spent yet, so the first call is made.
    let reply = ask(&mut mayor, "hi").await;
    assert_eq!(reply, "One.");

    let (reply, events) = heard(&mut mayor, "again").await;
    assert!(
        reply.starts_with(
            "[stopped: The hamlet has spent its budget for today"
        ) && reply.contains("Only the chief can raise it"),
        "{reply}"
    );
    assert!(!events.iter().any(|e| matches!(e, Event::ModelCall(_))));
    // The statement is in the log, so the history stays whole.
    let resumed = open(&home, vec![]);
    assert_eq!(
        resumed.messages.last().unwrap()["content"],
        reply.as_str()
    );

    // The spend is counted from the ledger, as after a restart.
    let report = Hamlet::new(home.root.clone()).report("own");
    assert!(report.hamlet.spent.tokens > 1);
    // Agents that did nothing cost nothing (H16).
    assert_eq!(
        report.agent("scout").unwrap().spent,
        budget::Amount::default()
    );

    // The chief raises the budget and the work goes on.
    limit(&home, "[budget]\ncost = 1.0\n");
    let reply = ask(&mut mayor, "now").await;
    assert_eq!(reply, "Two.");
}

#[tokio::test]
async fn a_budget_lowered_in_the_middle_of_a_turn_stops_its_next_call()
 {
    let home = place("agent-midturn");
    let hamlet = hamlet_of_two(&home);
    let lower = format!(
        "printf '[budget]\\ncost = 0.000001\\n' > '{}'",
        hamlet_home::limits::path(&home.root).display()
    );
    let responses = vec![
        call("bash", json!({"command": lower})),
        says("never said"),
    ];
    let mut mayor = mayor(&home, &hamlet, responses);
    let reply = ask(&mut mayor, "go").await;
    assert!(
        reply.contains("The hamlet has spent its budget"),
        "{reply}"
    );
}

#[tokio::test]
async fn limits_that_cannot_be_read_stop_all_spending() {
    let home = place("agent-unreadable");
    let hamlet = hamlet_of_two(&home);
    limit(&home, "most_agent = 3\n");
    let mut mayor = mayor(&home, &hamlet, vec![says("never said")]);
    let reply = ask(&mut mayor, "go").await;
    assert!(
        reply.starts_with(
            "[stopped: The chief's limits cannot be read"
        ),
        "{reply}"
    );
}

#[tokio::test]
async fn a_task_carries_its_spend_cap_along_every_agent_it_touches() {
    let home = place("agent-taskcap");
    let hamlet = hamlet_of_two(&home);
    limit(&home, "[task]\ncost = 0.000001\n");
    let responses = vec![
        call(
            "ask_agent",
            json!({"name": "scout", "message": "Look it up."}),
        ),
        says("Fresh."),
    ];
    let mut mayor = mayor(&home, &hamlet, responses);
    let reply = ask(&mut mayor, "go").await;
    // The mayor's one call spent the cap, so the scout stopped
    // before its first call and then so did the mayor.
    for said in [last_result(&mayor), reply] {
        assert!(
            said.starts_with("[stopped: This task has spent its cap"),
            "{said}"
        );
    }
    // The next instruction is a new task with its own cap.
    let reply = ask(&mut mayor, "again").await;
    assert_eq!(reply, "Fresh.");
}

#[tokio::test]
async fn the_chief_can_reserve_what_the_mayor_does_alone() {
    let home = place("agent-reserved");
    let hamlet = hamlet_of_two(&home);
    limit(
        &home,
        "[mayor]\ncreate_agents = false\nbuild_roads = false\n",
    );
    let responses = vec![
        call(
            "create_agent",
            json!({"name": "clerk", "purpose": "Files things.", "reason": "x"}),
        ),
        call(
            "build_road",
            json!({"a": "librarian", "b": "scout", "reason": "x"}),
        ),
        says("I need the chief."),
    ];
    let mut mayor = mayor(&home, &hamlet, responses);
    ask(&mut mayor, "grow").await;
    let created = mayor.messages[2]["content"].as_str().unwrap();
    assert!(
        created.contains("the chief has reserved creating an agent"),
        "{created}"
    );
    assert!(
        last_result(&mayor)
            .contains("the chief has reserved building a road")
    );
    assert_eq!(hamlet.residents().len(), 2);
    assert!(!hamlet.has_road("librarian", "scout"));
}

#[tokio::test]
async fn the_mayor_chooses_an_allowed_model_and_the_agent_runs_it() {
    let home = place("agent-models");
    limit(&home, "models = [\"small\"]\n");
    std::fs::write(
        hamlet_model::catalogue::path(&home.root),
        "[[model]]\nid = \"small\"\nallowed = true\n",
    )
    .unwrap();
    let responses = vec![
        call(
            "create_agent",
            json!({"name": "clerk", "purpose": "Files things.", "reason": "x", "model": "small"}),
        ),
        call(
            "set_model",
            json!({"name": "clerk", "model": "huge", "reason": "x"}),
        ),
        call(
            "ask_agent",
            json!({"name": "clerk", "message": "File this."}),
        ),
        says("Filed."),
        says("Done."),
    ];
    let mut mayor = open(&home, responses);
    let hamlet = Hamlet::new(home.root.clone());
    mayor.hamlet = Some(hamlet.again());
    ask(&mut mayor, "go").await;
    assert!(
        mayor.messages[4]["content"]
            .as_str()
            .unwrap()
            .contains("may not run `huge`")
    );
    // The model each call ran, by agent, from the ledger.
    let ran = |agent: &str| {
        budget::Ledger::new(&home.root)
            .since(chrono::NaiveDate::MIN)
            .into_iter()
            .filter(|(_, line)| line["agent"] == agent)
            .map(|(_, line)| {
                line["model"].as_str().unwrap().to_string()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(ran("clerk"), ["small"]);
    assert!(ran(MAYOR).iter().all(|m| m == "replay"));
    assert_eq!(
        hamlet.report("own").agent("clerk").unwrap().model,
        "small"
    );

    // The chief takes the model off the list while the agent
    // uses it: its next call runs the hamlet's own.
    limit(&home, "models = []\n");
    let mut mayor = open(
        &home,
        vec![
            call(
                "ask_agent",
                json!({"name": "clerk", "message": "And this."}),
            ),
            says("Filed too."),
            says("Done."),
        ],
    );
    mayor.hamlet = Some(hamlet.again());
    ask(&mut mayor, "more").await;
    assert_eq!(ran("clerk"), ["small", "replay"]);
    assert!(hamlet.budget_words(MAYOR, true).contains(
        "may no longer run `small`, so `clerk` runs the hamlet's \
         own model"
    ));
}

fn tool_outputs(agent: &Agent) -> Vec<String> {
    agent
        .messages
        .iter()
        .filter(|m| m["role"] == "tool")
        .filter_map(|m| m["content"].as_str().map(String::from))
        .collect()
}

#[tokio::test]
async fn a_mayor_cannot_merge_or_remove_the_voters_while_they_vote() {
    let home = place("agent-vote-holds");
    let hamlet = hamlet_of_two(&home);
    hamlet.create("smith", "Mends.", "test").unwrap();
    // A vote on the mayor is open, as when another agent's turn
    // is asking the voters one by one.
    vote::call(
        &hamlet,
        &Rules::default(),
        "librarian",
        Motion::ReplaceMayor {
            candidate: "scout".into(),
        },
        "It lost my list.",
    )
    .unwrap();
    let responses = vec![
        call(
            "merge_agents",
            json!({"keep": "smith", "absorb": "scout", "purpose": "Both.", "reason": "Fewer votes."}),
        ),
        call(
            "remove_agent",
            json!({"name": "librarian", "reason": "It called the vote."}),
        ),
        says("I will wait."),
    ];
    let mut mayor = mayor(&home, &hamlet, responses);
    ask(&mut mayor, "tidy up").await;

    let told = tool_outputs(&mayor);
    assert_eq!(told.len(), 2);
    for told in &told {
        assert!(told.contains("vote 1 is open"), "{told}");
    }
    assert_eq!(hamlet.residents().len(), 3);
    // Once it has closed, the mayor decides again.
    vote::settle(&hamlet, &Rules::default(), 1).unwrap();
    assert!(vote::wait(&hamlet, &Rules::default()).is_ok());
}

#[tokio::test]
async fn the_mayor_removes_an_agent_and_those_who_try_it_are_told() {
    let home = place("agent-remove");
    let hamlet = hamlet_of_two(&home);
    hamlet.build_road("librarian", "scout", "test", "").unwrap();
    let responses = vec![
        call(
            "remove_agent",
            json!({"name": "scout", "reason": "Nobody asks it."}),
        ),
        call(
            "ask_agent",
            json!({"name": "librarian", "message": "Ask the scout who wrote Dune."}),
        ),
        // The librarian had a road to the scout.
        call(
            "ask_agent",
            json!({"name": "scout", "message": "Who wrote Dune?"}),
        ),
        call(
            "tell_agent",
            json!({"name": "scout", "message": "Who wrote Dune?"}),
        ),
        says("The scout is gone."),
        says("Done."),
    ];
    let mut mayor = mayor(&home, &hamlet, responses);
    let (_, events) = heard(&mut mayor, "tidy up").await;

    assert!(events.iter().any(|e| matches!(
        e,
        Event::AgentRemoved { name } if name == "scout"
    )));
    assert_eq!(hamlet.residents().len(), 1);
    assert!(!home.root.join("agents/scout").exists());
    assert!(tool_outputs(&mayor)[0].contains("archived as scout-"));
    let log = log::read_events(
        &home.root.join("agents/librarian/sessions/mayor.jsonl"),
    )
    .unwrap();
    let told = log
        .iter()
        .filter(|e| e["type"] == "tool")
        .filter_map(|e| e["output"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(told.len(), 2);
    for output in told {
        assert!(output.contains("`scout` was removed"));
        assert!(output.contains("Nobody asks it."));
    }
    // Nothing went along a road to it.
    assert!(hamlet.roads().iter().all(|r| !r.has("scout")));
}

#[tokio::test]
async fn the_mayor_merges_two_agents_with_one_model_call_for_the_notes()
 {
    let home = place("agent-merge");
    let hamlet = hamlet_of_two(&home);
    with_notes(&home);
    let responses = vec![
        call(
            "merge_agents",
            json!({"keep": "librarian", "absorb": "scout",
                "purpose": "Keeps the list and looks things up.", "reason": "They overlap."}),
        ),
        // The merged notes, written for the librarian.
        says("# Notes\nAlice likes tea. The wiki is slow.\n"),
        call(
            "ask_agent",
            json!({"name": "scout", "message": "Still there?"}),
        ),
        says("They are one now."),
    ];
    let mut mayor = mayor(&home, &hamlet, responses);
    let (reply, events) = heard(&mut mayor, "merge them").await;
    assert_eq!(reply, "They are one now.");

    assert!(events.iter().any(|e| matches!(
        e,
        Event::AgentsMerged { kept, gone }
            if kept == "librarian" && gone == "scout"
    )));
    let merged =
        std::fs::read_to_string(notes(&home, "librarian")).unwrap();
    assert!(
        merged.starts_with(
            "# Notes\nAlice likes tea. The wiki is slow."
        )
    );
    assert!(merged.contains("## Merged"));
    // The call for the notes is counted with the mayor's own.
    let calls = log::read_events(mayor.log_path())
        .unwrap()
        .iter()
        .filter(|e| e["type"] == "model_call")
        .count();
    assert_eq!(calls, 4);
    // And charged to the mayor.
    assert_eq!(mayor_calls(&home), 4);
    let outputs = tool_outputs(&mayor);
    assert!(
        outputs[0].starts_with("Merged `scout` into `librarian`")
    );
    assert!(!outputs[0].contains("joined"));
    assert!(outputs[1].contains("talk to `librarian`"));
}

#[tokio::test]
async fn an_agent_removed_in_the_middle_of_its_turn_stops() {
    let home = place("agent-gone-midturn");
    let hamlet = hamlet_of_two(&home);
    let responses = vec![
        call(
            "ask_agent",
            json!({"name": "scout", "message": "Find out who wrote Dune."}),
        ),
        // The scout asks back, and the mayor removes it while
        // it waits.
        call(
            "ask_agent",
            json!({"name": "mayor", "message": "Which Dune?"}),
        ),
        call(
            "remove_agent",
            json!({"name": "scout", "reason": "It asks too much."}),
        ),
        says("Never mind."),
        says("The scout is removed."),
    ];
    let mut mayor = mayor(&home, &hamlet, responses);
    let reply = ask(&mut mayor, "who wrote Dune?").await;
    assert_eq!(reply, "The scout is removed.");
    // The mayor, waiting on the scout, learns why no reply came.
    let outputs = tool_outputs(&mayor);
    assert!(outputs[0].contains("`scout` could not finish"));
    assert!(outputs[0].contains("It asks too much."));
    // The scout's turn ended in its archived log.
    let archive = hamlet.archived().remove(0).id;
    let log = log::read_events(
        &home
            .root
            .join("archive")
            .join(archive)
            .join("house/sessions/mayor.jsonl"),
    )
    .unwrap();
    assert_eq!(log.last().unwrap()["type"], "turn_aborted");
}

/// The count of the mayor's model calls in the ledger.
fn mayor_calls(home: &Home) -> usize {
    budget::Ledger::new(&home.root)
        .since(chrono::NaiveDate::MIN)
        .into_iter()
        .filter(|(_, line)| line["agent"] == MAYOR)
        .count()
}

#[tokio::test]
async fn the_call_for_merged_notes_is_stopped_when_the_budget_is_spent()
 {
    let home = place("agent-notes-budget");
    let hamlet = hamlet_of_two(&home);
    with_notes(&home);
    // The mayor's first call spends the budget, so the call for
    // the notes is never made.
    limit(&home, "[budget]\ncost = 0.000001\n");
    let responses = vec![
        call(
            "merge_agents",
            json!({"keep": "librarian", "absorb": "scout", "purpose": "Both.", "reason": "x"}),
        ),
        says("never said"),
    ];
    let mut mayor = mayor(&home, &hamlet, responses);
    let reply = ask(&mut mayor, "merge them").await;
    assert!(reply.starts_with("[stopped: The hamlet has spent"));
    let said = last_result(&mayor);
    assert!(
        said.contains(
            "No model was asked to rewrite them: The hamlet has \
             spent its budget"
        ),
        "{said}"
    );
    // The merge itself costs nothing and went ahead.
    assert_eq!(hamlet.residents().len(), 1);
    let merged =
        std::fs::read_to_string(notes(&home, "librarian")).unwrap();
    assert!(merged.contains("## From scout\nThe wiki is slow."));
    assert_eq!(mayor_calls(&home), 1);
}

fn civics(events: &[Event]) -> Vec<Civic> {
    events
        .iter()
        .filter_map(|e| match innermost(e) {
            Event::Civic(civic) => Some(civic.clone()),
            _ => None,
        })
        .collect()
}

fn ballot(choice: &str, evidence: &str) -> Value {
    call(
        "cast_ballot",
        json!({"choice": choice, "reason": "From my own work.", "evidence": evidence}),
    )
}

#[tokio::test]
async fn a_vote_replaces_the_mayor_in_the_middle_of_its_turn() {
    let home = place("agent-vote");
    let hamlet = hamlet_of_two(&home);
    hamlet.create("smith", "Fixes things.", "test").unwrap();
    let house = hamlet.house("librarian").unwrap();
    std::fs::write(
        house.notes(),
        "# Notes\nThe mayor lost my list twice this week.\n",
    )
    .unwrap();
    let responses = vec![
        call(
            "ask_agent",
            json!({"name": "librarian", "message": "Sort the list."}),
        ),
        call(
            "call_vote",
            json!({"kind": "replace_mayor", "candidate": "scout", "reason": "It lost my list."}),
        ),
        // The mayor is heard once, in its own house.
        says("The list was mislaid once and found again."),
        // Each ballot is a turn in the voter's own house.
        ballot("yes", "the mayor lost my list twice"),
        ballot("yes", "nothing I wrote down"),
        says("I would rather not say."),
        says("The vote passed."),
        // The mayor it replaced is still in its turn.
        call(
            "create_agent",
            json!({"name": "guard", "purpose": "Guards.", "reason": "To stay."}),
        ),
        says("I am no longer the mayor."),
    ];
    let mut mayor = mayor(&home, &hamlet, responses);
    let (_, events) = heard(&mut mayor, "tidy up").await;

    assert_eq!(hamlet.mayor().name, "scout");
    assert!(hamlet.member("guard").is_none());
    assert!(last_result(&mayor).contains("you are not the mayor"));
    // The chief is told when it is called and when it closes.
    let told = civics(&events);
    assert!(matches!(
        &told[0],
        Civic::VoteCalled { id: 1, by, .. } if by == "librarian"
    ));
    assert_eq!(
        told.iter()
            .filter(|c| matches!(c, Civic::Ballot { .. }))
            .count(),
        3
    );
    assert!(told.contains(&Civic::MayorChanged {
        hamlet: HOME.into(),
        from: FOUNDER.into(),
        to: "scout".into(),
    }));

    let vote = vote::find(&hamlet, 1).unwrap();
    assert_eq!(vote.standing, Standing::Passed);
    let of = |voter: &str| {
        vote.ballots.iter().find(|b| b.voter == voter).unwrap()
    };
    // Evidence is checked against the voter's own house.
    assert!(of("librarian").grounded);
    assert!(!of("scout").grounded);
    assert_eq!(of("smith").choice, None);
    // The ballot was asked in the voter's house, with its own
    // history, and the caller is told only the count.
    let asked = log::read_events(
        &home.root.join("agents/librarian/sessions/vote_1.jsonl"),
    )
    .unwrap();
    assert!(
        asked[0]["content"]
            .as_str()
            .unwrap()
            .contains("mayor said to you: Sort the list.")
    );
    let result = log::read_events(
        &home.root.join("agents/librarian/sessions/mayor.jsonl"),
    )
    .unwrap();
    let result = result
        .iter()
        .find(|e| e["type"] == "tool" && e["name"] == "call_vote")
        .unwrap()["output"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(result.contains("Vote 1 passed: 2 yes of 3 agents"));
    assert!(!result.contains("smith"));
    // Every ballot was the hamlet's to pay, under the vote's own
    // task.
    let ledger = budget::Ledger::new(&home.root);
    let ballots = ledger
        .since(budget::today())
        .into_iter()
        .filter(|(_, line)| line["agent"] == vote::PAYER)
        .map(|(_, line)| line["task"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    // Three ballots and the mayor's answer.
    assert_eq!(ballots.len(), 4);
    assert!(ballots.iter().all(|task| task.ends_with("-vote")));
    // Every voter was shown what the mayor said.
    assert_eq!(
        vote::find(&hamlet, 1).unwrap().answer.as_deref(),
        Some("The list was mislaid once and found again.")
    );
}

#[tokio::test]
async fn voters_that_fail_do_not_carry_a_vote() {
    let home = place("agent-silent");
    let hamlet = hamlet_of_two(&home);
    hamlet.create("smith", "Fixes things.", "test").unwrap();
    // The mayor's answer and one ballot, then the model stops
    // answering.
    let mut caller = open(
        &home,
        vec![says("I have nothing to add."), ballot("yes", "")],
    );
    caller.hamlet = Some(hamlet.again());
    caller.resident = hamlet.member("librarian");
    let motion = Motion::ReplaceMayor {
        candidate: "scout".into(),
    };
    let result = caller
        .hold_vote(motion, "x", None, &mut |_| {})
        .await
        .unwrap();
    assert!(result.contains("failed: 1 yes of 3 agents"));
    assert!(result.contains("2 did not answer"));
    assert_eq!(hamlet.mayor().name, FOUNDER);
    let vote = vote::find(&hamlet, 1).unwrap();
    assert!(vote.ballots[1].reason.contains("could not vote"));
    // A second vote can be called: the first is closed.
    assert!(matches!(vote.standing, Standing::Failed(_)));
}

#[tokio::test]
async fn a_voter_cannot_do_anything_but_vote() {
    let home = place("agent-only");
    let hamlet = hamlet_of_two(&home);
    hamlet.create("smith", "Fixes things.", "test").unwrap();
    let responses = vec![
        // The mayor tries a command where it is asked to answer,
        // then answers.
        call("bash", json!({"command": "touch said.txt"})),
        says("I stand by my work."),
        // The first voter tries a command and another vote.
        call("bash", json!({"command": "touch voted.txt"})),
        call(
            "call_vote",
            json!({"kind": "replace_mayor", "candidate": "smith", "reason": "x"}),
        ),
        ballot("maybe", ""),
        ballot("no", ""),
        ballot("abstain", ""),
    ];
    let mut caller = mayor(&home, &hamlet, responses);
    caller.resident = hamlet.member("scout");
    // Nothing a voter does is put to the gate.
    caller.gate = Some(Box::new(|_| {
        Box::pin(async { Ruling::Deny("asked".into()) })
    }));
    let motion = Motion::ReplaceMayor {
        candidate: "scout".into(),
    };
    caller
        .hold_vote(motion, "x", None, &mut |_| {})
        .await
        .unwrap();
    let house = home.root.join("agents/librarian");
    assert!(!house.join("workspace/voted.txt").exists());
    assert!(!home.root.join("workspace/said.txt").exists());
    let vote = vote::find(&hamlet, 1).unwrap();
    assert_eq!(vote::votes(&hamlet).len(), 1);
    // It ran out of steps without a valid ballot.
    assert_eq!(vote.ballots[0].choice, None);
    assert_eq!(vote.ballots[1].choice, Some(Choice::No));
    assert_eq!(vote.ballots[2].choice, Some(Choice::Abstain));
}

#[tokio::test]
async fn mayors_agree_a_road_and_a_move_between_hamlets() {
    let home = place("agent-across");
    let realm = Realm::open(&home.root).unwrap();
    let hamlet = hamlet_of_two(&home);
    let human = Origin::Human {
        user: "alice".into(),
    };
    let mill =
        realm.found("mill", "miller", "Grinds.", human).unwrap();
    mill.create("grinder", "Turns the stone.", "test").unwrap();
    let responses = vec![
        call(
            "agree_road",
            json!({"a": "scout", "b": "grinder", "reason": "Flour prices."}),
        ),
        // The other mayor, asked along the mayors' road.
        call(
            "agree_road",
            json!({"a": "scout", "b": "grinder", "reason": "Fine."}),
        ),
        says("Agreed."),
        call(
            "ask_agent",
            json!({"name": "scout", "message": "Ask the grinder the price."}),
        ),
        call(
            "ask_agent",
            json!({"name": "grinder", "message": "What is the price?"}),
        ),
        says("Two coins."),
        says("The grinder says two coins."),
        call(
            "agree_move",
            json!({"agent": "librarian", "to": "mill", "reason": "It reads there."}),
        ),
        call(
            "agree_move",
            json!({"agent": "librarian", "to": "mill", "reason": "Welcome."}),
        ),
        says("It may come."),
        call(
            "ask_removal",
            json!({"hamlet": "mill", "reason": "Too far."}),
        ),
        says("Done."),
    ];
    let mut mayor = mayor(&home, &hamlet, responses);
    let (reply, events) =
        heard(&mut mayor, "trade with the mill").await;
    assert_eq!(reply, "Done.");

    assert!(realm.has_road("scout", "grinder"));
    // The asker pays: the two questions to the other mayor and the
    // scout's to the grinder.
    assert_eq!((realm.paid(HOME), realm.paid("mill")), (3, 0));
    let there = home.root.join("hamlets/mill/agents");
    assert!(there.join("grinder/sessions/scout.jsonl").exists());
    assert!(there.join("miller/sessions/mayor@home.jsonl").exists());
    assert_eq!(realm.hamlet_of("librarian").as_deref(), Some("mill"));
    assert!(there.join("librarian/workspace/NOTES.md").exists());
    let told = civics(&events);
    assert!(told.contains(&Civic::RoadAgreed {
        a: "scout".into(),
        b: "grinder".into(),
        built: true,
    }));
    assert!(told.contains(&Civic::AgentMoved {
        agent: "librarian".into(),
        from: HOME.into(),
        to: "mill".into(),
    }));
    // One other mayor is not enough to ask the chief.
    assert!(told.contains(&Civic::RemovalAsked {
        hamlet: "mill".into(),
        by: HOME.into(),
        ready: false,
    }));
    assert!(mill.root().exists());
}

#[tokio::test]
async fn a_rewind_takes_a_turn_out_of_what_the_model_is_sent() {
    let home = place("agent-rewind");
    let mut agent =
        open(&home, vec![says("1"), says("2"), says("3")]);
    ask(&mut agent, "one").await;
    ask(&mut agent, "two").await;
    let line = log::read_events(agent.log_path())
        .unwrap()
        .iter()
        .position(|line| line["content"] == "two")
        .unwrap();
    agent.rewind(line).unwrap();
    assert_eq!(agent.unreflected, 1);
    ask(&mut agent, "three").await;
    let said = |agent: &Agent| {
        agent
            .messages
            .iter()
            .map(|m| m["content"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(said(&agent), ["one", "1", "three", "3"]);
    // An agent opened on the log later is sent the same.
    assert_eq!(said(&open(&home, vec![])), said(&agent));
}
